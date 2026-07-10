//! Persistent, tool-neutral deck review artifacts.
//!
//! The on-disk format is deliberately JSON rather than a server-private database. Version 2
//! contains a canonical deck identity, the build manifest against which targets were captured,
//! and annotations. `state` describes human workflow and is independent of `freshness`, which is
//! derived from the current build manifest. Artifacts live below XDG state and are keyed by the
//! SHA-256 of the canonical deck root; no constructor accepts an artifact path inside a deck.
//!
//! Writers take a portable sidecar lock, reload while holding it, check the caller's revision,
//! and replace the JSON through a synced temporary file in the artifact directory. This makes the
//! repository suitable for both the review server and short-lived CLI processes.

use fs4::fs_std::FileExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    fmt, fs,
    fs::{File, OpenOptions},
    io::{self, Read, Write},
    path::{Component, Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

pub const REVIEW_SCHEMA_VERSION: u32 = 2;
pub const REVIEW_CANVAS_WIDTH: f64 = 1920.0;
pub const REVIEW_CANVAS_HEIGHT: f64 = 1080.0;
pub const MAX_REVIEW_ARTIFACT_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_REVIEW_ANNOTATIONS: usize = 1024;
pub const MAX_REVIEW_MANIFEST_SLIDES: usize = 4096;
pub const MAX_REVIEW_BODY_BYTES: usize = 8 * 1024;
pub const MAX_REVIEW_HINT_BYTES: usize = 512;
pub const MAX_REVIEW_DISPOSITION_NOTE_BYTES: usize = 2 * 1024;
const MAX_PATH_BYTES: usize = 16 * 1024;
const MAX_ID_BYTES: usize = 256;
const MAX_VERIFICATION_COMMANDS: usize = 32;
const MAX_COMMAND_BYTES: usize = 2 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ReviewSnapshot {
    pub schema_version: u32,
    pub revision: u64,
    pub annotations: Vec<ReviewAnnotation>,
}

/// Complete durable JSON document. New optional fields must have serde defaults so readers can
/// continue to load early version-2 artifacts; incompatible changes require a schema migration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ReviewArtifact {
    pub schema_version: u32,
    pub revision: u64,
    pub deck: ReviewDeckIdentity,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build: Option<ReviewBuildManifest>,
    pub annotations: Vec<ReviewAnnotation>,
    #[serde(default = "default_next_annotation_id")]
    pub next_annotation_id: u64,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cleared_at_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ReviewDeckIdentity {
    /// Canonical absolute deck source root. It is identity/context, never a storage location.
    pub canonical_root: String,
    /// Lowercase SHA-256 of the platform representation of `canonical_root`.
    pub root_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ReviewBuildManifest {
    pub build_id: String,
    pub built_at_ms: u64,
    pub slides: Vec<ReviewSlideManifest>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub verification_commands: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ReviewSlideManifest {
    pub slide_id: String,
    pub source_path: String,
    /// Tool-selected digest of the source represented by this build (normally SHA-256).
    pub source_digest: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ReviewAnnotation {
    pub id: String,
    pub slide_id: String,
    pub source_path: String,
    pub target: ReviewTarget,
    pub body: String,
    pub kind: ReviewKind,
    pub action: Option<ReviewAction>,
    /// Human workflow state. This is never inferred from build freshness.
    pub state: ReviewState,
    /// Explicit reviewer/agent disposition, independent of resolution and freshness.
    #[serde(default)]
    pub disposition: ReviewDisposition,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disposition_note: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disposition_updated_at_ms: Option<u64>,
    /// Derived by matching the captured source digest to the current build manifest.
    #[serde(default)]
    pub freshness: ReviewFreshness,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub captured_build_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub captured_source_digest: Option<String>,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ReviewTarget {
    Point {
        x: f64,
        y: f64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        selector_hint: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        text_hint: Option<String>,
    },
    Region {
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        selector_hint: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        text_hint: Option<String>,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReviewKind {
    Note,
    Issue,
    Question,
    Praise,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReviewAction {
    Fix,
    Explain,
    Test,
    FollowUp,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReviewState {
    Todo,
    Resolved,
}

#[derive(Debug, Default, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReviewDisposition {
    #[default]
    Pending,
    Addressed,
    WontFix,
    Deferred,
}

#[derive(Debug, Default, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReviewFreshness {
    #[default]
    Current,
    Stale,
    Orphaned,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct NewReviewAnnotation {
    pub slide_id: String,
    pub source_path: String,
    pub target: ReviewTarget,
    pub body: String,
    pub kind: ReviewKind,
    pub action: Option<ReviewAction>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum ReviewMutation {
    Create {
        revision: u64,
        annotation: NewReviewAnnotation,
    },
    Edit {
        revision: u64,
        id: String,
        body: String,
        kind: ReviewKind,
        action: Option<ReviewAction>,
    },
    SetState {
        revision: u64,
        id: String,
        state: ReviewState,
    },
    SetDisposition {
        revision: u64,
        id: String,
        disposition: ReviewDisposition,
        #[serde(default)]
        note: Option<String>,
    },
    Delete {
        revision: u64,
        id: String,
    },
}

impl ReviewMutation {
    pub fn revision(&self) -> u64 {
        match self {
            Self::Create { revision, .. }
            | Self::Edit { revision, .. }
            | Self::SetState { revision, .. }
            | Self::SetDisposition { revision, .. }
            | Self::Delete { revision, .. } => *revision,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ReviewMutationError {
    Stale(ReviewSnapshot),
    Invalid(String),
    NotFound,
}

/// Compatibility in-memory store used by the existing review HTTP endpoint. New server and CLI
/// integrations should use [`ReviewRepository`] so mutations survive process exit.
#[derive(Debug)]
pub struct ReviewStore {
    revision: u64,
    annotations: Vec<ReviewAnnotation>,
    id_prefix: String,
    next_id: u64,
}

impl ReviewStore {
    pub fn new(id_prefix: String) -> Self {
        Self {
            revision: 0,
            annotations: Vec::new(),
            id_prefix,
            next_id: 1,
        }
    }

    pub fn snapshot(&self) -> ReviewSnapshot {
        ReviewSnapshot {
            schema_version: REVIEW_SCHEMA_VERSION,
            revision: self.revision,
            annotations: self.annotations.clone(),
        }
    }

    pub fn apply(
        &mut self,
        mutation: ReviewMutation,
    ) -> Result<ReviewSnapshot, ReviewMutationError> {
        if mutation.revision() != self.revision {
            return Err(ReviewMutationError::Stale(self.snapshot()));
        }
        if self.revision == u64::MAX {
            return Err(ReviewMutationError::Invalid(
                "review revision sequence exhausted".into(),
            ));
        }
        apply_mutation_to_annotations(
            &mut self.annotations,
            mutation,
            &self.id_prefix,
            &mut self.next_id,
            None,
        )?;
        self.revision += 1;
        Ok(self.snapshot())
    }
}

#[derive(Debug)]
pub enum ReviewRepositoryError {
    Io(io::Error),
    Malformed(String),
    Oversized { actual: u64, limit: usize },
    Invalid(String),
    Conflict(Box<ReviewArtifact>),
    NotFound,
}

impl fmt::Display for ReviewRepositoryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "review repository I/O error: {error}"),
            Self::Malformed(message) => write!(f, "malformed review artifact: {message}"),
            Self::Oversized { actual, limit } => {
                write!(f, "review artifact is {actual} bytes; limit is {limit}")
            }
            Self::Invalid(message) => write!(f, "invalid review data: {message}"),
            Self::Conflict(artifact) => write!(
                f,
                "review revision conflict (current revision {})",
                artifact.revision
            ),
            Self::NotFound => write!(f, "review annotation not found"),
        }
    }
}

impl std::error::Error for ReviewRepositoryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

impl From<io::Error> for ReviewRepositoryError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

/// Repository for the one active artifact associated with a canonical deck root.
#[derive(Debug, Clone)]
pub struct ReviewRepository {
    deck_root: PathBuf,
    deck: ReviewDeckIdentity,
    reviews_dir: PathBuf,
    artifact_path: PathBuf,
    lock_path: PathBuf,
}

impl ReviewRepository {
    /// Uses `$XDG_STATE_HOME/sideshow/reviews`, falling back to
    /// `$HOME/.local/state/sideshow/reviews` as specified by the XDG base directory convention.
    pub fn new(deck_root: &Path) -> Result<Self, ReviewRepositoryError> {
        Self::with_state_root(deck_root, xdg_state_home()?)
    }

    /// State-root-injectable constructor for embedding and tests. `state_root` is the XDG state
    /// root, not the final sideshow directory, and must be absolute and outside the deck tree.
    pub fn with_state_root(
        deck_root: &Path,
        state_root: impl AsRef<Path>,
    ) -> Result<Self, ReviewRepositoryError> {
        let deck_root = fs::canonicalize(deck_root).map_err(|error| {
            ReviewRepositoryError::Invalid(format!(
                "cannot canonicalize deck root {}: {error}",
                deck_root.display()
            ))
        })?;
        if !deck_root.is_dir() {
            return Err(ReviewRepositoryError::Invalid(
                "deck root must be a directory".into(),
            ));
        }
        let canonical_root = deck_root
            .to_str()
            .ok_or_else(|| {
                ReviewRepositoryError::Invalid(
                    "deck root must be valid UTF-8 for JSON handoff".into(),
                )
            })?
            .to_owned();
        validate_single_line_value(
            "canonical deck root",
            &canonical_root,
            MAX_PATH_BYTES,
            false,
        )?;
        let root_key = deck_root_key(&deck_root);
        let state_root = canonicalize_intended_path(state_root.as_ref())?;
        if state_root.starts_with(&deck_root) {
            return Err(ReviewRepositoryError::Invalid(
                "XDG state root must be outside the deck tree".into(),
            ));
        }
        // Resolve the nearest existing ancestor before creating anything. This catches an
        // existing `sideshow` symlink into the deck and prevents even an empty review directory
        // from being created there.
        let reviews_dir = canonicalize_intended_path(&state_root.join("sideshow/reviews"))?;
        if reviews_dir.starts_with(&deck_root) {
            return Err(ReviewRepositoryError::Invalid(
                "resolved XDG review state directory must be outside the deck tree".into(),
            ));
        }
        let artifact_path = reviews_dir.join(format!("{root_key}.json"));
        let lock_path = reviews_dir.join(format!("{root_key}.lock"));
        Ok(Self {
            deck_root,
            deck: ReviewDeckIdentity {
                canonical_root,
                root_key,
            },
            reviews_dir,
            artifact_path,
            lock_path,
        })
    }

    pub fn deck_root(&self) -> &Path {
        &self.deck_root
    }

    pub fn artifact_path(&self) -> &Path {
        &self.artifact_path
    }

    pub fn lock_path(&self) -> &Path {
        &self.lock_path
    }

    /// Returns an empty revision-0 artifact when no file exists. Missing, migrated v1, and
    /// normalized early-v2 artifacts are durably written as canonical v2 while holding the
    /// sidecar lock.
    pub fn load_artifact(&self) -> Result<ReviewArtifact, ReviewRepositoryError> {
        self.with_exclusive_lock(|repository| {
            let loaded = repository.read_unlocked()?;
            if loaded.needs_rewrite {
                repository.write_unlocked(&loaded.artifact)?;
            }
            Ok(loaded.artifact)
        })
    }

    pub fn load_snapshot(&self) -> Result<ReviewSnapshot, ReviewRepositoryError> {
        Ok(self.load_artifact()?.snapshot())
    }

    /// Process-safe read-modify-write with optimistic revision checking.
    pub fn apply_mutation(
        &self,
        mutation: ReviewMutation,
    ) -> Result<ReviewArtifact, ReviewRepositoryError> {
        self.with_exclusive_lock(|repository| {
            let mut artifact = repository.read_unlocked()?.artifact;
            check_revision(&artifact, mutation.revision())?;
            ensure_revision_available(&artifact)?;
            let prefix = &repository.deck.root_key[..12];
            let build = artifact.build.as_ref();
            apply_mutation_to_annotations(
                &mut artifact.annotations,
                mutation,
                prefix,
                &mut artifact.next_annotation_id,
                build,
            )
            .map_err(repository_mutation_error)?;
            bump_artifact(&mut artifact);
            validate_artifact(&artifact, &repository.deck)?;
            repository.write_unlocked(&artifact)?;
            Ok(artifact)
        })
    }

    /// Replaces the current build identity/manifest and derives current/stale/orphaned freshness.
    /// Annotations are never dropped, including unresolved annotations whose slides disappeared.
    pub fn update_build_manifest(
        &self,
        expected_revision: u64,
        manifest: ReviewBuildManifest,
    ) -> Result<ReviewArtifact, ReviewRepositoryError> {
        validate_manifest(&manifest)?;
        self.with_exclusive_lock(|repository| {
            let loaded = repository.read_unlocked()?;
            let mut artifact = loaded.artifact;
            check_revision(&artifact, expected_revision)?;
            if artifact
                .build
                .as_ref()
                .is_some_and(|current| manifests_match_ignoring_time(current, &manifest))
            {
                let advances_ordering_watermark = artifact
                    .build
                    .as_ref()
                    .is_some_and(|current| manifest.built_at_ms > current.built_at_ms);
                if advances_ordering_watermark {
                    artifact.build.as_mut().unwrap().built_at_ms = manifest.built_at_ms;
                }
                if loaded.needs_rewrite || advances_ordering_watermark {
                    repository.write_unlocked(&artifact)?;
                }
                return Ok(artifact);
            }
            if let Some(current) = &artifact.build
                && manifest.built_at_ms < current.built_at_ms
            {
                return Err(ReviewRepositoryError::Invalid(format!(
                    "build manifest timestamp {} is older than current timestamp {}",
                    manifest.built_at_ms, current.built_at_ms
                )));
            }
            ensure_revision_available(&artifact)?;
            for annotation in &mut artifact.annotations {
                annotation.freshness = freshness_for(annotation, &manifest);
            }
            artifact.build = Some(manifest);
            bump_artifact(&mut artifact);
            validate_artifact(&artifact, &repository.deck)?;
            repository.write_unlocked(&artifact)?;
            Ok(artifact)
        })
    }

    /// Explicit clear writes an empty next revision; it never removes the artifact.
    pub fn clear(&self, expected_revision: u64) -> Result<ReviewArtifact, ReviewRepositoryError> {
        self.with_exclusive_lock(|repository| {
            let mut artifact = repository.read_unlocked()?.artifact;
            check_revision(&artifact, expected_revision)?;
            ensure_revision_available(&artifact)?;
            artifact.annotations.clear();
            artifact.cleared_at_ms = Some(unix_time_ms());
            bump_artifact(&mut artifact);
            repository.write_unlocked(&artifact)?;
            Ok(artifact)
        })
    }

    pub fn resolve(
        &self,
        expected_revision: u64,
        id: impl Into<String>,
        resolved: bool,
    ) -> Result<ReviewArtifact, ReviewRepositoryError> {
        self.apply_mutation(ReviewMutation::SetState {
            revision: expected_revision,
            id: id.into(),
            state: if resolved {
                ReviewState::Resolved
            } else {
                ReviewState::Todo
            },
        })
    }

    pub fn set_disposition(
        &self,
        expected_revision: u64,
        id: impl Into<String>,
        disposition: ReviewDisposition,
        note: Option<String>,
    ) -> Result<ReviewArtifact, ReviewRepositoryError> {
        self.apply_mutation(ReviewMutation::SetDisposition {
            revision: expected_revision,
            id: id.into(),
            disposition,
            note,
        })
    }

    pub fn handoff_json(&self) -> Result<String, ReviewRepositoryError> {
        serde_json::to_string_pretty(&self.load_artifact()?)
            .map(|json| format!("{json}\n"))
            .map_err(|error| ReviewRepositoryError::Malformed(error.to_string()))
    }

    /// Prompt-oriented view; JSON remains the canonical machine-readable handoff.
    pub fn handoff_markdown(&self) -> Result<String, ReviewRepositoryError> {
        let artifact = self.load_artifact()?;
        let mut output = format!(
            "# Sideshow review handoff\n\n- Deck root: {}\n- Artifact: {}\n- Revision: {}\n",
            markdown_inline_code(&artifact.deck.canonical_root),
            markdown_inline_code(&self.artifact_path.display().to_string()),
            artifact.revision
        );
        if let Some(build) = &artifact.build {
            output.push_str(&format!(
                "- Build: {}\n",
                markdown_inline_code(&build.build_id)
            ));
        } else {
            output.push_str("- Build: not recorded\n");
        }
        output.push_str("\n## Annotations\n");
        if artifact.annotations.is_empty() {
            output.push_str("\nNo annotations.\n");
        }
        for (index, annotation) in artifact.annotations.iter().enumerate() {
            output.push_str(&format!(
                "\n### Annotation {}\n\n- ID: {}\n- Source: {}\n- Slide: {}\n- Workflow: `{:?}`\n- Freshness: `{:?}`\n- Disposition: `{:?}`\n- Kind/action: `{:?}` / `{}`\n- Target: `{}`\n",
                index + 1,
                markdown_inline_code(&annotation.id),
                markdown_inline_code(&annotation.source_path),
                markdown_inline_code(&annotation.slide_id),
                annotation.state,
                annotation.freshness,
                annotation.disposition,
                annotation.kind,
                annotation
                    .action
                    .map(|action| format!("{action:?}"))
                    .unwrap_or_else(|| "none".into()),
                target_summary(&annotation.target),
            ));
            let (selector_hint, text_hint) = target_hints(&annotation.target);
            if let Some(hint) = selector_hint {
                output.push_str(&format!(
                    "- Selector hint: {}\n",
                    markdown_inline_code(hint)
                ));
            }
            if let Some(hint) = text_hint {
                output.push_str(&format!("- Text hint: {}\n", markdown_inline_code(hint)));
            }
            output.push_str(&format!("\n{}\n", quote_markdown(&annotation.body)));
            if let Some(note) = &annotation.disposition_note {
                output.push_str(&format!(
                    "\nDisposition note:\n\n{}\n",
                    quote_markdown(note)
                ));
            }
        }
        output.push_str("\n## Verification\n\n");
        let commands = artifact
            .build
            .as_ref()
            .map(|build| build.verification_commands.as_slice())
            .unwrap_or(&[]);
        if commands.is_empty() {
            let deck_root = shell_quote(&artifact.deck.canonical_root);
            output.push_str(&format!(
                "    sideshow check {deck_root}\n    sideshow build {deck_root}\n"
            ));
        } else {
            for command in commands {
                output.push_str("    ");
                output.push_str(command);
                output.push('\n');
            }
        }
        Ok(output)
    }

    fn with_exclusive_lock<T>(
        &self,
        operation: impl FnOnce(&Self) -> Result<T, ReviewRepositoryError>,
    ) -> Result<T, ReviewRepositoryError> {
        self.ensure_layout()?;
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true);
        set_private_file_mode(&mut options);
        let lock = options.open(&self.lock_path)?;
        lock.lock_exclusive()?;
        let result = operation(self);
        let unlock_result = lock.unlock();
        match (result, unlock_result) {
            (Ok(value), Ok(())) => Ok(value),
            (Err(error), _) => Err(error),
            (Ok(_), Err(error)) => Err(error.into()),
        }
    }

    fn ensure_layout(&self) -> Result<(), ReviewRepositoryError> {
        create_private_dir_all(&self.reviews_dir)?;
        let canonical_state = fs::canonicalize(&self.reviews_dir)?;
        if canonical_state.starts_with(&self.deck_root) {
            return Err(ReviewRepositoryError::Invalid(
                "resolved XDG review state directory is inside the deck tree".into(),
            ));
        }
        Ok(())
    }

    fn read_unlocked(&self) -> Result<LoadedArtifact, ReviewRepositoryError> {
        let mut file = match File::open(&self.artifact_path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(LoadedArtifact {
                    artifact: ReviewArtifact::empty(self.deck.clone()),
                    needs_rewrite: true,
                });
            }
            Err(error) => return Err(error.into()),
        };
        let metadata_len = file.metadata()?.len();
        if metadata_len > MAX_REVIEW_ARTIFACT_BYTES as u64 {
            return Err(ReviewRepositoryError::Oversized {
                actual: metadata_len,
                limit: MAX_REVIEW_ARTIFACT_BYTES,
            });
        }
        let mut bytes = Vec::with_capacity(metadata_len as usize);
        Read::by_ref(&mut file)
            .take((MAX_REVIEW_ARTIFACT_BYTES + 1) as u64)
            .read_to_end(&mut bytes)?;
        if bytes.len() > MAX_REVIEW_ARTIFACT_BYTES {
            return Err(ReviewRepositoryError::Oversized {
                actual: bytes.len() as u64,
                limit: MAX_REVIEW_ARTIFACT_BYTES,
            });
        }
        let value: serde_json::Value = serde_json::from_slice(&bytes)
            .map_err(|error| ReviewRepositoryError::Malformed(error.to_string()))?;
        let original_value = value.clone();
        let version = value
            .get("schema_version")
            .and_then(serde_json::Value::as_u64)
            .ok_or_else(|| ReviewRepositoryError::Malformed("missing schema_version".into()))?;
        let (mut artifact, migrated) = match version {
            1 => {
                let legacy: LegacyReviewSnapshot = serde_json::from_value(value)
                    .map_err(|error| ReviewRepositoryError::Malformed(error.to_string()))?;
                (ReviewArtifact::from_v1(self.deck.clone(), legacy), true)
            }
            version if version == REVIEW_SCHEMA_VERSION as u64 => {
                let artifact: ReviewArtifact = serde_json::from_value(value)
                    .map_err(|error| ReviewRepositoryError::Malformed(error.to_string()))?;
                (artifact, false)
            }
            other => {
                return Err(ReviewRepositoryError::Malformed(format!(
                    "unsupported schema version {other}"
                )));
            }
        };
        artifact.next_annotation_id = artifact
            .next_annotation_id
            .max(infer_next_id(&artifact.annotations));
        validate_artifact(&artifact, &self.deck)?;
        if let Some(manifest) = &artifact.build {
            for annotation in &mut artifact.annotations {
                annotation.freshness = freshness_for(annotation, manifest);
            }
        }
        let normalized_value = serde_json::to_value(&artifact)
            .map_err(|error| ReviewRepositoryError::Malformed(error.to_string()))?;
        Ok(LoadedArtifact {
            artifact,
            needs_rewrite: migrated || normalized_value != original_value,
        })
    }

    fn write_unlocked(&self, artifact: &ReviewArtifact) -> Result<(), ReviewRepositoryError> {
        validate_artifact(artifact, &self.deck)?;
        let bytes = serde_json::to_vec_pretty(artifact)
            .map_err(|error| ReviewRepositoryError::Malformed(error.to_string()))?;
        if bytes.len() + 1 > MAX_REVIEW_ARTIFACT_BYTES {
            return Err(ReviewRepositoryError::Oversized {
                actual: (bytes.len() + 1) as u64,
                limit: MAX_REVIEW_ARTIFACT_BYTES,
            });
        }
        let mut options = atomic_write_file::OpenOptions::new();
        #[cfg(unix)]
        {
            use atomic_write_file::unix::OpenOptionsExt as AtomicOpenOptionsExt;
            use std::os::unix::fs::OpenOptionsExt as StdOpenOptionsExt;
            options.preserve_mode(false);
            options.preserve_owner(false);
            options.mode(0o600);
        }
        let mut file = options.open(&self.artifact_path)?;
        file.write_all(&bytes)?;
        file.write_all(b"\n")?;
        file.commit()?;
        Ok(())
    }
}

impl ReviewArtifact {
    fn empty(deck: ReviewDeckIdentity) -> Self {
        let now = unix_time_ms();
        Self {
            schema_version: REVIEW_SCHEMA_VERSION,
            revision: 0,
            deck,
            build: None,
            annotations: Vec::new(),
            next_annotation_id: 1,
            created_at_ms: now,
            updated_at_ms: now,
            cleared_at_ms: None,
        }
    }

    fn from_v1(deck: ReviewDeckIdentity, legacy: LegacyReviewSnapshot) -> Self {
        let now = unix_time_ms();
        let latest_timestamp = legacy
            .annotations
            .iter()
            .map(|annotation| annotation.updated_at_ms)
            .max()
            .unwrap_or(now);
        let earliest_timestamp = legacy
            .annotations
            .iter()
            .map(|annotation| annotation.created_at_ms)
            .min()
            .unwrap_or(now);
        Self {
            schema_version: REVIEW_SCHEMA_VERSION,
            revision: legacy.revision,
            deck,
            build: None,
            next_annotation_id: infer_next_id(&legacy.annotations),
            annotations: legacy.annotations,
            created_at_ms: earliest_timestamp,
            updated_at_ms: latest_timestamp,
            cleared_at_ms: None,
        }
    }

    pub fn snapshot(&self) -> ReviewSnapshot {
        ReviewSnapshot {
            schema_version: self.schema_version,
            revision: self.revision,
            annotations: self.annotations.clone(),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyReviewSnapshot {
    #[serde(rename = "schema_version")]
    _schema_version: u32,
    revision: u64,
    annotations: Vec<ReviewAnnotation>,
}

struct LoadedArtifact {
    artifact: ReviewArtifact,
    needs_rewrite: bool,
}

fn apply_mutation_to_annotations(
    annotations: &mut Vec<ReviewAnnotation>,
    mutation: ReviewMutation,
    id_prefix: &str,
    next_id: &mut u64,
    build: Option<&ReviewBuildManifest>,
) -> Result<(), ReviewMutationError> {
    match mutation {
        ReviewMutation::Create { annotation, .. } => {
            validate_new_annotation(&annotation)?;
            if annotations.len() >= MAX_REVIEW_ANNOTATIONS {
                return Err(ReviewMutationError::Invalid(format!(
                    "at most {MAX_REVIEW_ANNOTATIONS} annotations are allowed"
                )));
            }
            let now = unix_time_ms();
            let id = loop {
                let candidate = format!("r-{id_prefix}-{next_id}");
                *next_id = next_id.checked_add(1).ok_or_else(|| {
                    ReviewMutationError::Invalid("annotation id sequence exhausted".into())
                })?;
                if !annotations
                    .iter()
                    .any(|annotation| annotation.id == candidate)
                {
                    break candidate;
                }
            };
            let captured = match build {
                Some(manifest) => {
                    let slide = manifest
                        .slides
                        .iter()
                        .find(|slide| {
                            slide.slide_id == annotation.slide_id
                                && slide.source_path == annotation.source_path
                        })
                        .ok_or_else(|| {
                            ReviewMutationError::Invalid(
                                "annotation slide id and source path must exactly match a manifest slide"
                                    .into(),
                            )
                        })?;
                    Some((manifest.build_id.clone(), slide.source_digest.clone()))
                }
                None => None,
            };
            let mut created = ReviewAnnotation {
                id,
                slide_id: annotation.slide_id,
                source_path: annotation.source_path,
                target: annotation.target,
                body: annotation.body,
                kind: annotation.kind,
                action: annotation.action,
                state: ReviewState::Todo,
                disposition: ReviewDisposition::Pending,
                disposition_note: None,
                disposition_updated_at_ms: None,
                freshness: ReviewFreshness::Current,
                captured_build_id: captured.as_ref().map(|(id, _)| id.clone()),
                captured_source_digest: captured.map(|(_, digest)| digest),
                created_at_ms: now,
                updated_at_ms: now,
            };
            if let Some(manifest) = build {
                created.freshness = freshness_for(&created, manifest);
            }
            annotations.push(created);
        }
        ReviewMutation::Edit {
            id,
            body,
            kind,
            action,
            ..
        } => {
            validate_body(&body)?;
            let annotation = find_annotation_mut(annotations, &id)?;
            annotation.body = body;
            annotation.kind = kind;
            annotation.action = action;
            annotation.updated_at_ms = unix_time_ms();
        }
        ReviewMutation::SetState { id, state, .. } => {
            let annotation = find_annotation_mut(annotations, &id)?;
            annotation.state = state;
            annotation.updated_at_ms = unix_time_ms();
        }
        ReviewMutation::SetDisposition {
            id,
            disposition,
            note,
            ..
        } => {
            if let Some(note) = &note {
                validate_bounded_text(
                    "disposition note",
                    note,
                    MAX_REVIEW_DISPOSITION_NOTE_BYTES,
                    true,
                )?;
            }
            let annotation = find_annotation_mut(annotations, &id)?;
            let now = unix_time_ms();
            annotation.disposition = disposition;
            annotation.disposition_note = note;
            annotation.disposition_updated_at_ms = Some(now);
            annotation.updated_at_ms = now;
        }
        ReviewMutation::Delete { id, .. } => {
            let old_len = annotations.len();
            annotations.retain(|annotation| annotation.id != id);
            if annotations.len() == old_len {
                return Err(ReviewMutationError::NotFound);
            }
        }
    }
    Ok(())
}

fn find_annotation_mut<'a>(
    annotations: &'a mut [ReviewAnnotation],
    id: &str,
) -> Result<&'a mut ReviewAnnotation, ReviewMutationError> {
    annotations
        .iter_mut()
        .find(|annotation| annotation.id == id)
        .ok_or(ReviewMutationError::NotFound)
}

fn repository_mutation_error(error: ReviewMutationError) -> ReviewRepositoryError {
    match error {
        ReviewMutationError::Invalid(message) => ReviewRepositoryError::Invalid(message),
        ReviewMutationError::NotFound => ReviewRepositoryError::NotFound,
        ReviewMutationError::Stale(_) => {
            unreachable!("repository checks revisions before mutation")
        }
    }
}

fn check_revision(artifact: &ReviewArtifact, expected: u64) -> Result<(), ReviewRepositoryError> {
    if artifact.revision == expected {
        Ok(())
    } else {
        Err(ReviewRepositoryError::Conflict(Box::new(artifact.clone())))
    }
}

fn bump_artifact(artifact: &mut ReviewArtifact) {
    artifact.revision += 1;
    artifact.updated_at_ms = unix_time_ms();
}

fn ensure_revision_available(artifact: &ReviewArtifact) -> Result<(), ReviewRepositoryError> {
    if artifact.revision == u64::MAX {
        Err(ReviewRepositoryError::Invalid(
            "review revision sequence exhausted".into(),
        ))
    } else {
        Ok(())
    }
}

fn freshness_for(annotation: &ReviewAnnotation, manifest: &ReviewBuildManifest) -> ReviewFreshness {
    let Some(slide) = manifest
        .slides
        .iter()
        .find(|slide| slide.slide_id == annotation.slide_id)
    else {
        return ReviewFreshness::Orphaned;
    };
    if slide.source_path != annotation.source_path {
        return ReviewFreshness::Stale;
    }
    match &annotation.captured_source_digest {
        Some(digest) if digest == &slide.source_digest => ReviewFreshness::Current,
        _ => ReviewFreshness::Stale,
    }
}

fn manifests_match_ignoring_time(left: &ReviewBuildManifest, right: &ReviewBuildManifest) -> bool {
    left.build_id == right.build_id
        && left.slides == right.slides
        && left.verification_commands == right.verification_commands
}

fn validate_artifact(
    artifact: &ReviewArtifact,
    expected_deck: &ReviewDeckIdentity,
) -> Result<(), ReviewRepositoryError> {
    if artifact.schema_version != REVIEW_SCHEMA_VERSION {
        return Err(ReviewRepositoryError::Malformed(format!(
            "expected schema version {REVIEW_SCHEMA_VERSION}, got {}",
            artifact.schema_version
        )));
    }
    if &artifact.deck != expected_deck {
        return Err(ReviewRepositoryError::Invalid(
            "artifact deck identity does not match repository deck".into(),
        ));
    }
    if artifact.next_annotation_id == 0 {
        return Err(ReviewRepositoryError::Invalid(
            "next_annotation_id must be positive".into(),
        ));
    }
    if artifact.annotations.len() > MAX_REVIEW_ANNOTATIONS {
        return Err(ReviewRepositoryError::Invalid(format!(
            "at most {MAX_REVIEW_ANNOTATIONS} annotations are allowed"
        )));
    }
    if let Some(manifest) = &artifact.build {
        validate_manifest(manifest)?;
    }
    let mut ids = HashSet::with_capacity(artifact.annotations.len());
    for annotation in &artifact.annotations {
        validate_annotation(annotation)?;
        if !ids.insert(&annotation.id) {
            return Err(ReviewRepositoryError::Invalid(format!(
                "duplicate annotation id {}",
                annotation.id
            )));
        }
    }
    Ok(())
}

fn validate_manifest(manifest: &ReviewBuildManifest) -> Result<(), ReviewRepositoryError> {
    validate_single_line_value("build id", &manifest.build_id, MAX_ID_BYTES, false)?;
    if manifest.slides.len() > MAX_REVIEW_MANIFEST_SLIDES {
        return Err(ReviewRepositoryError::Invalid(format!(
            "at most {MAX_REVIEW_MANIFEST_SLIDES} manifest slides are allowed"
        )));
    }
    if manifest.verification_commands.len() > MAX_VERIFICATION_COMMANDS {
        return Err(ReviewRepositoryError::Invalid(format!(
            "at most {MAX_VERIFICATION_COMMANDS} verification commands are allowed"
        )));
    }
    let mut slide_ids = HashSet::with_capacity(manifest.slides.len());
    for slide in &manifest.slides {
        validate_single_line_value("slide id", &slide.slide_id, MAX_ID_BYTES, false)?;
        validate_source_path_value(&slide.source_path)?;
        validate_single_line_value("source digest", &slide.source_digest, MAX_ID_BYTES, false)?;
        if !slide_ids.insert(&slide.slide_id) {
            return Err(ReviewRepositoryError::Invalid(format!(
                "duplicate manifest slide id {}",
                slide.slide_id
            )));
        }
    }
    for command in &manifest.verification_commands {
        validate_single_line_value("verification command", command, MAX_COMMAND_BYTES, false)?;
    }
    Ok(())
}

fn validate_annotation(annotation: &ReviewAnnotation) -> Result<(), ReviewRepositoryError> {
    validate_single_line_value("annotation id", &annotation.id, MAX_ID_BYTES, false)?;
    validate_single_line_value("slide id", &annotation.slide_id, MAX_ID_BYTES, false)?;
    validate_source_path_value(&annotation.source_path)?;
    validate_bounded_text_value(
        "annotation body",
        &annotation.body,
        MAX_REVIEW_BODY_BYTES,
        false,
    )?;
    validate_target_value(&annotation.target)?;
    if let Some(note) = &annotation.disposition_note {
        validate_bounded_text_value(
            "disposition note",
            note,
            MAX_REVIEW_DISPOSITION_NOTE_BYTES,
            true,
        )?;
    }
    if let Some(build_id) = &annotation.captured_build_id {
        validate_single_line_value("captured build id", build_id, MAX_ID_BYTES, false)?;
    }
    if let Some(digest) = &annotation.captured_source_digest {
        validate_single_line_value("captured source digest", digest, MAX_ID_BYTES, false)?;
    }
    Ok(())
}

fn validate_new_annotation(annotation: &NewReviewAnnotation) -> Result<(), ReviewMutationError> {
    validate_single_line("slide id", &annotation.slide_id, MAX_ID_BYTES, false)?;
    validate_source_path(&annotation.source_path)?;
    validate_body(&annotation.body)?;
    validate_target(&annotation.target)
}

fn validate_body(body: &str) -> Result<(), ReviewMutationError> {
    validate_bounded_text("annotation body", body, MAX_REVIEW_BODY_BYTES, false)
}

fn validate_source_path(path: &str) -> Result<(), ReviewMutationError> {
    validate_single_line("source path", path, 1024, false)?;
    if !normalized_relative_path(path) {
        return Err(ReviewMutationError::Invalid(
            "source path must be a normalized relative path".into(),
        ));
    }
    Ok(())
}

fn validate_source_path_value(path: &str) -> Result<(), ReviewRepositoryError> {
    validate_single_line_value("source path", path, 1024, false)?;
    if !normalized_relative_path(path) {
        return Err(ReviewRepositoryError::Invalid(
            "source path must be a normalized relative path".into(),
        ));
    }
    Ok(())
}

fn normalized_relative_path(value: &str) -> bool {
    let path = Path::new(value);
    !value.contains('\\')
        && !value.starts_with('/')
        && !value.ends_with('/')
        && !value.split('/').any(str::is_empty)
        && !path.is_absolute()
        && path
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}

fn validate_bounded_text(
    name: &str,
    text: &str,
    max_bytes: usize,
    allow_empty: bool,
) -> Result<(), ReviewMutationError> {
    validate_text(name, text, max_bytes, allow_empty).map_err(ReviewMutationError::Invalid)
}

fn validate_bounded_text_value(
    name: &str,
    text: &str,
    max_bytes: usize,
    allow_empty: bool,
) -> Result<(), ReviewRepositoryError> {
    validate_text(name, text, max_bytes, allow_empty).map_err(ReviewRepositoryError::Invalid)
}

fn validate_single_line(
    name: &str,
    text: &str,
    max_bytes: usize,
    allow_empty: bool,
) -> Result<(), ReviewMutationError> {
    validate_single_line_inner(name, text, max_bytes, allow_empty)
        .map_err(ReviewMutationError::Invalid)
}

fn validate_single_line_value(
    name: &str,
    text: &str,
    max_bytes: usize,
    allow_empty: bool,
) -> Result<(), ReviewRepositoryError> {
    validate_single_line_inner(name, text, max_bytes, allow_empty)
        .map_err(ReviewRepositoryError::Invalid)
}

fn validate_single_line_inner(
    name: &str,
    text: &str,
    max_bytes: usize,
    allow_empty: bool,
) -> Result<(), String> {
    validate_text(name, text, max_bytes, allow_empty)?;
    if text.contains(['\n', '\r', '\t']) {
        return Err(format!("{name} must be a single line"));
    }
    Ok(())
}

fn validate_text(
    name: &str,
    text: &str,
    max_bytes: usize,
    allow_empty: bool,
) -> Result<(), String> {
    if (!allow_empty && text.trim().is_empty()) || text.len() > max_bytes {
        return Err(format!("{name} must contain 1..={max_bytes} bytes"));
    }
    if text
        .chars()
        .any(|character| character.is_control() && character != '\n' && character != '\t')
    {
        return Err(format!("{name} contains unsupported control characters"));
    }
    Ok(())
}

fn validate_target(target: &ReviewTarget) -> Result<(), ReviewMutationError> {
    validate_target_inner(target).map_err(ReviewMutationError::Invalid)
}

fn validate_target_value(target: &ReviewTarget) -> Result<(), ReviewRepositoryError> {
    validate_target_inner(target).map_err(ReviewRepositoryError::Invalid)
}

fn validate_target_inner(target: &ReviewTarget) -> Result<(), String> {
    let (x, y, width, height, selector_hint, text_hint) = match target {
        ReviewTarget::Point {
            x,
            y,
            selector_hint,
            text_hint,
        } => (*x, *y, 0.0, 0.0, selector_hint, text_hint),
        ReviewTarget::Region {
            x,
            y,
            width,
            height,
            selector_hint,
            text_hint,
        } => (*x, *y, *width, *height, selector_hint, text_hint),
    };
    if ![x, y, width, height].into_iter().all(f64::is_finite)
        || x < 0.0
        || y < 0.0
        || width < 0.0
        || height < 0.0
        || x + width > REVIEW_CANVAS_WIDTH
        || y + height > REVIEW_CANVAS_HEIGHT
    {
        return Err(format!(
            "target must fit within the {}x{} logical canvas",
            REVIEW_CANVAS_WIDTH as u32, REVIEW_CANVAS_HEIGHT as u32
        ));
    }
    if matches!(target, ReviewTarget::Region { .. }) && (width < 1.0 || height < 1.0) {
        return Err("region targets must have positive width and height".into());
    }
    if let Some(hint) = selector_hint {
        validate_text("selector hint", hint, MAX_REVIEW_HINT_BYTES, true)?;
    }
    if let Some(hint) = text_hint {
        validate_text("text hint", hint, MAX_REVIEW_HINT_BYTES, true)?;
    }
    Ok(())
}

fn xdg_state_home() -> Result<PathBuf, ReviewRepositoryError> {
    xdg_state_home_from(
        std::env::var_os("XDG_STATE_HOME").as_deref(),
        std::env::var_os("HOME").as_deref(),
    )
}

fn xdg_state_home_from(
    xdg_state_home: Option<&std::ffi::OsStr>,
    home: Option<&std::ffi::OsStr>,
) -> Result<PathBuf, ReviewRepositoryError> {
    if let Some(path) = xdg_state_home.filter(|path| !path.is_empty()) {
        let path = PathBuf::from(path);
        if !path.is_absolute() {
            return Err(ReviewRepositoryError::Invalid(
                "XDG_STATE_HOME must be absolute".into(),
            ));
        }
        return Ok(path);
    }
    let home = home.ok_or_else(|| {
        ReviewRepositoryError::Invalid("HOME is required when XDG_STATE_HOME is unset".into())
    })?;
    Ok(PathBuf::from(home).join(".local/state"))
}

fn canonicalize_intended_path(path: &Path) -> Result<PathBuf, ReviewRepositoryError> {
    if !path.is_absolute() {
        return Err(ReviewRepositoryError::Invalid(
            "review state root must be absolute".into(),
        ));
    }
    let mut existing = path;
    let mut suffix = Vec::new();
    loop {
        match fs::symlink_metadata(existing) {
            Ok(_) => break,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let name = existing.file_name().ok_or_else(|| {
                    ReviewRepositoryError::Invalid(format!(
                        "review state root {} has no existing ancestor",
                        path.display()
                    ))
                })?;
                suffix.push(name.to_owned());
                existing = existing.parent().ok_or_else(|| {
                    ReviewRepositoryError::Invalid(format!(
                        "review state root {} has no existing ancestor",
                        path.display()
                    ))
                })?;
            }
            Err(error) => return Err(error.into()),
        }
    }
    let mut resolved = fs::canonicalize(existing)?;
    for component in suffix.iter().rev() {
        resolved.push(component);
    }
    Ok(resolved)
}

fn deck_root_key(path: &Path) -> String {
    let mut hasher = Sha256::new();
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        hasher.update(path.as_os_str().as_bytes());
    }
    #[cfg(not(unix))]
    hasher.update(path.to_string_lossy().as_bytes());
    format!("{:x}", hasher.finalize())
}

fn infer_next_id(annotations: &[ReviewAnnotation]) -> u64 {
    annotations
        .iter()
        .filter_map(|annotation| annotation.id.rsplit('-').next()?.parse::<u64>().ok())
        .max()
        .unwrap_or(0)
        .saturating_add(1)
        .max(1)
}

fn default_next_annotation_id() -> u64 {
    1
}

fn create_private_dir_all(path: &Path) -> io::Result<()> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

fn set_private_file_mode(options: &mut OpenOptions) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
}

fn target_summary(target: &ReviewTarget) -> String {
    match target {
        ReviewTarget::Point { x, y, .. } => format!("point({x:.1}, {y:.1})"),
        ReviewTarget::Region {
            x,
            y,
            width,
            height,
            ..
        } => format!("region({x:.1}, {y:.1}, {width:.1}, {height:.1})"),
    }
}

fn target_hints(target: &ReviewTarget) -> (Option<&str>, Option<&str>) {
    match target {
        ReviewTarget::Point {
            selector_hint,
            text_hint,
            ..
        }
        | ReviewTarget::Region {
            selector_hint,
            text_hint,
            ..
        } => (selector_hint.as_deref(), text_hint.as_deref()),
    }
}

fn shell_quote(value: &str) -> String {
    if !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"/_-.".contains(&byte))
    {
        value.to_owned()
    } else {
        format!("'{}'", value.replace('\'', "'\\''"))
    }
}

fn markdown_inline_code(value: &str) -> String {
    let longest_run = value
        .split(|character| character != '`')
        .map(str::len)
        .max()
        .unwrap_or(0);
    let delimiter = "`".repeat(longest_run + 1);
    format!("{delimiter} {value} {delimiter}")
}

fn quote_markdown(value: &str) -> String {
    value
        .lines()
        .map(|line| format!("> {line}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn unix_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Barrier, Mutex};
    use tempfile::TempDir;

    fn point() -> ReviewTarget {
        ReviewTarget::Point {
            x: 100.0,
            y: 200.0,
            selector_hint: Some("h1.title".into()),
            text_hint: Some("Title".into()),
        }
    }

    fn new_annotation() -> NewReviewAnnotation {
        NewReviewAnnotation {
            slide_id: "s-01-title".into(),
            source_path: "slides/01-title.html".into(),
            target: point(),
            body: "Tighten this title".into(),
            kind: ReviewKind::Issue,
            action: Some(ReviewAction::Fix),
        }
    }

    fn create(revision: u64) -> ReviewMutation {
        ReviewMutation::Create {
            revision,
            annotation: new_annotation(),
        }
    }

    fn fixture() -> (TempDir, TempDir, ReviewRepository) {
        let deck = TempDir::new().unwrap();
        fs::write(deck.path().join("deck.toml"), "[deck]\ntitle='Test'\n").unwrap();
        let state = TempDir::new().unwrap();
        let repository = ReviewRepository::with_state_root(deck.path(), state.path()).unwrap();
        (deck, state, repository)
    }

    fn manifest(digest: &str) -> ReviewBuildManifest {
        ReviewBuildManifest {
            build_id: format!("build-{digest}"),
            built_at_ms: 1234,
            slides: vec![ReviewSlideManifest {
                slide_id: "s-01-title".into(),
                source_path: "slides/01-title.html".into(),
                source_digest: digest.into(),
            }],
            verification_commands: vec!["sideshow check .".into(), "sideshow build .".into()],
        }
    }

    fn manifest_at(digest: &str, built_at_ms: u64) -> ReviewBuildManifest {
        ReviewBuildManifest {
            built_at_ms,
            ..manifest(digest)
        }
    }

    #[test]
    fn in_memory_mutations_remain_compatible() {
        let mut store = ReviewStore::new("session".into());
        let created = store.apply(create(0)).unwrap();
        assert_eq!(created.revision, 1);
        assert_eq!(created.annotations[0].id, "r-session-1");

        let edited = store
            .apply(ReviewMutation::Edit {
                revision: 1,
                id: "r-session-1".into(),
                body: "Use a shorter title".into(),
                kind: ReviewKind::Note,
                action: None,
            })
            .unwrap();
        assert_eq!(edited.revision, 2);
        assert_eq!(edited.annotations[0].body, "Use a shorter title");
        assert_eq!(edited.annotations[0].slide_id, "s-01-title");
        assert_eq!(
            edited.annotations[0].disposition,
            ReviewDisposition::Pending
        );
    }

    #[test]
    fn stale_in_memory_mutation_returns_latest_snapshot() {
        let mut store = ReviewStore::new("session".into());
        store.apply(create(0)).unwrap();
        let ReviewMutationError::Stale(snapshot) = store.apply(create(0)).unwrap_err() else {
            panic!("expected stale revision");
        };
        assert_eq!(snapshot.revision, 1);
        assert_eq!(snapshot.annotations.len(), 1);
    }

    #[test]
    fn deterministic_xdg_path_is_outside_deck_and_keyed_by_canonical_root() {
        let (deck, state, first) = fixture();
        let second =
            ReviewRepository::with_state_root(&deck.path().join("."), state.path()).unwrap();
        assert_eq!(first.artifact_path(), second.artifact_path());
        assert!(
            first
                .artifact_path()
                .starts_with(state.path().canonicalize().unwrap())
        );
        assert!(!first.artifact_path().starts_with(deck.path()));
        assert_eq!(
            first
                .artifact_path()
                .extension()
                .and_then(|value| value.to_str()),
            Some("json")
        );
    }

    #[test]
    fn rejects_state_root_inside_deck_even_through_symlink() {
        let deck = TempDir::new().unwrap();
        assert!(matches!(
            ReviewRepository::with_state_root(deck.path(), deck.path().join("state")),
            Err(ReviewRepositoryError::Invalid(_))
        ));

        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            let outside = TempDir::new().unwrap();
            let linked = outside.path().join("linked-state");
            symlink(deck.path(), &linked).unwrap();
            assert!(matches!(
                ReviewRepository::with_state_root(deck.path(), &linked),
                Err(ReviewRepositoryError::Invalid(_))
            ));
        }
    }

    #[test]
    fn schema_round_trip_and_json_handoff() {
        let (_deck, _state, repository) = fixture();
        let with_build = repository
            .update_build_manifest(0, manifest("aaa"))
            .unwrap();
        let created = repository
            .apply_mutation(create(with_build.revision))
            .unwrap();
        let loaded = repository.load_artifact().unwrap();
        assert_eq!(loaded, created);
        assert_eq!(loaded.schema_version, REVIEW_SCHEMA_VERSION);
        assert_eq!(loaded.annotations[0].freshness, ReviewFreshness::Current);
        assert_eq!(
            serde_json::from_str::<ReviewArtifact>(&repository.handoff_json().unwrap()).unwrap(),
            loaded
        );
    }

    #[test]
    fn first_load_durably_creates_one_canonical_empty_v2_under_lock() {
        let (deck, state, repository) = fixture();
        let second = ReviewRepository::with_state_root(deck.path(), state.path()).unwrap();
        let barrier = Arc::new(Barrier::new(3));
        let workers = [repository.clone(), second]
            .into_iter()
            .map(|repository| {
                let barrier = Arc::clone(&barrier);
                std::thread::spawn(move || {
                    barrier.wait();
                    repository.load_artifact()
                })
            })
            .collect::<Vec<_>>();

        barrier.wait();
        let loaded = workers
            .into_iter()
            .map(|worker| worker.join().unwrap().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(loaded[0], loaded[1]);
        assert_eq!(loaded[0].schema_version, REVIEW_SCHEMA_VERSION);
        assert_eq!(loaded[0].revision, 0);
        assert!(loaded[0].annotations.is_empty());

        let disk: ReviewArtifact =
            serde_json::from_slice(&fs::read(repository.artifact_path()).unwrap()).unwrap();
        assert_eq!(disk, loaded[0]);
    }

    #[test]
    fn migrates_v1_snapshot_with_new_field_defaults_and_rewrites_v2() {
        let (_deck, _state, repository) = fixture();
        repository.ensure_layout().unwrap();
        let legacy = r#"{
            "schema_version":1,
            "revision":7,
            "annotations":[{
                "id":"r-old-4",
                "slide_id":"s-01-title",
                "source_path":"slides/01-title.html",
                "target":{"type":"point","x":1.0,"y":2.0},
                "body":"legacy note",
                "kind":"note",
                "action":null,
                "state":"todo",
                "created_at_ms":10,
                "updated_at_ms":11
            }]
        }"#;
        fs::write(repository.artifact_path(), legacy).unwrap();

        let migrated = repository.load_artifact().unwrap();
        assert_eq!(migrated.schema_version, 2);
        assert_eq!(migrated.revision, 7);
        assert_eq!(migrated.next_annotation_id, 5);
        assert_eq!(
            migrated.annotations[0].disposition,
            ReviewDisposition::Pending
        );
        assert_eq!(migrated.annotations[0].freshness, ReviewFreshness::Current);
        let disk: ReviewArtifact =
            serde_json::from_slice(&fs::read(repository.artifact_path()).unwrap()).unwrap();
        assert_eq!(disk, migrated);
    }

    #[test]
    fn version_two_optional_defaults_round_trip() {
        let (_deck, _state, repository) = fixture();
        let identity = &repository.deck;
        let raw = format!(
            r#"{{"schema_version":2,"revision":0,"deck":{{"canonical_root":{},"root_key":{}}},"annotations":[],"created_at_ms":1,"updated_at_ms":1}}"#,
            serde_json::to_string(&identity.canonical_root).unwrap(),
            serde_json::to_string(&identity.root_key).unwrap()
        );
        repository.ensure_layout().unwrap();
        fs::write(repository.artifact_path(), raw).unwrap();
        let loaded = repository.load_artifact().unwrap();
        assert_eq!(loaded.next_annotation_id, 1);
        assert!(loaded.build.is_none());
        let disk: serde_json::Value =
            serde_json::from_slice(&fs::read(repository.artifact_path()).unwrap()).unwrap();
        assert_eq!(disk["next_annotation_id"], 1);
    }

    #[test]
    fn missing_v2_id_sequence_is_inferred_without_duplicate_ids() {
        let (_deck, _state, repository) = fixture();
        let created = repository.apply_mutation(create(0)).unwrap();
        let mut value = serde_json::to_value(&created).unwrap();
        value.as_object_mut().unwrap().remove("next_annotation_id");
        fs::write(
            repository.artifact_path(),
            serde_json::to_vec_pretty(&value).unwrap(),
        )
        .unwrap();

        let loaded = repository.load_artifact().unwrap();
        assert_eq!(loaded.next_annotation_id, 2);
        let disk: ReviewArtifact =
            serde_json::from_slice(&fs::read(repository.artifact_path()).unwrap()).unwrap();
        assert_eq!(disk, loaded);
        let second = repository.apply_mutation(create(loaded.revision)).unwrap();
        assert_eq!(second.annotations.len(), 2);
        assert_ne!(second.annotations[0].id, second.annotations[1].id);
    }

    #[test]
    fn rejects_malformed_unknown_version_and_oversized_artifacts() {
        let (_deck, _state, repository) = fixture();
        repository.ensure_layout().unwrap();
        fs::write(repository.artifact_path(), b"{not json").unwrap();
        assert!(matches!(
            repository.load_artifact(),
            Err(ReviewRepositoryError::Malformed(_))
        ));

        fs::write(repository.artifact_path(), br#"{"schema_version":99}"#).unwrap();
        assert!(matches!(
            repository.load_artifact(),
            Err(ReviewRepositoryError::Malformed(_))
        ));

        let oversized = vec![b'x'; MAX_REVIEW_ARTIFACT_BYTES + 1];
        fs::write(repository.artifact_path(), oversized).unwrap();
        assert!(matches!(
            repository.load_artifact(),
            Err(ReviewRepositoryError::Oversized { .. })
        ));
    }

    #[test]
    fn rejects_oversized_body_hints_counts_and_bad_paths() {
        let mut oversized_body = create(0);
        let ReviewMutation::Create { annotation, .. } = &mut oversized_body else {
            unreachable!()
        };
        annotation.body = "x".repeat(MAX_REVIEW_BODY_BYTES + 1);
        assert!(matches!(
            ReviewStore::new("s".into()).apply(oversized_body),
            Err(ReviewMutationError::Invalid(_))
        ));

        let mut oversized_hint = create(0);
        let ReviewMutation::Create { annotation, .. } = &mut oversized_hint else {
            unreachable!()
        };
        annotation.target = ReviewTarget::Point {
            x: 1.0,
            y: 2.0,
            selector_hint: Some("x".repeat(MAX_REVIEW_HINT_BYTES + 1)),
            text_hint: None,
        };
        assert!(matches!(
            ReviewStore::new("s".into()).apply(oversized_hint),
            Err(ReviewMutationError::Invalid(_))
        ));

        let mut traversal = create(0);
        let ReviewMutation::Create { annotation, .. } = &mut traversal else {
            unreachable!()
        };
        annotation.source_path = "../deck.toml".into();
        assert!(matches!(
            ReviewStore::new("s".into()).apply(traversal),
            Err(ReviewMutationError::Invalid(_))
        ));

        let mut annotations = Vec::new();
        let mut next = 1;
        for _ in 0..MAX_REVIEW_ANNOTATIONS {
            apply_mutation_to_annotations(&mut annotations, create(0), "bounded", &mut next, None)
                .unwrap();
        }
        assert!(matches!(
            apply_mutation_to_annotations(&mut annotations, create(0), "bounded", &mut next, None),
            Err(ReviewMutationError::Invalid(_))
        ));
    }

    #[test]
    fn mutation_json_rejects_unknown_fields_and_out_of_bounds_target() {
        let raw = r#"{
            "operation":"create","revision":0,
            "annotation":{"slide_id":"s-01","source_path":"slides/01.html",
            "target":{"type":"point","x":1,"y":2,"z":3},"body":"note",
            "kind":"note","action":null}}
        "#;
        assert!(serde_json::from_str::<ReviewMutation>(raw).is_err());

        let mut outside = create(0);
        let ReviewMutation::Create { annotation, .. } = &mut outside else {
            unreachable!()
        };
        annotation.target = ReviewTarget::Point {
            x: 1921.0,
            y: 1.0,
            selector_hint: None,
            text_hint: None,
        };
        assert!(matches!(
            ReviewStore::new("s".into()).apply(outside),
            Err(ReviewMutationError::Invalid(_))
        ));
    }

    #[test]
    fn repository_conflicts_allow_only_one_same_revision_writer() {
        let (_deck, _state, repository) = fixture();
        let repository = Arc::new(repository);
        let barrier = Arc::new(Barrier::new(3));
        let workers = (0..2)
            .map(|_| {
                let repository = Arc::clone(&repository);
                let barrier = Arc::clone(&barrier);
                std::thread::spawn(move || {
                    barrier.wait();
                    repository.apply_mutation(create(0))
                })
            })
            .collect::<Vec<_>>();
        barrier.wait();
        let results = workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        assert_eq!(
            results
                .iter()
                .filter(|result| matches!(result, Err(ReviewRepositoryError::Conflict(_))))
                .count(),
            1
        );
        assert_eq!(repository.load_artifact().unwrap().annotations.len(), 1);
    }

    #[test]
    fn create_requires_an_exact_manifest_slide_pair() {
        let (_deck, _state, repository) = fixture();
        let build = repository
            .update_build_manifest(0, manifest("aaa"))
            .unwrap();

        let mut wrong_source = new_annotation();
        wrong_source.source_path = "slides/renamed.html".into();
        assert!(matches!(
            repository.apply_mutation(ReviewMutation::Create {
                revision: build.revision,
                annotation: wrong_source,
            }),
            Err(ReviewRepositoryError::Invalid(_))
        ));

        let mut wrong_slide = new_annotation();
        wrong_slide.slide_id = "s-missing".into();
        assert!(matches!(
            repository.apply_mutation(ReviewMutation::Create {
                revision: build.revision,
                annotation: wrong_slide,
            }),
            Err(ReviewRepositoryError::Invalid(_))
        ));
        assert_eq!(repository.load_artifact().unwrap(), build);
    }

    #[test]
    fn exhausted_revision_is_rejected_without_modifying_artifact() {
        let (_deck, _state, repository) = fixture();
        let mut artifact = repository.load_artifact().unwrap();
        artifact.revision = u64::MAX;
        repository.write_unlocked(&artifact).unwrap();

        assert!(matches!(
            repository.clear(u64::MAX),
            Err(ReviewRepositoryError::Invalid(_))
        ));
        assert_eq!(repository.load_artifact().unwrap(), artifact);

        let mut store = ReviewStore::new("full".into());
        store.revision = u64::MAX;
        assert!(matches!(
            store.apply(create(u64::MAX)),
            Err(ReviewMutationError::Invalid(_))
        ));
        assert!(store.annotations.is_empty());
    }

    #[test]
    fn separate_repository_instances_do_not_lose_updates() {
        let (deck, state, first) = fixture();
        let second = ReviewRepository::with_state_root(deck.path(), state.path()).unwrap();
        let created = first.apply_mutation(create(0)).unwrap();
        let resolved = second
            .resolve(created.revision, &created.annotations[0].id, true)
            .unwrap();
        assert_eq!(resolved.revision, 2);
        assert_eq!(resolved.annotations[0].state, ReviewState::Resolved);
        assert_eq!(first.load_artifact().unwrap(), resolved);
    }

    #[test]
    fn rebuild_derives_freshness_and_preserves_orphans_and_workflow() {
        let (_deck, _state, repository) = fixture();
        let build = repository
            .update_build_manifest(0, manifest("aaa"))
            .unwrap();
        let created = repository.apply_mutation(create(build.revision)).unwrap();
        let id = created.annotations[0].id.clone();
        let resolved = repository.resolve(created.revision, &id, true).unwrap();
        let dispositioned = repository
            .set_disposition(
                resolved.revision,
                &id,
                ReviewDisposition::Addressed,
                Some("Updated source".into()),
            )
            .unwrap();
        let workflow_updated_at = dispositioned.annotations[0].updated_at_ms;

        let stale = repository
            .update_build_manifest(dispositioned.revision, manifest("bbb"))
            .unwrap();
        assert_eq!(stale.annotations[0].freshness, ReviewFreshness::Stale);
        assert_eq!(stale.annotations[0].state, ReviewState::Resolved);
        assert_eq!(
            stale.annotations[0].disposition,
            ReviewDisposition::Addressed
        );
        assert_eq!(stale.annotations[0].updated_at_ms, workflow_updated_at);

        let mut removed = manifest("ccc");
        removed.slides.clear();
        let orphaned = repository
            .update_build_manifest(stale.revision, removed)
            .unwrap();
        assert_eq!(orphaned.annotations.len(), 1);
        assert_eq!(orphaned.annotations[0].freshness, ReviewFreshness::Orphaned);
        assert_eq!(orphaned.annotations[0].state, ReviewState::Resolved);
        assert_eq!(orphaned.annotations[0].updated_at_ms, workflow_updated_at);
    }

    #[test]
    fn persisted_moved_source_remains_stale_but_new_creation_is_rejected() {
        let (_deck, _state, repository) = fixture();
        let created = repository.apply_mutation(create(0)).unwrap();
        let mut moved = manifest("aaa");
        moved.slides[0].source_path = "slides/renamed.html".into();
        let build = repository
            .update_build_manifest(created.revision, moved)
            .unwrap();
        assert_eq!(build.annotations[0].freshness, ReviewFreshness::Stale);
        let loaded = repository.load_artifact().unwrap();
        assert_eq!(loaded, build);

        assert!(matches!(
            repository.apply_mutation(create(build.revision)),
            Err(ReviewRepositoryError::Invalid(_))
        ));
        assert_eq!(repository.load_artifact().unwrap(), build);
    }

    #[test]
    fn manifest_updates_are_monotonic_and_identical_content_is_a_no_op() {
        let (_deck, _state, repository) = fixture();
        let current = repository
            .update_build_manifest(0, manifest_at("aaa", 200))
            .unwrap();

        let no_op = repository
            .update_build_manifest(current.revision, manifest_at("aaa", 999))
            .unwrap();
        assert_eq!(no_op.revision, current.revision);
        assert_eq!(no_op.updated_at_ms, current.updated_at_ms);
        assert_eq!(no_op.build.as_ref().unwrap().built_at_ms, 999);

        assert!(matches!(
            repository.update_build_manifest(current.revision, manifest_at("bbb", 998)),
            Err(ReviewRepositoryError::Invalid(_))
        ));
        assert_eq!(repository.load_artifact().unwrap(), no_op);

        let same_time_new_build = repository
            .update_build_manifest(current.revision, manifest_at("bbb", 999))
            .unwrap();
        assert_eq!(same_time_new_build.revision, current.revision + 1);
        assert_eq!(same_time_new_build.build.unwrap().build_id, "build-bbb");
    }

    #[test]
    fn same_manifest_racers_are_both_no_ops() {
        let (_deck, _state, repository) = fixture();
        let current = repository
            .update_build_manifest(0, manifest_at("aaa", 200))
            .unwrap();
        let repository = Arc::new(repository);
        let barrier = Arc::new(Barrier::new(3));
        let workers = [300, 400]
            .into_iter()
            .map(|built_at_ms| {
                let repository = Arc::clone(&repository);
                let barrier = Arc::clone(&barrier);
                let revision = current.revision;
                std::thread::spawn(move || {
                    barrier.wait();
                    repository.update_build_manifest(revision, manifest_at("aaa", built_at_ms))
                })
            })
            .collect::<Vec<_>>();
        barrier.wait();
        for worker in workers {
            let artifact = worker.join().unwrap().unwrap();
            assert_eq!(artifact.revision, current.revision);
            assert_eq!(artifact.build.as_ref().unwrap().build_id, "build-aaa");
        }
        let final_artifact = repository.load_artifact().unwrap();
        assert_eq!(final_artifact.revision, current.revision);
        assert_eq!(final_artifact.updated_at_ms, current.updated_at_ms);
        assert_eq!(final_artifact.build.as_ref().unwrap().built_at_ms, 400);
    }

    #[test]
    fn unresolved_annotation_survives_repository_reopen_and_rebuild() {
        let (deck, state, repository) = fixture();
        let build = repository
            .update_build_manifest(0, manifest("aaa"))
            .unwrap();
        let created = repository.apply_mutation(create(build.revision)).unwrap();
        drop(repository);
        let reopened = ReviewRepository::with_state_root(deck.path(), state.path()).unwrap();
        let rebuilt = reopened
            .update_build_manifest(created.revision, manifest("aaa"))
            .unwrap();
        assert_eq!(rebuilt.annotations.len(), 1);
        assert_eq!(rebuilt.annotations[0].state, ReviewState::Todo);
        assert_eq!(rebuilt.annotations[0].freshness, ReviewFreshness::Current);
    }

    #[test]
    fn clear_increments_revision_and_keeps_durable_artifact() {
        let (_deck, _state, repository) = fixture();
        let created = repository.apply_mutation(create(0)).unwrap();
        let cleared = repository.clear(created.revision).unwrap();
        assert_eq!(cleared.revision, created.revision + 1);
        assert!(cleared.annotations.is_empty());
        assert!(cleared.cleared_at_ms.is_some());
        assert!(repository.artifact_path().is_file());
        assert_eq!(repository.load_artifact().unwrap(), cleared);
    }

    #[test]
    fn disposition_is_orthogonal_and_note_is_bounded() {
        let (_deck, _state, repository) = fixture();
        let created = repository.apply_mutation(create(0)).unwrap();
        let id = created.annotations[0].id.clone();
        let dispositioned = repository
            .set_disposition(
                created.revision,
                &id,
                ReviewDisposition::Deferred,
                Some("Follow up next cycle".into()),
            )
            .unwrap();
        assert_eq!(dispositioned.annotations[0].state, ReviewState::Todo);
        assert_eq!(
            dispositioned.annotations[0].disposition,
            ReviewDisposition::Deferred
        );
        assert!(matches!(
            repository.set_disposition(
                dispositioned.revision,
                id,
                ReviewDisposition::WontFix,
                Some("x".repeat(MAX_REVIEW_DISPOSITION_NOTE_BYTES + 1))
            ),
            Err(ReviewRepositoryError::Invalid(_))
        ));
    }

    #[test]
    fn markdown_handoff_contains_agent_context_and_verification() {
        let (_deck, _state, repository) = fixture();
        let build = repository
            .update_build_manifest(0, manifest("aaa"))
            .unwrap();
        repository.apply_mutation(create(build.revision)).unwrap();
        let markdown = repository.handoff_markdown().unwrap();
        assert!(markdown.contains("slides/01-title.html"));
        assert!(markdown.contains("Tighten this title"));
        assert!(markdown.contains("Freshness: `Current`"));
        assert!(markdown.contains("sideshow check ."));
        assert!(markdown.contains(repository.artifact_path().to_str().unwrap()));
        assert!(markdown.contains("Selector hint: ` h1.title `"));
        assert!(markdown.contains("Text hint: ` Title `"));
    }

    #[test]
    fn no_manifest_markdown_shell_quotes_the_canonical_deck_root() {
        let root = TempDir::new().unwrap();
        let deck_path = root.path().join("deck with ' quote");
        fs::create_dir(&deck_path).unwrap();
        let state_path = root.path().join("state");
        fs::create_dir(&state_path).unwrap();
        let repository = ReviewRepository::with_state_root(&deck_path, &state_path).unwrap();
        let markdown = repository.handoff_markdown().unwrap();
        let canonical_root = deck_path.canonicalize().unwrap();
        let quoted = shell_quote(canonical_root.to_str().unwrap());
        assert!(markdown.contains(&format!("    sideshow check {quoted}\n")));
        assert!(markdown.contains(&format!("    sideshow build {quoted}\n")));
        assert!(!markdown.contains("sideshow check ."));
    }

    #[test]
    fn atomic_writer_leaves_no_temporary_files() {
        let (_deck, _state, repository) = fixture();
        repository.apply_mutation(create(0)).unwrap();
        let entries = fs::read_dir(&repository.reviews_dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert!(entries.iter().any(|name| name.ends_with(".json")));
        assert!(entries.iter().any(|name| name.ends_with(".lock")));
        assert!(!entries.iter().any(|name| name.ends_with(".tmp")));
        assert_eq!(entries.len(), 2);
    }

    #[cfg(unix)]
    #[test]
    fn atomic_writer_enforces_private_permissions_on_create_and_replace() {
        use std::os::unix::fs::PermissionsExt;

        let (_deck, _state, repository) = fixture();
        let empty = repository.load_artifact().unwrap();
        assert_eq!(
            fs::metadata(repository.artifact_path())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );

        fs::set_permissions(
            repository.artifact_path(),
            fs::Permissions::from_mode(0o644),
        )
        .unwrap();
        repository.apply_mutation(create(empty.revision)).unwrap();
        assert_eq!(
            fs::metadata(repository.artifact_path())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }

    #[test]
    fn xdg_empty_uses_home_but_relative_nonempty_is_rejected() {
        let home = Path::new("/tmp/home").as_os_str();
        assert_eq!(
            xdg_state_home_from(Some(std::ffi::OsStr::new("")), Some(home)).unwrap(),
            PathBuf::from("/tmp/home/.local/state")
        );
        assert!(matches!(
            xdg_state_home_from(Some(std::ffi::OsStr::new("relative/state")), Some(home)),
            Err(ReviewRepositoryError::Invalid(_))
        ));
    }

    #[test]
    fn concurrent_in_memory_same_revision_mutations_have_one_winner() {
        let store = Arc::new(Mutex::new(ReviewStore::new("session".into())));
        let barrier = Arc::new(Barrier::new(3));
        let workers = (0..2)
            .map(|_| {
                let store = Arc::clone(&store);
                let barrier = Arc::clone(&barrier);
                std::thread::spawn(move || {
                    barrier.wait();
                    store.lock().unwrap().apply(create(0))
                })
            })
            .collect::<Vec<_>>();
        barrier.wait();
        let results = workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        assert_eq!(
            results
                .iter()
                .filter(|result| matches!(result, Err(ReviewMutationError::Stale(_))))
                .count(),
            1
        );
    }
}
