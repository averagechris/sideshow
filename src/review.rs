use serde::{Deserialize, Serialize};
use std::{
    path::{Component, Path},
    time::{SystemTime, UNIX_EPOCH},
};

pub const REVIEW_SCHEMA_VERSION: u32 = 1;
pub const REVIEW_CANVAS_WIDTH: f64 = 1920.0;
pub const REVIEW_CANVAS_HEIGHT: f64 = 1080.0;
pub const MAX_REVIEW_BODY_BYTES: usize = 8 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ReviewSnapshot {
    pub schema_version: u32,
    pub revision: u64,
    pub annotations: Vec<ReviewAnnotation>,
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
    pub state: ReviewState,
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

        match mutation {
            ReviewMutation::Create { annotation, .. } => {
                validate_new_annotation(&annotation)?;
                let now = unix_time_ms();
                let id = format!("r-{}-{}", self.id_prefix, self.next_id);
                self.next_id += 1;
                self.annotations.push(ReviewAnnotation {
                    id,
                    slide_id: annotation.slide_id,
                    source_path: annotation.source_path,
                    target: annotation.target,
                    body: annotation.body,
                    kind: annotation.kind,
                    action: annotation.action,
                    state: ReviewState::Todo,
                    created_at_ms: now,
                    updated_at_ms: now,
                });
            }
            ReviewMutation::Edit {
                id,
                body,
                kind,
                action,
                ..
            } => {
                validate_body(&body)?;
                let annotation = self
                    .annotations
                    .iter_mut()
                    .find(|annotation| annotation.id == id)
                    .ok_or(ReviewMutationError::NotFound)?;
                annotation.body = body;
                annotation.kind = kind;
                annotation.action = action;
                annotation.updated_at_ms = unix_time_ms();
            }
            ReviewMutation::SetState { id, state, .. } => {
                let annotation = self
                    .annotations
                    .iter_mut()
                    .find(|annotation| annotation.id == id)
                    .ok_or(ReviewMutationError::NotFound)?;
                annotation.state = state;
                annotation.updated_at_ms = unix_time_ms();
            }
            ReviewMutation::Delete { id, .. } => {
                let old_len = self.annotations.len();
                self.annotations.retain(|annotation| annotation.id != id);
                if self.annotations.len() == old_len {
                    return Err(ReviewMutationError::NotFound);
                }
            }
        }

        self.revision += 1;
        Ok(self.snapshot())
    }
}

fn validate_new_annotation(annotation: &NewReviewAnnotation) -> Result<(), ReviewMutationError> {
    validate_bounded_text("slide id", &annotation.slide_id, 256, false)?;
    validate_bounded_text("source path", &annotation.source_path, 1024, false)?;
    let source_path = Path::new(&annotation.source_path);
    if annotation.source_path.contains('\\')
        || annotation.source_path.starts_with('/')
        || annotation.source_path.ends_with('/')
        || annotation.source_path.split('/').any(str::is_empty)
        || source_path.is_absolute()
        || source_path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(ReviewMutationError::Invalid(
            "source path must be a normalized relative path".into(),
        ));
    }
    validate_body(&annotation.body)?;
    validate_target(&annotation.target)
}

fn validate_body(body: &str) -> Result<(), ReviewMutationError> {
    validate_bounded_text("annotation body", body, MAX_REVIEW_BODY_BYTES, false)
}

fn validate_bounded_text(
    name: &str,
    text: &str,
    max_bytes: usize,
    allow_empty: bool,
) -> Result<(), ReviewMutationError> {
    if (!allow_empty && text.trim().is_empty()) || text.len() > max_bytes {
        return Err(ReviewMutationError::Invalid(format!(
            "{name} must contain 1..={max_bytes} bytes"
        )));
    }
    if text
        .chars()
        .any(|ch| ch.is_control() && ch != '\n' && ch != '\t')
    {
        return Err(ReviewMutationError::Invalid(format!(
            "{name} contains unsupported control characters"
        )));
    }
    Ok(())
}

fn validate_target(target: &ReviewTarget) -> Result<(), ReviewMutationError> {
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
        return Err(ReviewMutationError::Invalid(format!(
            "target must fit within the {}x{} logical canvas",
            REVIEW_CANVAS_WIDTH as u32, REVIEW_CANVAS_HEIGHT as u32
        )));
    }
    if matches!(target, ReviewTarget::Region { .. }) && (width < 1.0 || height < 1.0) {
        return Err(ReviewMutationError::Invalid(
            "region targets must have positive width and height".into(),
        ));
    }
    if let Some(hint) = selector_hint {
        validate_bounded_text("selector hint", hint, 512, true)?;
    }
    if let Some(hint) = text_hint {
        validate_bounded_text("text hint", hint, 512, true)?;
    }
    Ok(())
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

    fn point() -> ReviewTarget {
        ReviewTarget::Point {
            x: 100.0,
            y: 200.0,
            selector_hint: Some("h1.title".into()),
            text_hint: Some("Title".into()),
        }
    }

    fn create(revision: u64) -> ReviewMutation {
        ReviewMutation::Create {
            revision,
            annotation: NewReviewAnnotation {
                slide_id: "s-01-title".into(),
                source_path: "slides/01-title.html".into(),
                target: point(),
                body: "Tighten this title".into(),
                kind: ReviewKind::Issue,
                action: Some(ReviewAction::Fix),
            },
        }
    }

    #[test]
    fn mutations_increment_revisions_and_preserve_identity() {
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
    }

    #[test]
    fn stale_mutation_returns_latest_snapshot_without_overwriting() {
        let mut store = ReviewStore::new("session".into());
        store.apply(create(0)).unwrap();

        let err = store.apply(create(0)).unwrap_err();
        let ReviewMutationError::Stale(snapshot) = err else {
            panic!("expected stale revision");
        };
        assert_eq!(snapshot.revision, 1);
        assert_eq!(snapshot.annotations.len(), 1);
        assert_eq!(store.snapshot(), snapshot);
    }

    #[test]
    fn rejects_empty_bodies_and_out_of_bounds_targets() {
        let mut empty = create(0);
        if let ReviewMutation::Create { annotation, .. } = &mut empty {
            annotation.body = "  ".into();
        }
        assert!(matches!(
            ReviewStore::new("s".into()).apply(empty),
            Err(ReviewMutationError::Invalid(_))
        ));

        let mut outside = create(0);
        if let ReviewMutation::Create { annotation, .. } = &mut outside {
            annotation.target = ReviewTarget::Point {
                x: 1921.0,
                y: 1.0,
                selector_hint: None,
                text_hint: None,
            };
        }
        assert!(matches!(
            ReviewStore::new("s".into()).apply(outside),
            Err(ReviewMutationError::Invalid(_))
        ));
    }

    #[test]
    fn mutation_json_rejects_unknown_fields() {
        let raw = r#"{
            "operation":"create",
            "revision":0,
            "annotation":{
                "slide_id":"s-01",
                "source_path":"slides/01.html",
                "target":{"type":"point","x":1,"y":2,"z":3},
                "body":"note",
                "kind":"note",
                "action":null
            }
        }"#;

        assert!(serde_json::from_str::<ReviewMutation>(raw).is_err());
    }

    #[test]
    fn concurrent_same_revision_mutations_have_one_winner() {
        use std::sync::{Arc, Barrier, Mutex};

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
        assert_eq!(store.lock().unwrap().snapshot().annotations.len(), 1);
    }
}
