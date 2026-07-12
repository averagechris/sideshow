use anyhow::Context;
use clap::{Parser, Subcommand, ValueEnum};
use flate2::{Compression, write::GzEncoder};
use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use sha2::{Digest, Sha256};
use sideshow::find_tool;
use std::process::Command as ProcessCommand;
use std::{
    collections::BTreeMap,
    env, fs,
    io::{Cursor, Read, Write},
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, mpsc},
    thread,
    time::Duration,
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Debug, Parser)]
#[command(name = "sideshow", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Scaffold a new slide deck source directory.
    New {
        /// Directory to create.
        dir: PathBuf,
        /// Built-in theme to copy into the deck.
        #[arg(long, default_value = "signal")]
        theme: String,
    },
    /// Build a deck into dist/<slug-of-title>.html.
    Build { dir: PathBuf },
    /// Statically lint a deck source directory.
    Check {
        dir: PathBuf,
        #[arg(long)]
        format: Option<String>,
        #[arg(long)]
        strict: bool,
    },
    /// Inspect, resize, crop, and optimize images.
    Img {
        #[command(subcommand)]
        command: ImgCommand,
    },
    /// Inspect and optimize videos.
    Video {
        #[command(subcommand)]
        command: VideoCommand,
    },
    /// Render deck-local terminal demo tapes.
    Tape {
        #[command(subcommand)]
        command: TapeCommand,
    },
    /// List built-in themes and selection metadata.
    Themes {
        #[arg(long)]
        format: Option<String>,
    },
    /// Discover bundled registries and resources as stable JSON.
    Registry {
        #[command(subcommand)]
        command: RegistryCommand,
    },
    /// Build and serve dist/ over localhost.
    Serve {
        dir: PathBuf,
        #[arg(long, default_value_t = 8000)]
        port: u16,
        /// Open the served deck in the system browser after build and bind succeed.
        #[arg(long)]
        open: bool,
        /// Enable local, annotation-only slide review controls.
        #[arg(long)]
        review: bool,
    },
    /// Inspect and manage persistent deck review artifacts.
    Review {
        #[command(subcommand)]
        command: ReviewCommand,
    },
    /// Scaffold, validate, and export canonical plan decks.
    Plan {
        #[command(subcommand)]
        command: PlanCommand,
    },
    /// Publish an existing dist output to S3 or SourceHut Pages.
    Publish {
        dir: PathBuf,
        /// Publish target.
        #[arg(long, value_enum)]
        target: PublishTarget,
        /// S3 bucket name (required with --target s3).
        #[arg(long)]
        bucket: Option<String>,
        /// S3 object key (defaults to the dist file name).
        #[arg(long)]
        key: Option<String>,
        /// S3 presign expiration in seconds (1..=604800).
        #[arg(long, default_value_t = 604800)]
        expires: u32,
        /// SourceHut Pages domain (required with --target srht).
        #[arg(long)]
        domain: Option<String>,
        /// SourceHut Pages subdirectory (defaults to the deck slug).
        #[arg(long)]
        subdir: Option<String>,
        /// SourceHut Pages API base URL.
        #[arg(long, default_value = "https://pages.sr.ht")]
        pages_url: String,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum PublishTarget {
    S3,
    Srht,
}

#[derive(Debug, Subcommand)]
enum ReviewCommand {
    /// Print the artifact locator, deck identity, and current revision as JSON.
    Artifact { deck: PathBuf },
    /// Print the complete current review artifact as JSON.
    List { deck: PathBuf },
    /// Export a machine-readable or prompt-oriented review handoff.
    Export {
        deck: PathBuf,
        #[arg(long, value_enum, default_value_t = ReviewExportFormat::Json)]
        format: ReviewExportFormat,
        /// Write to this path instead of stdout. Paths inside the deck are rejected.
        #[arg(long)]
        output: Option<PathBuf>,
    },
    /// Explicitly clear every annotation at the given revision.
    Clear {
        deck: PathBuf,
        #[arg(long)]
        revision: u64,
        /// Confirm destructive clearing.
        #[arg(long)]
        yes: bool,
    },
    /// Mark one annotation resolved.
    Resolve {
        deck: PathBuf,
        id: String,
        #[arg(long)]
        revision: u64,
    },
    /// Reopen one resolved annotation.
    Reopen {
        deck: PathBuf,
        id: String,
        #[arg(long)]
        revision: u64,
    },
    /// Record an explicit disposition independently of workflow resolution.
    Disposition {
        deck: PathBuf,
        id: String,
        #[arg(long, value_enum)]
        status: ReviewDispositionArg,
        /// Optional rationale; omit it to clear a previous disposition note.
        #[arg(long)]
        note: Option<String>,
        #[arg(long)]
        revision: u64,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum ReviewExportFormat {
    Json,
    Markdown,
}

#[derive(Debug, Subcommand)]
enum PlanCommand {
    /// Scaffold a plan deck with canonical plan.json and visual slides.
    New {
        dir: PathBuf,
        #[arg(long, default_value = "signal")]
        theme: String,
    },
    /// Run normal deck checks plus plan schema/reference checks.
    Check {
        dir: PathBuf,
        #[arg(long, value_enum, default_value_t = PlanCheckFormat::Text)]
        format: PlanCheckFormat,
        #[arg(long)]
        strict: bool,
    },
    /// Export deterministic plan data; Markdown includes manual issue-drafting packets.
    Export {
        dir: PathBuf,
        #[arg(long, value_enum, default_value_t = PlanExportFormat::Markdown)]
        format: PlanExportFormat,
        #[arg(long)]
        output: Option<PathBuf>,
    },
    /// Serve the plan deck with local review controls.
    Serve {
        dir: PathBuf,
        #[arg(long, default_value_t = 8000)]
        port: u16,
        #[arg(long)]
        open: bool,
    },
}

#[derive(Debug, Subcommand)]
enum RegistryCommand {
    /// List activated registry entries as stable JSON.
    List,
    /// Explain one activated registry entry as stable JSON.
    Explain { kind: String, name: String },
    /// List activated registry sources as stable JSON.
    Sources,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum PlanCheckFormat {
    Text,
    Json,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum PlanExportFormat {
    Markdown,
    Json,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum ReviewDispositionArg {
    Pending,
    Addressed,
    WontFix,
    Deferred,
}

impl From<ReviewDispositionArg> for sideshow::review::ReviewDisposition {
    fn from(value: ReviewDispositionArg) -> Self {
        match value {
            ReviewDispositionArg::Pending => Self::Pending,
            ReviewDispositionArg::Addressed => Self::Addressed,
            ReviewDispositionArg::WontFix => Self::WontFix,
            ReviewDispositionArg::Deferred => Self::Deferred,
        }
    }
}

#[derive(Debug, Subcommand)]
enum ImgCommand {
    Info {
        paths: Vec<PathBuf>,
        #[arg(long)]
        format: Option<String>,
    },
    Resize {
        path: PathBuf,
        #[arg(long)]
        width: u32,
        #[arg(long)]
        height: Option<u32>,
        #[arg(long)]
        out: Option<PathBuf>,
    },
    Crop {
        path: PathBuf,
        #[arg(long)]
        rect: String,
        #[arg(long)]
        out: Option<PathBuf>,
    },
    Optimize {
        paths: Vec<PathBuf>,
        #[arg(long, default_value_t = 80.0)]
        quality: f32,
        #[arg(long, default_value_t = 3840)]
        max_dim: u32,
        #[arg(long)]
        in_place: bool,
    },
}

#[derive(Debug, Subcommand)]
enum VideoCommand {
    Optimize {
        file: PathBuf,
        #[arg(long, default_value_t = 40)]
        quality: u8,
        #[arg(long, default_value_t = 1280)]
        max_dim: u32,
        #[arg(long)]
        keep_audio: bool,
    },
}

#[derive(Debug, Subcommand)]
enum TapeCommand {
    Render {
        deck_dir: PathBuf,
        #[arg(long)]
        tape: Option<String>,
        #[arg(long)]
        force: bool,
    },
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::New { dir, theme } => {
            sideshow::new_deck(&dir, &theme)?;
            println!("created {} (theme: {theme})", dir.display());
            println!(
                "next: sideshow check {} && sideshow build {}",
                dir.display(),
                dir.display()
            );
            Ok(())
        }
        Command::Build { dir } => {
            // A normal build must not opt a deck into review state. If review state already
            // exists, however, capture a coherent manifest and refresh it after publishing the
            // output so CLI-only review workflows see the same freshness as `serve --review`.
            let canonical_dir = dir
                .canonicalize()
                .with_context(|| format!("failed to resolve deck root {}", dir.display()))?;
            // Review discovery happens after the expensive build and remains non-fatal when
            // XDG/HOME cannot identify any active artifact. A manifest is captured only if the
            // locked discovery step finds an existing artifact.
            let built = stable_build_deck(&canonical_dir, false)?;
            let repository = sideshow::review::ReviewRepository::discover_for_build(&canonical_dir)
                .map_err(review_cli_error)?;
            let built = accept_ordinary_build(built, repository.as_ref())?;
            println!("built {}", built.output.display());
            Ok(())
        }
        Command::Check {
            dir,
            format,
            strict,
        } => {
            let findings = sideshow::check_deck(&dir);
            if format.as_deref() == Some("json") {
                println!("{}", serde_json::to_string_pretty(&findings)?);
            } else if findings.is_empty() {
                println!("ok: no static findings (run the browser audit for visual verification)");
            } else {
                for f in &findings {
                    println!("{:?}: {}: {}: {}", f.severity, f.path, f.kind, f.message);
                }
            }
            let fail = findings
                .iter()
                .any(|f| f.severity == sideshow::FindingSeverity::Error || strict);
            if !fail { Ok(()) } else { std::process::exit(1) }
        }
        Command::Img { command } => img(command),
        Command::Video { command } => video(command),
        Command::Tape { command } => tape(command),
        Command::Themes { format } => {
            let themes = sideshow::theme_metadata();
            if format.as_deref() == Some("json") {
                println!("{}", serde_json::to_string_pretty(themes)?);
            } else {
                for t in themes {
                    println!("{} — {}", t.name, t.description);
                    println!(
                        "  mood: {}; formality: {}; density: {}",
                        t.mood, t.formality, t.density_fit
                    );
                    println!("  best for: {}", t.best_for);
                    println!("  avoid for: {}", t.avoid_for);
                }
            }
            Ok(())
        }
        Command::Registry { command } => registry_command(command),
        Command::Serve {
            dir,
            port,
            open,
            review,
        } => serve(&dir, port, review, open),
        Command::Review { command } => review_command(command),
        Command::Plan { command } => plan_command(command),
        Command::Publish {
            dir,
            target,
            bucket,
            key,
            expires,
            domain,
            subdir,
            pages_url,
        } => publish(
            &dir,
            PublishOptions {
                target,
                bucket: bucket.as_deref(),
                key: key.as_deref(),
                expires,
                domain: domain.as_deref(),
                subdir: subdir.as_deref(),
                pages_url: &pages_url,
            },
        ),
    }
}

fn registry_command(command: RegistryCommand) -> anyhow::Result<()> {
    match command {
        RegistryCommand::List => println!(
            "{}",
            serde_json::to_string_pretty(&sideshow::registry::registry_document()?)?
        ),
        RegistryCommand::Explain { kind, name } => println!(
            "{}",
            serde_json::to_string_pretty(&sideshow::registry::explain(&kind, &name)?)?
        ),
        RegistryCommand::Sources => println!(
            "{}",
            serde_json::to_string_pretty(&sideshow::registry::sources_document())?
        ),
    }
    Ok(())
}

fn plan_command(command: PlanCommand) -> anyhow::Result<()> {
    match command {
        PlanCommand::New { dir, theme } => {
            sideshow::plan::new_plan_deck(&dir, &theme)?;
            println!("created plan deck {} (theme: {theme})", dir.display());
            println!(
                "next: edit {}/plan.json and {}/slides/, then run sideshow plan check {} --strict",
                dir.display(),
                dir.display(),
                dir.display()
            );
            println!("review: sideshow plan serve {} --open", dir.display());
            Ok(())
        }
        PlanCommand::Check {
            dir,
            format,
            strict,
        } => {
            let findings = sideshow::plan::check(&dir);
            match format {
                PlanCheckFormat::Json => println!("{}", serde_json::to_string_pretty(&findings)?),
                PlanCheckFormat::Text if findings.is_empty() => {
                    println!("ok: plan deck checks passed")
                }
                PlanCheckFormat::Text => {
                    for f in &findings {
                        println!("{:?}: {}: {}: {}", f.severity, f.path, f.kind, f.message);
                    }
                }
            }
            let fail = findings.iter().any(|f| {
                f.severity == sideshow::FindingSeverity::Error
                    || (strict && f.severity == sideshow::FindingSeverity::Warning)
            });
            if fail { std::process::exit(1) } else { Ok(()) }
        }
        PlanCommand::Export {
            dir,
            format,
            output,
        } => {
            let plan = sideshow::plan::load(&dir)?;
            let rendered = match format {
                PlanExportFormat::Json => sideshow::plan::canonical_json(&plan)?,
                PlanExportFormat::Markdown => sideshow::plan::markdown(&plan),
            };
            if let Some(path) = output {
                write_new_file(&path, rendered.as_bytes())?;
            } else {
                print!("{rendered}");
            }
            Ok(())
        }
        PlanCommand::Serve { dir, port, open } => serve(&dir, port, true, open),
    }
}

fn write_new_file(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    if path.exists() {
        anyhow::bail!("refusing to overwrite existing output {}", path.display());
    }
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    fs::write(path, bytes).with_context(|| format!("failed to write {}", path.display()))
}

fn review_command(command: ReviewCommand) -> anyhow::Result<()> {
    use sideshow::review::ReviewRepository;

    match command {
        ReviewCommand::Artifact { deck } => {
            let repository = ReviewRepository::new(&deck).map_err(review_cli_error)?;
            let artifact = repository.load_artifact().map_err(review_cli_error)?;
            let output = serde_json::json!({
                "schema_version": artifact.schema_version,
                "canonical_root": artifact.deck.canonical_root,
                "root_key": artifact.deck.root_key,
                "artifact_path": repository.artifact_path().display().to_string(),
                "revision": artifact.revision,
                "build_id": artifact.build.as_ref().map(|build| &build.build_id),
            });
            println!("{}", serde_json::to_string_pretty(&output)?);
            Ok(())
        }
        ReviewCommand::List { deck } => {
            let repository = ReviewRepository::new(&deck).map_err(review_cli_error)?;
            let artifact = repository.load_artifact().map_err(review_cli_error)?;
            println!("{}", serde_json::to_string_pretty(&artifact)?);
            Ok(())
        }
        ReviewCommand::Export {
            deck,
            format,
            output,
        } => {
            let repository = ReviewRepository::new(&deck).map_err(review_cli_error)?;
            let handoff = match format {
                ReviewExportFormat::Json => repository.handoff_json(),
                ReviewExportFormat::Markdown => repository.handoff_markdown(),
            }
            .map_err(review_cli_error)?;
            if let Some(path) = output {
                repository
                    .write_export(&path, handoff.as_bytes())
                    .map_err(review_cli_error)?;
            } else {
                print!("{handoff}");
            }
            Ok(())
        }
        ReviewCommand::Clear {
            deck,
            revision,
            yes,
        } => {
            if !yes {
                anyhow::bail!("review clear requires --yes; no annotations were changed");
            }
            let repository = ReviewRepository::new(&deck).map_err(review_cli_error)?;
            let artifact = repository.clear(revision).map_err(review_cli_error)?;
            println!("{}", serde_json::to_string_pretty(&artifact)?);
            Ok(())
        }
        ReviewCommand::Resolve { deck, id, revision } => {
            let repository = ReviewRepository::new(&deck).map_err(review_cli_error)?;
            let artifact = repository
                .resolve(revision, id, true)
                .map_err(review_cli_error)?;
            println!("{}", serde_json::to_string_pretty(&artifact)?);
            Ok(())
        }
        ReviewCommand::Reopen { deck, id, revision } => {
            let repository = ReviewRepository::new(&deck).map_err(review_cli_error)?;
            let artifact = repository
                .resolve(revision, id, false)
                .map_err(review_cli_error)?;
            println!("{}", serde_json::to_string_pretty(&artifact)?);
            Ok(())
        }
        ReviewCommand::Disposition {
            deck,
            id,
            status,
            note,
            revision,
        } => {
            let repository = ReviewRepository::new(&deck).map_err(review_cli_error)?;
            let artifact = repository
                .set_disposition(revision, id, status.into(), note)
                .map_err(review_cli_error)?;
            println!("{}", serde_json::to_string_pretty(&artifact)?);
            Ok(())
        }
    }
}

fn review_cli_error(error: sideshow::review::ReviewRepositoryError) -> anyhow::Error {
    match error {
        sideshow::review::ReviewRepositoryError::Conflict(artifact) => anyhow::anyhow!(
            "review revision conflict: current revision is {}; reload with `sideshow review list <deck>` and retry",
            artifact.revision
        ),
        sideshow::review::ReviewRepositoryError::NotFound => {
            anyhow::anyhow!("review annotation not found; no annotations were changed")
        }
        other => anyhow::anyhow!(other),
    }
}

struct PublishOptions<'a> {
    target: PublishTarget,
    bucket: Option<&'a str>,
    key: Option<&'a str>,
    expires: u32,
    domain: Option<&'a str>,
    subdir: Option<&'a str>,
    pages_url: &'a str,
}

fn publish(dir: &Path, options: PublishOptions<'_>) -> anyhow::Result<()> {
    let dist = sideshow::deck_dist_path(dir)?;
    if !dist.is_file() {
        anyhow::bail!(
            "dist output not found: {}; run sideshow build {} first",
            dist.display(),
            dir.display()
        );
    }
    warn_if_stale(dir, &dist)?;
    match options.target {
        PublishTarget::S3 => publish_s3(&dist, options.bucket, options.key, options.expires),
        PublishTarget::Srht => {
            publish_srht(&dist, options.domain, options.subdir, options.pages_url)
        }
    }
}

fn publish_key(dist: &Path, key: Option<&str>) -> anyhow::Result<String> {
    key.map(ToOwned::to_owned)
        .or_else(|| dist.file_name().map(|s| s.to_string_lossy().into_owned()))
        .context("dist output has no file name for default S3 key")
}

fn validate_expires(expires: u32) -> anyhow::Result<()> {
    if !(1..=604800).contains(&expires) {
        anyhow::bail!("--expires must be between 1 and 604800 seconds (AWS SigV4 presign maximum)");
    }
    Ok(())
}

fn publish_s3(
    dist: &Path,
    bucket: Option<&str>,
    key: Option<&str>,
    expires: u32,
) -> anyhow::Result<()> {
    validate_expires(expires)?;
    let bucket = bucket.context("--bucket is required when --target s3")?;
    let key = publish_key(dist, key)?;
    let uri = format!("s3://{bucket}/{key}");
    let aws = find_tool(
        "aws",
        "SIDESHOW_AWS",
        "sideshow publish --target s3 needs the aws CLI to upload and presign",
        "https://docs.aws.amazon.com/cli/latest/userguide/getting-started-install.html",
    )?;
    let cp = std::process::Command::new(&aws)
        .args(["s3", "cp"])
        .arg(dist)
        .arg(&uri)
        .args(["--content-type", "text/html", "--no-progress"])
        .output()?;
    if !cp.status.success() {
        anyhow::bail!(
            "aws s3 cp failed: {}",
            String::from_utf8_lossy(&cp.stderr).trim()
        );
    }
    println!("uploaded {} -> {uri}", dist.display());
    let presign = std::process::Command::new(&aws)
        .args(["s3", "presign"])
        .arg(&uri)
        .args(["--expires-in", &expires.to_string()])
        .output()?;
    if !presign.status.success() {
        anyhow::bail!(
            "aws s3 presign failed: {}",
            String::from_utf8_lossy(&presign.stderr).trim()
        );
    }
    println!("presigned url (expires in {expires}s):");
    print!("{}", String::from_utf8_lossy(&presign.stdout));
    Ok(())
}

fn validate_srht_subdir(subdir: &str) -> anyhow::Result<()> {
    if subdir.is_empty() {
        anyhow::bail!(
            "--subdir cannot be empty; root publishing is intentionally unsupported to avoid replacing a whole site"
        );
    }
    if subdir.starts_with('/') || subdir.ends_with('/') {
        anyhow::bail!("--subdir must not start or end with '/'");
    }
    if subdir.split('/').any(|s| s == "..") {
        anyhow::bail!("--subdir must not contain '..' path segments");
    }
    if subdir.chars().any(char::is_whitespace) {
        anyhow::bail!("--subdir must not contain whitespace");
    }
    Ok(())
}

fn first_token_field(raw: &str) -> Option<String> {
    raw.split_whitespace().next().map(str::to_owned)
}

fn resolve_srht_token() -> anyhow::Result<String> {
    if let Ok(raw) = std::env::var("SRHT_TOKEN")
        && let Some(token) = first_token_field(raw.trim())
    {
        return Ok(token);
    }
    let (config, path) = sideshow::srht_config()?;
    if let Some(argv) = config.token_cmd {
        let program = argv.first().context("[srht] token-cmd must not be empty")?;
        let output = std::process::Command::new(program)
            .args(&argv[1..])
            .output()
            .with_context(|| format!("failed to run [srht] token-cmd {}", program))?;
        if !output.status.success() {
            anyhow::bail!("[srht] token-cmd {} failed", program);
        }
        if let Some(token) = first_token_field(&String::from_utf8_lossy(&output.stdout)) {
            return Ok(token);
        }
        anyhow::bail!("[srht] token-cmd {} produced no token", program);
    }
    anyhow::bail!(
        "srht publishing needs a token: set SRHT_TOKEN or [srht] token-cmd in {}; create a personal access token with scope pages.sr.ht/PAGES:RW at https://meta.sr.ht/oauth2",
        path.display()
    )
}

fn srht_site_tar_gz(index_html: &[u8]) -> anyhow::Result<Vec<u8>> {
    let gz = GzEncoder::new(Vec::new(), Compression::default());
    let mut tar = tar::Builder::new(gz);
    let mut header = tar::Header::new_gnu();
    header.set_entry_type(tar::EntryType::Regular);
    header.set_size(index_html.len() as u64);
    header.set_mode(0o644);
    header.set_uid(0);
    header.set_gid(0);
    header.set_mtime(0);
    tar.append_data(&mut header, "index.html", Cursor::new(index_html.to_vec()))?;
    let gz = tar.into_inner()?;
    Ok(gz.finish()?)
}

fn srht_multipart_body(boundary: &str, tar_gz: &[u8]) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"content\"; filename=\"site.tar.gz\"\r\nContent-Type: application/gzip\r\n\r\n").as_bytes());
    body.extend_from_slice(tar_gz);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    body
}

fn publish_srht(
    dist: &Path,
    domain: Option<&str>,
    subdir: Option<&str>,
    pages_url: &str,
) -> anyhow::Result<()> {
    let domain = domain.context("--domain is required when --target srht")?;
    let default_subdir = dist
        .file_stem()
        .and_then(|s| s.to_str())
        .context("dist output has no utf-8 file stem for default --subdir")?;
    let subdir = subdir.unwrap_or(default_subdir);
    validate_srht_subdir(subdir)?;
    let token = resolve_srht_token()?;
    let tar_gz = srht_site_tar_gz(&fs::read(dist)?)?;
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let boundary = format!("sideshow-srht-{nonce}-{}", std::process::id());
    let body = srht_multipart_body(&boundary, &tar_gz);
    let pages_url = pages_url.trim_end_matches('/');
    let url = format!("{pages_url}/publish/{domain}/{subdir}");
    let response = ureq::post(&url)
        .set("Authorization", &format!("Bearer {token}"))
        .set(
            "Content-Type",
            &format!("multipart/form-data; boundary={boundary}"),
        )
        .send_bytes(&body);
    match response {
        Ok(resp) => {
            let version = resp.into_string()?.trim().to_owned();
            println!(
                "published {} -> https://{domain}/{subdir}/ (site version {version})",
                dist.display()
            );
            println!("note: only /{subdir}/ was updated on {domain}");
            Ok(())
        }
        Err(ureq::Error::Status(code, resp)) => {
            let text = resp.into_string().unwrap_or_default();
            if code == 401 || code == 403 {
                anyhow::bail!(
                    "srht pages api returned {code}: {text}; check that the token has scope pages.sr.ht/PAGES:RW"
                );
            }
            anyhow::bail!("srht pages api returned {code}: {text}");
        }
        Err(err) => anyhow::bail!("srht pages api request failed: {err}"),
    }
}

fn warn_if_stale(dir: &Path, dist: &Path) -> anyhow::Result<()> {
    let dist_mtime = fs::metadata(dist)?.modified()?;
    if deck_sources_mtime(dir)? > dist_mtime {
        println!(
            "warning: dist output is older than deck sources; run sideshow build {} to refresh",
            dir.display()
        );
    }
    Ok(())
}

fn tape(command: TapeCommand) -> anyhow::Result<()> {
    match command {
        TapeCommand::Render {
            deck_dir,
            tape,
            force,
        } => render_tapes(&deck_dir, tape.as_deref(), force),
    }
}

fn render_tapes(deck_dir: &Path, tape: Option<&str>, force: bool) -> anyhow::Result<()> {
    let vhs = find_tool(
        "vhs",
        "SIDESHOW_VHS",
        "sideshow tape render needs vhs to render terminal demos",
        "https://github.com/charmbracelet/vhs",
    )?;
    let tapes_dir = deck_dir.join("tapes");
    let tapes = if let Some(name) = tape {
        let file = if name.ends_with(".tape") {
            tapes_dir.join(name)
        } else {
            tapes_dir.join(format!("{name}.tape"))
        };
        if !file.is_file() {
            anyhow::bail!("tape not found: {}", file.display());
        }
        vec![file]
    } else if tapes_dir.is_dir() {
        let mut tapes = fs::read_dir(&tapes_dir)?
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("tape"))
            .collect::<Vec<_>>();
        tapes.sort();
        tapes
    } else {
        Vec::new()
    };
    if tapes.is_empty() {
        println!("no tapes found in {}", tapes_dir.display());
        return Ok(());
    }
    fs::create_dir_all(deck_dir.join("assets"))?;
    for tape_path in tapes {
        let stem = tape_path
            .file_stem()
            .and_then(|s| s.to_str())
            .context("tape file name is not valid utf-8")?;
        let out = deck_dir.join("assets").join(format!("{stem}.webm"));
        let rel_tape = format!(
            "tapes/{}.tape",
            tape_path.file_stem().unwrap().to_string_lossy()
        );
        let rel_out = format!("assets/{stem}.webm");
        // Mtime caching is enough for the MVP: tapes are tiny source files and VHS rendering is expensive.
        if !force && is_newer_than(&out, &tape_path)? {
            println!("skipped {rel_tape} (up to date)");
            continue;
        }
        let status = std::process::Command::new(&vhs)
            .current_dir(deck_dir)
            .arg(&rel_tape)
            .status()?;
        if !status.success() {
            anyhow::bail!("vhs failed while rendering {rel_tape}");
        }
        if !out.is_file() {
            anyhow::bail!(
                "expected output not found: {rel_out}; add `Output \"{rel_out}\"` to {rel_tape}"
            );
        }
        let bytes = fs::metadata(&out)?.len();
        println!("rendered {rel_tape} -> {rel_out} ({bytes} bytes)");
    }
    Ok(())
}

fn is_newer_than(out: &Path, input: &Path) -> anyhow::Result<bool> {
    Ok(out.is_file() && fs::metadata(out)?.modified()? >= fs::metadata(input)?.modified()?)
}

fn video(command: VideoCommand) -> anyhow::Result<()> {
    match command {
        VideoCommand::Optimize {
            file,
            quality,
            max_dim,
            keep_audio,
        } => {
            let ffmpeg = find_tool(
                "ffmpeg",
                "SIDESHOW_FFMPEG",
                "sideshow video optimize needs ffmpeg to re-encode videos",
                "https://ffmpeg.org/download.html",
            )?;
            let old = fs::metadata(&file)?.len();
            let out = file.with_extension("webm");
            let tmp = out.with_extension("webm.tmp");
            let scale = format!(
                "scale='if(gt(iw,ih),min({max_dim},iw),-2)':'if(gt(iw,ih),-2,min({max_dim},ih))'"
            );
            let quality = quality.to_string();
            let mut cmd = std::process::Command::new(ffmpeg);
            cmd.args(["-y", "-i"]).arg(&file).args([
                "-c:v",
                "libvpx-vp9",
                "-crf",
                &quality,
                "-b:v",
                "0",
                "-vf",
                &scale,
            ]);
            if keep_audio {
                cmd.args(["-c:a", "libopus", "-b:a", "64k"]);
            } else {
                cmd.arg("-an");
            }
            let status = cmd.args(["-f", "webm"]).arg(&tmp).status()?;
            if !status.success() {
                let _ = fs::remove_file(&tmp);
                anyhow::bail!("ffmpeg failed while optimizing video");
            }
            let new = fs::metadata(&tmp)?.len();
            if new < old {
                fs::rename(&tmp, &out)?;
                println!(
                    "optimized {} -> {}: {} -> {} bytes",
                    file.display(),
                    out.display(),
                    old,
                    new
                );
            } else {
                let _ = fs::remove_file(&tmp);
                println!(
                    "kept {}: optimized WebM was not smaller than {} bytes",
                    file.display(),
                    old
                );
            }
        }
    }
    Ok(())
}

fn img(command: ImgCommand) -> anyhow::Result<()> {
    match command {
        ImgCommand::Info { paths, format } => {
            let infos = paths
                .iter()
                .map(|p| sideshow::image_info(p))
                .collect::<anyhow::Result<Vec<_>>>()?;
            if format.as_deref() == Some("json") {
                println!("{}", serde_json::to_string_pretty(&infos)?);
            } else {
                for i in infos {
                    println!(
                        "{}: {}x{} {} file={} inline={} bytes",
                        i.path, i.width, i.height, i.format, i.file_size, i.projected_inline_size
                    );
                }
            }
        }
        ImgCommand::Resize {
            path,
            width,
            height,
            out,
        } => {
            let dest = sideshow::resize_image(&path, width, height, out.as_deref())?;
            println!("resized {} -> {}", path.display(), dest.display());
        }
        ImgCommand::Crop { path, rect, out } => {
            let dest = sideshow::crop_image(&path, &rect, out.as_deref())?;
            println!("cropped {} -> {}", path.display(), dest.display());
        }
        ImgCommand::Optimize {
            paths,
            quality,
            max_dim,
            in_place,
        } => {
            for p in paths {
                match sideshow::optimize_image(&p, quality, max_dim, in_place) {
                    Ok((out, old, Some(new))) => println!(
                        "optimized {} -> {}: {} -> {} bytes",
                        p.display(),
                        out.display(),
                        old,
                        new
                    ),
                    Ok((_out, old, None)) => println!(
                        "kept {}: optimized WebP was not smaller than {} bytes",
                        p.display(),
                        old
                    ),
                    Err(e)
                        if p.extension()
                            .and_then(|s| s.to_str())
                            .is_some_and(|s| s.eq_ignore_ascii_case("svg")) =>
                    {
                        println!("skip {}: {e}", p.display())
                    }
                    Err(e) => return Err(e),
                }
            }
        }
    }
    Ok(())
}

const CSP: &str = "default-src 'self' data: blob:; script-src 'self' 'unsafe-eval' 'unsafe-inline'; style-src 'self' 'unsafe-inline'; worker-src 'self' 'unsafe-eval' 'unsafe-inline' data: blob:; frame-src https:; frame-ancestors 'none'; img-src data: https:; media-src https:; object-src 'none'; sandbox allow-downloads allow-forms allow-modals allow-pointer-lock allow-popups allow-presentation allow-same-origin allow-scripts;";
const MAX_HTTP_HEADER_BYTES: usize = 16 * 1024;
const MAX_HTTP_BODY_BYTES: usize = 64 * 1024;
const MAX_STABLE_BUILD_ATTEMPTS: usize = 3;
const MAX_MANIFEST_UPDATE_ATTEMPTS: usize = 8;
const REVIEW_CSS: &str = include_str!("review/review.css");
const REVIEW_JS: &str = include_str!("review/review.js");

#[derive(Debug)]
struct StableBuild {
    deck_root: PathBuf,
    staged_output: PathBuf,
    output: PathBuf,
    staging_dir: PathBuf,
    manifest: Option<sideshow::review::ReviewBuildManifest>,
    input_digest: String,
    output_bytes: Arc<[u8]>,
}

impl Drop for StableBuild {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.staging_dir);
    }
}

#[derive(Debug)]
struct AcceptedBuild {
    output: PathBuf,
    input_digest: String,
    output_bytes: Arc<[u8]>,
}

impl StableBuild {
    fn publish_file(
        &self,
        target: sideshow::review::ReviewPublicationTarget<'_>,
    ) -> std::io::Result<sideshow::secure_fs::AtomicWriteOutcome> {
        let current_digest = deck_input_digest(&self.deck_root).map_err(std::io::Error::other)?;
        if current_digest != self.input_digest {
            return Err(std::io::Error::other(
                "deck inputs changed after the stable build was captured",
            ));
        }
        let current_output = fs::read(&self.staged_output)?;
        if current_output.as_slice() != self.output_bytes.as_ref() {
            return Err(std::io::Error::other(
                "staged deck bytes changed after the stable build was captured",
            ));
        }
        fs::File::open(&self.staged_output)?.sync_all()?;
        let source =
            cap_std::fs::Dir::open_ambient_dir(&self.staging_dir, cap_std::ambient_authority())?;
        let ordinary_destination = match &target {
            sideshow::review::ReviewPublicationTarget::Ordinary => {
                Some(fs::File::open(self.output.parent().ok_or_else(|| {
                    std::io::Error::other("published deck has no parent directory")
                })?)?)
            }
            sideshow::review::ReviewPublicationTarget::Review { directory, .. } => {
                sideshow::secure_fs::sync_directory(directory)?;
                None
            }
        };
        // Directory sync failures before visibility remain ordinary, uncommitted errors.
        if let Some(destination) = &ordinary_destination {
            destination.sync_all()?;
        }
        sideshow::secure_fs::sync_directory(&source)?;
        let post_sync = match target {
            sideshow::review::ReviewPublicationTarget::Ordinary => {
                fs::rename(&self.staged_output, &self.output)?;
                ordinary_destination
                    .expect("ordinary publication has a destination directory")
                    .sync_all()
            }
            sideshow::review::ReviewPublicationTarget::Review {
                directory,
                file_name,
            } => {
                let staged_name = self
                    .staged_output
                    .file_name()
                    .ok_or_else(|| std::io::Error::other("staged deck has no file name"))?;
                source.rename(Path::new(staged_name), directory, Path::new(file_name))?;
                sideshow::secure_fs::sync_directory(directory)
            }
        };
        // The rename is the visibility boundary. Preserve committed semantics on any later sync
        // failure so paired publication can retry durability instead of assuming the old deck won.
        Ok(
            match post_sync.and_then(|()| sideshow::secure_fs::sync_directory(&source)) {
                Ok(()) => sideshow::secure_fs::AtomicWriteOutcome::Durable,
                Err(error) => {
                    sideshow::secure_fs::AtomicWriteOutcome::CommittedButDirectorySyncFailed(error)
                }
            },
        )
    }

    fn publish(self) -> anyhow::Result<AcceptedBuild> {
        let _committed = self
            .publish_file(sideshow::review::ReviewPublicationTarget::Ordinary)
            .with_context(|| {
                format!(
                    "failed to atomically publish {} as {}",
                    self.staged_output.display(),
                    self.output.display()
                )
            })?;
        Ok(AcceptedBuild {
            output: self.output.clone(),
            input_digest: self.input_digest.clone(),
            output_bytes: Arc::clone(&self.output_bytes),
        })
    }
}

struct ReviewServer {
    nonce: String,
    repository: sideshow::review::ReviewRepository,
}

#[derive(Clone)]
struct AcceptedServeState {
    output: PathBuf,
    html: Arc<[u8]>,
    generation: u64,
}

fn serve(dir: &Path, port: u16, review: bool, open: bool) -> anyhow::Result<()> {
    let built = stable_build_deck(dir, review)?;
    let canonical_dir = built.deck_root.clone();
    let review_server = review
        .then(|| -> anyhow::Result<_> {
            let nonce = review_nonce()?;
            let repository = sideshow::review::ReviewRepository::new(&canonical_dir)
                .map_err(review_cli_error)?;
            Ok(Arc::new(ReviewServer { repository, nonce }))
        })
        .transpose()?;
    let built = accept_stable_build(
        built,
        review_server.as_ref().map(|server| &server.repository),
    )?;
    let (dir, root, out) = normalized_serve_paths(dir, built.output)?;
    let accepted_state = Arc::new(Mutex::new(AcceptedServeState {
        output: out.clone(),
        html: Arc::clone(&built.output_bytes),
        generation: reload_session_id() << 32,
    }));
    let review = review_server;
    let listener = TcpListener::bind(("127.0.0.1", port))?;
    let url = serve_url(&listener)?;
    let _watcher = start_rebuild_watcher(
        dir.to_path_buf(),
        root.clone(),
        Arc::clone(&accepted_state),
        review.as_ref().map(|server| server.repository.clone()),
        built.input_digest,
    )?;
    let port = listener.local_addr()?.port();
    let server = start_http_server(
        listener,
        root.clone(),
        Arc::clone(&accepted_state),
        review.clone(),
        port,
    )?;
    println!("serving {} at {url}", root.display());
    if review.is_some() {
        println!("review mode enabled (annotations persist in XDG state)");
    }
    if open {
        open_url(&url).with_context(|| format!("failed to open {url}"))?;
    }
    server.wait()
}

struct HttpServer {
    shutdown: mpsc::Sender<()>,
    worker: Option<thread::JoinHandle<anyhow::Result<()>>>,
}

impl HttpServer {
    fn wait(mut self) -> anyhow::Result<()> {
        let worker = self
            .worker
            .take()
            .context("HTTP server worker is missing")?;
        worker
            .join()
            .map_err(|_| anyhow::anyhow!("HTTP server worker panicked"))?
    }
}

impl Drop for HttpServer {
    fn drop(&mut self) {
        let _ = self.shutdown.send(());
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn start_http_server(
    listener: TcpListener,
    root: PathBuf,
    accepted_state: Arc<Mutex<AcceptedServeState>>,
    review: Option<Arc<ReviewServer>>,
    port: u16,
) -> anyhow::Result<HttpServer> {
    listener.set_nonblocking(true)?;
    let (shutdown_tx, shutdown_rx) = mpsc::channel();
    let worker = thread::spawn(move || -> anyhow::Result<()> {
        loop {
            if shutdown_rx.try_recv().is_ok() {
                return Ok(());
            }
            match listener.accept() {
                Ok((mut stream, _)) => {
                    let root = root.clone();
                    let accepted_state = Arc::clone(&accepted_state);
                    let review = review.clone();
                    thread::spawn(move || {
                        let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
                        let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));
                        let _ = handle_stream(
                            &mut stream,
                            &root,
                            &accepted_state,
                            review.as_deref(),
                            port,
                        );
                    });
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(20));
                }
                Err(error) => return Err(error.into()),
            }
        }
    });
    Ok(HttpServer {
        shutdown: shutdown_tx,
        worker: Some(worker),
    })
}

fn serve_url(listener: &TcpListener) -> anyhow::Result<String> {
    let addr = listener.local_addr()?;
    Ok(format!("http://{}:{}/", addr.ip(), addr.port()))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct OpenerCommand {
    program: String,
    args: Vec<String>,
}

fn open_url(url: &str) -> anyhow::Result<()> {
    let opener = resolve_opener(&SystemOpenEnv)?;
    run_opener(&opener, url)
}

fn run_opener(opener: &OpenerCommand, url: &str) -> anyhow::Result<()> {
    let status = ProcessCommand::new(&opener.program)
        .args(&opener.args)
        .arg(url)
        .status()
        .with_context(|| format!("could not run opener `{}`", opener.program))?;
    if status.success() {
        Ok(())
    } else {
        anyhow::bail!("opener `{}` exited with {status}", opener.program)
    }
}

struct RebuildWatcher {
    shutdown: mpsc::Sender<()>,
    worker: Option<thread::JoinHandle<()>>,
}

impl Drop for RebuildWatcher {
    fn drop(&mut self) {
        let _ = self.shutdown.send(());
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

trait OpenEnv {
    fn os(&self) -> &str;
    fn var(&self, name: &str) -> Option<String>;
    fn executable_on_path(&self, program: &str) -> bool;
}

struct SystemOpenEnv;

impl OpenEnv for SystemOpenEnv {
    fn os(&self) -> &str {
        env::consts::OS
    }

    fn var(&self, name: &str) -> Option<String> {
        env::var_os(name).map(|v| v.to_string_lossy().into_owned())
    }

    fn executable_on_path(&self, program: &str) -> bool {
        env::var_os("PATH").is_some_and(|paths| {
            env::split_paths(&paths).any(|dir| {
                let path = dir.join(program);
                path.is_file() && is_executable(&path)
            })
        })
    }
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;

    fs::metadata(path)
        .map(|metadata| metadata.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.exists()
}

fn resolve_opener(env: &impl OpenEnv) -> anyhow::Result<OpenerCommand> {
    match env.os() {
        "macos" => Ok(OpenerCommand {
            program: "open".into(),
            args: Vec::new(),
        }),
        "linux" => {
            if env.var("DISPLAY").is_none() && env.var("WAYLAND_DISPLAY").is_none() {
                anyhow::bail!(
                    "--open needs a graphical Linux session; set DISPLAY or WAYLAND_DISPLAY, or run without --open"
                );
            }
            for program in ["xdg-open", "gio", "kde-open", "gnome-open"] {
                if env.executable_on_path(program) {
                    let args = if program == "gio" {
                        vec!["open".into()]
                    } else {
                        Vec::new()
                    };
                    return Ok(OpenerCommand {
                        program: program.into(),
                        args,
                    });
                }
            }
            anyhow::bail!(
                "--open could not find a desktop opener; install xdg-open (xdg-utils), gio, kde-open, or gnome-open, or run without --open"
            )
        }
        other => anyhow::bail!(
            "--open is unsupported on {other}; use macOS `open` or a Linux desktop opener, or run without --open"
        ),
    }
}

fn start_rebuild_watcher(
    dir: PathBuf,
    dist: PathBuf,
    accepted_state: Arc<Mutex<AcceptedServeState>>,
    review_repository: Option<sideshow::review::ReviewRepository>,
    initial_input_digest: String,
) -> anyhow::Result<RebuildWatcher> {
    let (tx, rx) = mpsc::channel::<notify::Result<Event>>();
    let mut watcher = RecommendedWatcher::new(
        move |res| {
            let _ = tx.send(res);
        },
        Config::default(),
    )?;
    watcher.watch(&dir, RecursiveMode::Recursive)?;
    // Close the gap between the final stable-build digest and watcher installation. Events after
    // installation are queued normally; this comparison catches an edit that landed just before
    // the watcher became active.
    let rebuild_after_start = deck_input_digest(&dir)
        .map(|digest| digest != initial_input_digest)
        .unwrap_or(true);
    let (shutdown_tx, shutdown_rx) = mpsc::channel();
    let worker = thread::spawn(move || {
        let _watcher = watcher;
        let mut should_rebuild = rebuild_after_start;
        loop {
            if shutdown_rx.try_recv().is_ok() {
                break;
            }
            if !should_rebuild {
                let res = match rx.recv_timeout(Duration::from_millis(250)) {
                    Ok(res) => res,
                    Err(mpsc::RecvTimeoutError::Timeout) => continue,
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                };
                let deadline = std::time::Instant::now() + Duration::from_secs(2);
                should_rebuild = event_is_relevant(res, &dir, &dist);
                while let Some(remaining) =
                    deadline.checked_duration_since(std::time::Instant::now())
                {
                    let timeout = remaining.min(Duration::from_millis(250));
                    let Ok(res) = rx.recv_timeout(timeout) else {
                        break;
                    };
                    should_rebuild |= event_is_relevant(res, &dir, &dist);
                }
            }
            if should_rebuild {
                eprintln!("change detected; rebuilding deck...");
                match stable_build_deck(&dir, review_repository.is_some()) {
                    Ok(built) => {
                        match accept_stable_build(built, review_repository.as_ref()) {
                            Ok(built) => match accepted_state.lock() {
                                Ok(mut state) => {
                                    state.output = built.output.clone();
                                    state.html = Arc::clone(&built.output_bytes);
                                    // The generation represents an already-published deck whose
                                    // review identity (when enabled) is durable.
                                    state.generation = state.generation.wrapping_add(1);
                                    eprintln!("rebuilt {}", built.output.display());
                                }
                                Err(_) => eprintln!(
                                    "rebuild publication failed: current output lock is poisoned"
                                ),
                            },
                            Err(error) => eprintln!(
                                "rebuild rejected; continuing to serve the last accepted deck: {error:#}"
                            ),
                        }
                    }
                    Err(err) => eprintln!("rebuild failed: {err:#}"),
                }
            }
            should_rebuild = false;
        }
    });
    Ok(RebuildWatcher {
        shutdown: shutdown_tx,
        worker: Some(worker),
    })
}

fn accept_stable_build(
    built: StableBuild,
    review_repository: Option<&sideshow::review::ReviewRepository>,
) -> anyhow::Result<AcceptedBuild> {
    if let Some(repository) = review_repository {
        let manifest = built
            .manifest
            .clone()
            .context("stable review build did not produce a manifest")?;
        let mut revision = repository
            .load_artifact()
            .map_err(review_cli_error)
            .context("could not synchronize review manifest; new deck was not published")?
            .revision;
        for attempt in 0..MAX_MANIFEST_UPDATE_ATTEMPTS {
            match repository.update_build_manifest_and_publish(
                revision,
                manifest.clone(),
                &built.output,
                |target| built.publish_file(target),
            ) {
                Ok(_) => {
                    return Ok(AcceptedBuild {
                        output: built.output.clone(),
                        input_digest: built.input_digest.clone(),
                        output_bytes: Arc::clone(&built.output_bytes),
                    });
                }
                Err(sideshow::review::ReviewRepositoryError::Conflict(current)) => {
                    revision = current.revision;
                    if attempt + 1 < MAX_MANIFEST_UPDATE_ATTEMPTS {
                        thread::sleep(Duration::from_millis(1));
                    }
                }
                Err(error) => {
                    return Err(review_cli_error(error)).context(
                        "could not synchronize review manifest; new deck was not published",
                    );
                }
            }
        }
        anyhow::bail!(
            "review manifest update conflicted {MAX_MANIFEST_UPDATE_ATTEMPTS} times; new deck was not published"
        );
    }
    built.publish()
}

fn accept_ordinary_build(
    mut built: StableBuild,
    review_repository: Option<&sideshow::review::ReviewRepository>,
) -> anyhow::Result<AcceptedBuild> {
    let Some(repository) = review_repository else {
        return built.publish();
    };
    match repository
        .publish_build_if_initialized(None, &built.output, |target| built.publish_file(target))
    {
        Ok(_) => {}
        Err(sideshow::review::ReviewRepositoryError::ManifestRequired) => {
            let deck_root = built.deck_root.clone();
            drop(built);
            built = stable_build_deck(&deck_root, true)?;
            let manifest = built
                .manifest
                .clone()
                .context("stable review-aware build did not produce a manifest")?;
            repository
                .publish_build_if_initialized(Some(manifest), &built.output, |target| {
                    built.publish_file(target)
                })
                .map_err(review_cli_error)
                .context("could not safely synchronize an existing review artifact")?;
        }
        Err(error) => {
            return Err(review_cli_error(error))
                .context("could not safely discover an existing review artifact");
        }
    }
    Ok(AcceptedBuild {
        output: built.output.clone(),
        input_digest: built.input_digest.clone(),
        output_bytes: Arc::clone(&built.output_bytes),
    })
}

fn stable_build_deck(deck_root: &Path, capture_manifest: bool) -> anyhow::Result<StableBuild> {
    stable_build_deck_with(deck_root, capture_manifest, sideshow::build_deck_to)
}

fn stable_build_deck_with(
    deck_root: &Path,
    capture_manifest: bool,
    mut build: impl FnMut(&Path, &Path) -> anyhow::Result<PathBuf>,
) -> anyhow::Result<StableBuild> {
    let deck_root = deck_root
        .canonicalize()
        .with_context(|| format!("failed to resolve deck root {}", deck_root.display()))?;
    for attempt in 1..=MAX_STABLE_BUILD_ATTEMPTS {
        let before = deck_input_digest(&deck_root)?;
        let staging_dir = create_build_staging_dir(&deck_root)?;
        let staged_output = match build(&deck_root, &staging_dir) {
            Ok(output) => output,
            Err(error) => {
                let _ = fs::remove_dir_all(&staging_dir);
                return Err(error);
            }
        };
        let output = deck_root.join("dist").join(
            staged_output
                .file_name()
                .context("staged build output has no file name")?,
        );
        let manifest = capture_manifest
            .then(|| review_build_manifest(&deck_root, &staged_output))
            .transpose();
        let manifest = match manifest {
            Ok(manifest) => manifest,
            Err(error) => {
                let _ = fs::remove_dir_all(&staging_dir);
                return Err(error);
            }
        };
        let after = match deck_input_digest(&deck_root) {
            Ok(digest) => digest,
            Err(error) => {
                let _ = fs::remove_dir_all(&staging_dir);
                return Err(error);
            }
        };
        if before == after {
            let output_bytes: Arc<[u8]> = fs::read(&staged_output)
                .with_context(|| {
                    format!("failed to capture built deck {}", staged_output.display())
                })?
                .into();
            return Ok(StableBuild {
                deck_root: deck_root.clone(),
                staged_output,
                output,
                staging_dir,
                manifest,
                input_digest: after,
                output_bytes,
            });
        }
        let _ = fs::remove_dir_all(&staging_dir);
        if attempt < MAX_STABLE_BUILD_ATTEMPTS {
            eprintln!(
                "deck inputs changed during build; retrying ({attempt}/{MAX_STABLE_BUILD_ATTEMPTS})"
            );
        }
    }
    anyhow::bail!(
        "deck inputs kept changing during {MAX_STABLE_BUILD_ATTEMPTS} build attempts; try again when edits settle"
    )
}

fn create_build_staging_dir(deck_root: &Path) -> anyhow::Result<PathBuf> {
    let dist = deck_root.join("dist");
    fs::create_dir_all(&dist)?;
    for _ in 0..16 {
        let mut random = [0_u8; 12];
        getrandom::fill(&mut random)
            .map_err(|error| anyhow::anyhow!("failed to name staged build: {error}"))?;
        let suffix = random
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        // Keep candidates outside the HTTP-served dist tree while retaining same-filesystem
        // atomic rename semantics for the final publish.
        let staging = deck_root.join(format!(".sideshow-stage-{suffix}"));
        match fs::create_dir(&staging) {
            Ok(()) => return Ok(staging),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        }
    }
    anyhow::bail!("could not allocate a unique staged build directory")
}

fn deck_input_digest(deck_root: &Path) -> anyhow::Result<String> {
    let mut inputs = deck_input_files(deck_root)?;
    inputs.sort_by(|left, right| left.0.cmp(&right.0));
    let mut digest = Sha256::new();
    for (path, bytes) in inputs {
        digest_field(&mut digest, path.as_bytes(), &bytes);
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn deck_input_files(deck_root: &Path) -> anyhow::Result<Vec<(String, Vec<u8>)>> {
    let deck_root = deck_root
        .canonicalize()
        .with_context(|| format!("failed to resolve deck root {}", deck_root.display()))?;
    let mut inputs = Vec::new();
    let deck_toml_path = deck_root.join("deck.toml");
    let deck_toml = fs::read(&deck_toml_path)
        .with_context(|| format!("failed to read build input {}", deck_toml_path.display()))?;
    inputs.push(("deck.toml".to_owned(), deck_toml.clone()));
    let theme_path = deck_root.join("theme.css");
    inputs.push((
        "theme.css".to_owned(),
        fs::read(&theme_path)
            .with_context(|| format!("failed to read build input {}", theme_path.display()))?,
    ));
    collect_deck_input_tree(&deck_root, Path::new("slides"), true, &mut inputs)?;
    collect_deck_input_tree(&deck_root, Path::new("assets"), false, &mut inputs)?;
    // deck.toml can name slide files outside the conventional slides/ directory. Include the
    // exact selected source set as well as the conservative trees above.
    let deck = sideshow::parse_deck_toml(std::str::from_utf8(&deck_toml)?)?;
    for path in sideshow::slide_order(&deck_root, &deck)? {
        let name = path
            .strip_prefix(&deck_root)
            .expect("slide_order confines canonical sources to the canonical deck root")
            .to_str()
            .with_context(|| format!("slide path is not valid UTF-8: {}", path.display()))?
            .replace('\\', "/");
        inputs.push((
            name,
            fs::read(&path)
                .with_context(|| format!("failed to read slide source {}", path.display()))?,
        ));
    }
    Ok(inputs)
}

fn collect_deck_input_tree(
    deck_root: &Path,
    relative: &Path,
    required: bool,
    inputs: &mut Vec<(String, Vec<u8>)>,
) -> anyhow::Result<()> {
    let directory = deck_root.join(relative);
    let entries = match fs::read_dir(&directory) {
        Ok(entries) => entries.collect::<Result<Vec<_>, _>>()?,
        Err(error) if !required && error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(error).with_context(|| {
                format!(
                    "failed to read build input directory {}",
                    directory.display()
                )
            });
        }
    };
    let mut entries = entries;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        let child_relative = relative.join(entry.file_name());
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            collect_deck_input_tree(deck_root, &child_relative, true, inputs)?;
        } else {
            let normalized = child_relative
                .to_str()
                .with_context(|| {
                    format!("build input path is not valid UTF-8: {}", path.display())
                })?
                .replace('\\', "/");
            inputs.push((
                normalized,
                fs::read(&path)
                    .with_context(|| format!("failed to read build input {}", path.display()))?,
            ));
        }
    }
    Ok(())
}

fn review_build_manifest(
    deck_root: &Path,
    output: &Path,
) -> anyhow::Result<sideshow::review::ReviewBuildManifest> {
    let deck_root = deck_root
        .canonicalize()
        .with_context(|| format!("failed to resolve deck root {}", deck_root.display()))?;
    let output_bytes = fs::read(output)
        .with_context(|| format!("failed to read built deck {}", output.display()))?;
    let build_id = sha256_hex(&output_bytes);
    let deck_toml = fs::read(deck_root.join("deck.toml"))?;
    let theme_css = match fs::read(deck_root.join("theme.css")) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(error) => return Err(error.into()),
    };
    let deck = sideshow::parse_deck_toml(std::str::from_utf8(&deck_toml)?)?;
    let slide_paths = sideshow::slide_order(&deck_root, &deck)?;
    let mut assets = Vec::new();
    collect_deck_input_tree(&deck_root, Path::new("assets"), false, &mut assets)?;
    assets.sort_by(|left, right| left.0.cmp(&right.0));
    let mut slides = Vec::with_capacity(slide_paths.len());
    for path in slide_paths {
        let source = fs::read(&path)
            .with_context(|| format!("failed to read slide source {}", path.display()))?;
        let relative = path
            .strip_prefix(&deck_root)
            .with_context(|| format!("slide source is outside deck root: {}", path.display()))?
            .to_str()
            .context("slide source path must be valid UTF-8")?
            .replace('\\', "/");
        let stem = path
            .file_stem()
            .and_then(|value| value.to_str())
            .context("slide file name must be valid UTF-8")?;
        let mut digest = Sha256::new();
        digest_field(&mut digest, b"deck.toml", &deck_toml);
        digest_field(&mut digest, b"theme.css", &theme_css);
        digest_field(&mut digest, relative.as_bytes(), &source);
        for (asset_path, asset_bytes) in &assets {
            digest_field(&mut digest, asset_path.as_bytes(), asset_bytes);
        }
        slides.push(sideshow::review::ReviewSlideManifest {
            slide_id: format!("s-{stem}"),
            source_path: relative,
            source_digest: format!("{:x}", digest.finalize()),
        });
    }
    Ok(sideshow::review::ReviewBuildManifest {
        build_id,
        built_at_ms: unix_time_ms(),
        slides,
        // Handoff verification is regenerated from ReviewRepository's canonical deck context;
        // executable text is never persisted in editable review JSON.
        verification_commands: Vec::new(),
    })
}

fn digest_field(hasher: &mut Sha256, name: &[u8], bytes: &[u8]) {
    hasher.update((name.len() as u64).to_be_bytes());
    hasher.update(name);
    hasher.update((bytes.len() as u64).to_be_bytes());
    hasher.update(bytes);
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn unix_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

fn event_is_relevant(res: notify::Result<Event>, deck_dir: &Path, dist: &Path) -> bool {
    match res {
        Ok(event) => {
            if event.need_rescan() {
                return true;
            }
            if !event_kind_is_build_input(&event.kind) {
                return false;
            }
            event
                .paths
                .iter()
                .any(|path| path != deck_dir && build_input_path_is_relevant(path, deck_dir, dist))
        }
        Err(err) => {
            eprintln!("watch error: {err}");
            false
        }
    }
}

fn event_kind_is_build_input(kind: &EventKind) -> bool {
    use notify::event::{CreateKind, ModifyKind, RemoveKind, RenameMode};
    matches!(
        kind,
        EventKind::Create(CreateKind::Any | CreateKind::File | CreateKind::Folder)
            | EventKind::Remove(RemoveKind::Any | RemoveKind::File | RemoveKind::Folder)
            | EventKind::Modify(
                ModifyKind::Any
                    | ModifyKind::Data(_)
                    | ModifyKind::Name(
                        RenameMode::Any | RenameMode::Both | RenameMode::From | RenameMode::To,
                    ),
            )
            | EventKind::Any
    )
}

fn normalized_serve_paths(
    input_dir: &Path,
    output: PathBuf,
) -> anyhow::Result<(PathBuf, PathBuf, PathBuf)> {
    let dir = input_dir
        .canonicalize()
        .with_context(|| format!("failed to resolve deck directory {}", input_dir.display()))?;
    let root = dir.join("dist");
    let output = if output.is_absolute() {
        output
    } else {
        std::env::current_dir()?.join(output)
    };
    let output = output.canonicalize().unwrap_or(output);
    Ok((dir, root, output))
}

fn build_input_path_is_relevant(path: &Path, deck_dir: &Path, dist: &Path) -> bool {
    if is_in_dir(path, dist) {
        return false;
    }
    let rel = path.strip_prefix(deck_dir).unwrap_or(path);
    rel == Path::new("deck.toml")
        || rel == Path::new("theme.css")
        || has_top_level_component(rel, "slides")
        || has_top_level_component(rel, "assets")
        || configured_slide_path_is_relevant(path, deck_dir)
}

fn configured_slide_path_is_relevant(path: &Path, deck_dir: &Path) -> bool {
    fs::read_to_string(deck_dir.join("deck.toml"))
        .ok()
        .and_then(|source| sideshow::parse_deck_toml(&source).ok())
        .and_then(|deck| deck.deck.slides)
        .is_some_and(|slides| slides.iter().any(|slide| deck_dir.join(slide) == path))
}

fn has_top_level_component(path: &Path, name: &str) -> bool {
    matches!(path.components().next(), Some(std::path::Component::Normal(component)) if component == name)
}

fn is_in_dir(path: &Path, dir: &Path) -> bool {
    if path == dir || path.starts_with(dir) {
        return true;
    }
    match (path.canonicalize(), dir.canonicalize()) {
        (Ok(path), Ok(dir)) => path == dir || path.starts_with(dir),
        _ => false,
    }
}

struct HttpRequest {
    method: String,
    path: String,
    headers: BTreeMap<String, String>,
    body: Vec<u8>,
}

fn read_request(stream: &mut TcpStream) -> anyhow::Result<Option<HttpRequest>> {
    let mut bytes = Vec::new();
    let mut chunk = [0_u8; 2048];
    let header_end = loop {
        let read = stream.read(&mut chunk)?;
        if read == 0 {
            respond_status(stream, "400 Bad Request")?;
            return Ok(None);
        }
        bytes.extend_from_slice(&chunk[..read]);
        if let Some(index) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            break index + 4;
        }
        if bytes.len() > MAX_HTTP_HEADER_BYTES {
            respond_status(stream, "431 Request Header Fields Too Large")?;
            return Ok(None);
        }
    };
    if header_end > MAX_HTTP_HEADER_BYTES {
        respond_status(stream, "431 Request Header Fields Too Large")?;
        return Ok(None);
    }

    let Ok(head) = std::str::from_utf8(&bytes[..header_end]) else {
        respond_status(stream, "400 Bad Request")?;
        return Ok(None);
    };
    let mut lines = head[..head.len() - 4].split("\r\n");
    let Some(line) = lines.next() else {
        respond_status(stream, "400 Bad Request")?;
        return Ok(None);
    };
    let Some(request_line) = parse_request_line(line) else {
        respond_status(stream, "400 Bad Request")?;
        return Ok(None);
    };
    let mut headers = BTreeMap::new();
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            respond_status(stream, "400 Bad Request")?;
            return Ok(None);
        };
        let name = name.trim().to_ascii_lowercase();
        if name.is_empty() || headers.insert(name, value.trim().to_owned()).is_some() {
            respond_status(stream, "400 Bad Request")?;
            return Ok(None);
        }
    }
    if headers.contains_key("transfer-encoding") {
        respond_status(stream, "400 Bad Request")?;
        return Ok(None);
    }
    let content_length = match headers.get("content-length") {
        Some(value) => match value.parse::<usize>() {
            Ok(length) => length,
            Err(_) => {
                respond_status(stream, "400 Bad Request")?;
                return Ok(None);
            }
        },
        None => 0,
    };
    if content_length > MAX_HTTP_BODY_BYTES {
        respond_status(stream, "413 Payload Too Large")?;
        return Ok(None);
    }
    let method = request_line.method.to_owned();
    let path = request_line.path;
    while bytes.len() - header_end < content_length {
        let read = stream.read(&mut chunk)?;
        if read == 0 {
            respond_status(stream, "400 Bad Request")?;
            return Ok(None);
        }
        bytes.extend_from_slice(&chunk[..read]);
    }
    let mut body = bytes[header_end..].to_vec();
    body.truncate(content_length);
    Ok(Some(HttpRequest {
        method,
        path,
        headers,
        body,
    }))
}

fn handle_stream(
    stream: &mut TcpStream,
    root: &Path,
    accepted_state: &Mutex<AcceptedServeState>,
    review: Option<&ReviewServer>,
    port: u16,
) -> anyhow::Result<()> {
    let Some(request) = read_request(stream)? else {
        return Ok(());
    };
    if !request_has_allowed_host(&request, port) {
        return respond_status(stream, "421 Misdirected Request");
    }
    let state = accepted_state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let accepted = state.clone();
    drop(state);
    if request.path == "__sideshow/review" {
        return match review {
            Some(review) => respond_review(stream, &request, review),
            None => respond_status(stream, "404 Not Found"),
        };
    }
    if request.path == "__sideshow/reload" {
        if request.method != "GET" && request.method != "HEAD" {
            return respond_status(stream, "405 Method Not Allowed");
        }
        return respond_reload(stream, accepted.generation, request.method == "HEAD");
    }
    if request.method != "GET" && request.method != "HEAD" {
        return respond_status(stream, "405 Method Not Allowed");
    }
    respond(
        stream,
        root,
        &request.path,
        &accepted,
        request.method == "HEAD",
        review.map(|review| review.nonce.as_str()),
    )
}

struct RequestLine<'a> {
    method: &'a str,
    path: String,
}

fn parse_request_line(req: &str) -> Option<RequestLine<'_>> {
    let mut parts = req.lines().next()?.split_whitespace();
    let method = parts.next()?;
    let target = parts.next()?;
    if parts.next()? != "HTTP/1.1" || parts.next().is_some() {
        return None;
    }
    let target = target.split(['?', '#']).next().unwrap_or(target);
    let path = safe_request_path(target)?;
    Some(RequestLine { method, path })
}

fn safe_request_path(target: &str) -> Option<String> {
    if !target.starts_with('/') || target.starts_with("//") {
        return None;
    }
    let trimmed = target.trim_start_matches('/');
    let decoded = percent_decode(trimmed)?;
    let p = Path::new(&decoded);
    if p.is_absolute()
        || p.components()
            .any(|c| !matches!(c, std::path::Component::Normal(_)))
    {
        return None;
    }
    Some(decoded)
}

fn percent_decode(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if i + 2 >= bytes.len() {
                return None;
            }
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

fn respond_status(stream: &mut TcpStream, code: &str) -> anyhow::Result<()> {
    let body = format!("<!doctype html><title>{code}</title><h1>{code}</h1>");
    write!(
        stream,
        "HTTP/1.1 {code}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nContent-Security-Policy: {CSP}\r\nX-Frame-Options: DENY\r\nX-Content-Type-Options: nosniff\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )?;
    Ok(())
}
fn respond(
    stream: &mut TcpStream,
    root: &Path,
    path: &str,
    accepted: &AcceptedServeState,
    head_only: bool,
    review_nonce: Option<&str>,
) -> anyhow::Result<()> {
    let requested = if path.is_empty() {
        accepted.output.clone()
    } else {
        root.join(path)
    };
    let accepted_name = accepted.output.file_name();
    let request_path = Path::new(path);
    let is_accepted_deck = path.is_empty()
        || (request_path
            .parent()
            .is_some_and(|parent| parent.as_os_str().is_empty())
            && request_path.file_name() == accepted_name);
    let file = (!is_accepted_deck)
        .then(|| resolve_served_file(root, &requested))
        .flatten();
    let (code, mut body) = if is_accepted_deck {
        ("200 OK", accepted.html.to_vec())
    } else if let Some(file) = file.as_ref() {
        ("200 OK", fs::read(file)?)
    } else {
        (
            "404 Not Found",
            b"<!doctype html><title>404 Not Found</title><h1>404 Not Found</h1>".to_vec(),
        )
    };
    let mime = match requested.extension().and_then(|s| s.to_str()).unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "css" => "text/css",
        "js" => "text/javascript",
        "json" => "application/json",
        "svg" => "image/svg+xml",
        _ => "application/octet-stream",
    };
    if code == "200 OK" && mime.starts_with("text/html") {
        body = inject_livereload(&body, accepted.generation);
        if let Some(nonce) = review_nonce {
            body = inject_review(&body, nonce);
        }
    }
    write!(
        stream,
        "HTTP/1.1 {code}\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nContent-Security-Policy: {CSP}\r\nX-Frame-Options: DENY\r\nX-Content-Type-Options: nosniff\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    if !head_only {
        stream.write_all(&body)?;
    }
    Ok(())
}

fn resolve_served_file(root: &Path, file: &Path) -> Option<PathBuf> {
    if !file.is_file() {
        return None;
    }
    let root = root.canonicalize().ok()?;
    let file = file.canonicalize().ok()?;
    if file.starts_with(&root) {
        Some(file)
    } else {
        None
    }
}

fn respond_reload(stream: &mut TcpStream, generation: u64, head_only: bool) -> anyhow::Result<()> {
    let body = generation.to_string();
    write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nContent-Security-Policy: {CSP}\r\nX-Frame-Options: DENY\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    if !head_only {
        stream.write_all(body.as_bytes())?;
    }
    Ok(())
}

fn reload_session_id() -> u64 {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64;
    (nanos ^ u64::from(std::process::id())).max(1)
}

fn inject_livereload(body: &[u8], generation: u64) -> Vec<u8> {
    let script = format!(
        r#"<script>(()=>{{let g='{generation}';async function poll(){{let d=document.hidden?3000:1000;try{{const r=await fetch('/__sideshow/reload',{{cache:'no-store'}});const n=(await r.text()).trim();if(n&&n!==g)location.reload();}}catch(_){{}}setTimeout(poll,d);}}setTimeout(poll,1000);}})();</script>"#
    );
    let Ok(html) = std::str::from_utf8(body) else {
        return body.to_vec();
    };
    if let Some(index) = html.rfind("</body>") {
        let mut injected = String::with_capacity(html.len() + script.len());
        injected.push_str(&html[..index]);
        injected.push_str(&script);
        injected.push_str(&html[index..]);
        injected.into_bytes()
    } else {
        let mut injected = Vec::with_capacity(body.len() + script.len());
        injected.extend_from_slice(body);
        injected.extend_from_slice(script.as_bytes());
        injected
    }
}

fn review_nonce() -> anyhow::Result<String> {
    let mut bytes = [0_u8; 24];
    getrandom::fill(&mut bytes)
        .map_err(|error| anyhow::anyhow!("failed to generate review session nonce: {error}"))?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn inject_review(body: &[u8], nonce: &str) -> Vec<u8> {
    let injection = format!(
        "<style id=\"sideshow-review-style\">{REVIEW_CSS}</style><script id=\"sideshow-review-script\" data-nonce=\"{nonce}\">{REVIEW_JS}</script>"
    );
    let Ok(html) = std::str::from_utf8(body) else {
        return body.to_vec();
    };
    if let Some(index) = html.rfind("</body>") {
        let mut injected = String::with_capacity(html.len() + injection.len());
        injected.push_str(&html[..index]);
        injected.push_str(&injection);
        injected.push_str(&html[index..]);
        injected.into_bytes()
    } else {
        let mut injected = Vec::with_capacity(body.len() + injection.len());
        injected.extend_from_slice(body);
        injected.extend_from_slice(injection.as_bytes());
        injected
    }
}

fn respond_review(
    stream: &mut TcpStream,
    request: &HttpRequest,
    review: &ReviewServer,
) -> anyhow::Result<()> {
    if request.headers.get("x-sideshow-review") != Some(&review.nonce) {
        return respond_json_error(stream, "403 Forbidden", "invalid review nonce");
    }
    match request.method.as_str() {
        "GET" | "HEAD" => match review.repository.load_snapshot() {
            Ok(snapshot) => respond_json_etag(
                stream,
                "200 OK",
                &snapshot,
                request.method == "HEAD",
                Some(snapshot.revision),
            ),
            Err(error) => respond_review_repository_error(stream, error),
        },
        "POST" => {
            if !request.headers.get("content-type").is_some_and(|value| {
                value.split(';').next().is_some_and(|media_type| {
                    media_type.trim().eq_ignore_ascii_case("application/json")
                })
            }) {
                return respond_json_error(
                    stream,
                    "415 Unsupported Media Type",
                    "expected application/json",
                );
            }
            if !request_is_same_origin(request) {
                return respond_json_error(stream, "403 Forbidden", "origin is not same-origin");
            }
            let mutation =
                match serde_json::from_slice::<sideshow::review::ReviewMutation>(&request.body) {
                    Ok(mutation) => mutation,
                    Err(_) => {
                        return respond_json_error(
                            stream,
                            "400 Bad Request",
                            "malformed review mutation",
                        );
                    }
                };
            let if_match = match request.headers.get("if-match") {
                None => {
                    return respond_json_error(
                        stream,
                        "428 Precondition Required",
                        "If-Match with the current quoted review revision is required",
                    );
                }
                Some(value) => match parse_revision_etag(value) {
                    Some(revision) => revision,
                    None => {
                        return respond_json_error(
                            stream,
                            "400 Bad Request",
                            "malformed If-Match review revision",
                        );
                    }
                },
            };
            if if_match != mutation.revision() {
                return respond_json_error(
                    stream,
                    "400 Bad Request",
                    "If-Match revision does not match mutation revision",
                );
            }
            match review.repository.apply_mutation(mutation) {
                Ok(artifact) => {
                    let snapshot = artifact.snapshot();
                    respond_json_etag(stream, "200 OK", &snapshot, false, Some(snapshot.revision))
                }
                Err(sideshow::review::ReviewRepositoryError::Conflict(artifact)) => {
                    let snapshot = artifact.snapshot();
                    respond_json_etag(
                        stream,
                        "409 Conflict",
                        &snapshot,
                        false,
                        Some(snapshot.revision),
                    )
                }
                Err(error) => respond_review_repository_error(stream, error),
            }
        }
        _ => respond_status(stream, "405 Method Not Allowed"),
    }
}

fn parse_revision_etag(value: &str) -> Option<u64> {
    let value = value.trim();
    if value.len() < 3 || !value.starts_with('"') || !value.ends_with('"') {
        return None;
    }
    let digits = &value[1..value.len() - 1];
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

fn respond_review_repository_error(
    stream: &mut TcpStream,
    error: sideshow::review::ReviewRepositoryError,
) -> anyhow::Result<()> {
    match error {
        sideshow::review::ReviewRepositoryError::Invalid(_) => respond_json_error(
            stream,
            "422 Unprocessable Content",
            "invalid review mutation or artifact",
        ),
        sideshow::review::ReviewRepositoryError::NotFound => {
            respond_json_error(stream, "404 Not Found", "annotation not found")
        }
        sideshow::review::ReviewRepositoryError::Conflict(artifact) => {
            let snapshot = artifact.snapshot();
            respond_json_etag(
                stream,
                "409 Conflict",
                &snapshot,
                false,
                Some(snapshot.revision),
            )
        }
        sideshow::review::ReviewRepositoryError::Io(_)
        | sideshow::review::ReviewRepositoryError::Malformed(_)
        | sideshow::review::ReviewRepositoryError::Oversized { .. }
        | sideshow::review::ReviewRepositoryError::ManifestRequired => respond_json_error(
            stream,
            "500 Internal Server Error",
            "review persistence is unavailable",
        ),
    }
}

fn request_is_same_origin(request: &HttpRequest) -> bool {
    let Some(host) = request.headers.get("host") else {
        return false;
    };
    let local_host = host == "localhost"
        || host == "127.0.0.1"
        || host
            .strip_prefix("localhost:")
            .is_some_and(|port| port.parse::<u16>().is_ok())
        || host
            .strip_prefix("127.0.0.1:")
            .is_some_and(|port| port.parse::<u16>().is_ok());
    local_host
        && request
            .headers
            .get("origin")
            .is_some_and(|origin| origin == &format!("http://{host}"))
}

fn request_has_allowed_host(request: &HttpRequest, port: u16) -> bool {
    let Some(host) = request.headers.get("host") else {
        return false;
    };
    host == &format!("localhost:{port}")
        || host == &format!("127.0.0.1:{port}")
        || (port == 80 && matches!(host.as_str(), "localhost" | "127.0.0.1"))
}

fn respond_json<T: serde::Serialize>(
    stream: &mut TcpStream,
    code: &str,
    value: &T,
    head_only: bool,
) -> anyhow::Result<()> {
    respond_json_etag(stream, code, value, head_only, None)
}

fn respond_json_etag<T: serde::Serialize>(
    stream: &mut TcpStream,
    code: &str,
    value: &T,
    head_only: bool,
    revision: Option<u64>,
) -> anyhow::Result<()> {
    let body = serde_json::to_vec(value)?;
    let etag = revision
        .map(|revision| format!("ETag: \"{revision}\"\r\n"))
        .unwrap_or_default();
    write!(
        stream,
        "HTTP/1.1 {code}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n{etag}Content-Security-Policy: {CSP}\r\nX-Frame-Options: DENY\r\nX-Content-Type-Options: nosniff\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    if !head_only {
        stream.write_all(&body)?;
    }
    Ok(())
}

fn respond_json_error(stream: &mut TcpStream, code: &str, message: &str) -> anyhow::Result<()> {
    respond_json(
        stream,
        code,
        &serde_json::json!({ "error": message }),
        false,
    )
}

fn deck_sources_mtime(dir: &Path) -> anyhow::Result<SystemTime> {
    fn consider_file(p: &Path, max: &mut SystemTime) -> anyhow::Result<()> {
        if p.is_file()
            && let Ok(m) = fs::metadata(p)?.modified()
            && m > *max
        {
            *max = m;
        }
        Ok(())
    }
    fn walk(p: &Path, max: &mut SystemTime) -> anyhow::Result<()> {
        if !p.is_dir() {
            return Ok(());
        }
        for e in fs::read_dir(p)? {
            let e = e?;
            let p = e.path();
            if p.is_dir() {
                walk(&p, max)?;
            } else {
                consider_file(&p, max)?;
            }
        }
        Ok(())
    }
    let mut max = SystemTime::UNIX_EPOCH;
    consider_file(&dir.join("deck.toml"), &mut max)?;
    consider_file(&dir.join("theme.css"), &mut max)?;
    walk(&dir.join("slides"), &mut max)?;
    walk(&dir.join("assets"), &mut max)?;
    Ok(max)
}

#[cfg(test)]
mod serve_tests {
    use super::*;
    use std::collections::BTreeSet;
    use std::sync::atomic::{AtomicU64, Ordering};

    struct FakeOpenEnv {
        os: &'static str,
        vars: BTreeMap<&'static str, &'static str>,
        executables: BTreeSet<&'static str>,
    }

    impl FakeOpenEnv {
        fn new(os: &'static str) -> Self {
            Self {
                os,
                vars: BTreeMap::new(),
                executables: BTreeSet::new(),
            }
        }

        fn with_var(mut self, name: &'static str, value: &'static str) -> Self {
            self.vars.insert(name, value);
            self
        }

        fn with_executable(mut self, program: &'static str) -> Self {
            self.executables.insert(program);
            self
        }
    }

    impl OpenEnv for FakeOpenEnv {
        fn os(&self) -> &str {
            self.os
        }

        fn var(&self, name: &str) -> Option<String> {
            self.vars.get(name).map(|value| (*value).into())
        }

        fn executable_on_path(&self, program: &str) -> bool {
            self.executables.contains(program)
        }
    }

    struct ReviewFixture {
        server: Arc<ReviewServer>,
        _deck: tempfile::TempDir,
        _state: tempfile::TempDir,
    }

    fn exchange_with_review(request: &[u8], review: Arc<ReviewServer>) -> String {
        use std::net::Shutdown;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let state = Mutex::new(AcceptedServeState {
                output: PathBuf::from("index.html"),
                html: Arc::from(b"<!doctype html><body>test</body>".as_slice()),
                generation: 1,
            });
            handle_stream(&mut stream, Path::new("."), &state, Some(&review), 8000).unwrap();
        });
        let mut client = TcpStream::connect(address).unwrap();
        client.write_all(request).unwrap();
        client.shutdown(Shutdown::Write).unwrap();
        let mut response = String::new();
        client.read_to_string(&mut response).unwrap();
        server.join().unwrap();
        response
    }

    fn review_fixture() -> ReviewFixture {
        let deck = tempfile::tempdir().unwrap();
        fs::write(deck.path().join("deck.toml"), "[deck]\ntitle='Test'\n").unwrap();
        let state = tempfile::tempdir().unwrap();
        let repository =
            sideshow::review::ReviewRepository::with_state_root(deck.path(), state.path()).unwrap();
        ReviewFixture {
            server: Arc::new(ReviewServer {
                nonce: "test-nonce".into(),
                repository,
            }),
            _deck: deck,
            _state: state,
        }
    }

    #[test]
    fn serve_open_flag_is_opt_in_and_parseable_with_review() {
        let cli = Cli::try_parse_from(["sideshow", "serve", "deck"]).unwrap();
        let Command::Serve { open, review, .. } = cli.command else {
            panic!("expected serve command");
        };
        assert!(!open);
        assert!(!review);

        let cli = Cli::try_parse_from(["sideshow", "serve", "deck", "--review", "--open"]).unwrap();
        let Command::Serve { open, review, .. } = cli.command else {
            panic!("expected serve command");
        };
        assert!(open);
        assert!(review);
    }

    #[test]
    fn serve_url_uses_actual_bound_port_including_port_zero() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();

        assert_eq!(
            serve_url(&listener).unwrap(),
            format!("http://127.0.0.1:{port}/")
        );
    }

    #[test]
    fn http_accept_loop_is_ready_before_opener_phase_and_stops_cleanly() {
        let deck = tempfile::tempdir().unwrap();
        let dist = deck.path().join("dist");
        fs::create_dir(&dist).unwrap();
        let output = dist.join("deck.html");
        fs::write(&output, "<!doctype html><body>ready</body>").unwrap();
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let address = listener.local_addr().unwrap();
        let server = start_http_server(
            listener,
            dist,
            Arc::new(Mutex::new(AcceptedServeState {
                output,
                html: Arc::from(b"<!doctype html><body>ready</body>".as_slice()),
                generation: 1,
            })),
            None,
            address.port(),
        )
        .unwrap();

        let mut client = TcpStream::connect(address).unwrap();
        write!(
            client,
            "GET / HTTP/1.1\r\nHost: 127.0.0.1:{}\r\n\r\n",
            address.port()
        )
        .unwrap();
        let mut response = String::new();
        client.read_to_string(&mut response).unwrap();
        assert!(response.contains("ready"), "{response}");

        // The accepted bytes and generation are one immutable snapshot. Replacing the published
        // pathname behind the server must not expose unaccepted bytes under generation 1.
        fs::write(deck.path().join("dist/deck.html"), "unaccepted replacement").unwrap();
        let mut client = TcpStream::connect(address).unwrap();
        write!(
            client,
            "GET / HTTP/1.1\r\nHost: 127.0.0.1:{}\r\n\r\n",
            address.port()
        )
        .unwrap();
        let mut response = String::new();
        client.read_to_string(&mut response).unwrap();
        assert!(response.contains("ready"), "{response}");
        assert!(!response.contains("unaccepted replacement"), "{response}");
        assert!(response.contains("let g='1'"), "{response}");

        drop(server);
        TcpListener::bind(address).expect("listener must be released after startup failure");
    }

    #[cfg(unix)]
    #[test]
    fn accepted_filename_route_uses_immutable_bytes_through_symlinked_dist() {
        use std::os::unix::fs::symlink;

        let deck = tempfile::tempdir().unwrap();
        let real_dist = deck.path().join("real-dist");
        fs::create_dir(&real_dist).unwrap();
        symlink(&real_dist, deck.path().join("dist")).unwrap();
        let output = real_dist.join("deck.html");
        fs::write(&output, "accepted on disk").unwrap();
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let address = listener.local_addr().unwrap();
        let server = start_http_server(
            listener,
            deck.path().join("dist"),
            Arc::new(Mutex::new(AcceptedServeState {
                output: output.canonicalize().unwrap(),
                html: Arc::from(b"immutable accepted bytes".as_slice()),
                generation: 7,
            })),
            None,
            address.port(),
        )
        .unwrap();
        fs::write(&output, "unaccepted replacement").unwrap();

        let mut client = TcpStream::connect(address).unwrap();
        write!(
            client,
            "GET /deck.html HTTP/1.1\r\nHost: 127.0.0.1:{}\r\n\r\n",
            address.port()
        )
        .unwrap();
        let mut response = String::new();
        client.read_to_string(&mut response).unwrap();

        assert!(response.contains("immutable accepted bytes"), "{response}");
        assert!(!response.contains("unaccepted replacement"), "{response}");
        assert!(response.contains("let g='7'"), "{response}");
        drop(server);
    }

    #[test]
    fn opener_resolution_supports_macos_and_linux_desktops() {
        assert_eq!(
            resolve_opener(&FakeOpenEnv::new("macos")).unwrap(),
            OpenerCommand {
                program: "open".into(),
                args: Vec::new(),
            }
        );
        assert_eq!(
            resolve_opener(
                &FakeOpenEnv::new("linux")
                    .with_var("WAYLAND_DISPLAY", "wayland-0")
                    .with_executable("gio")
            )
            .unwrap(),
            OpenerCommand {
                program: "gio".into(),
                args: vec!["open".into()],
            }
        );
    }

    #[test]
    fn opener_resolution_reports_headless_and_unsupported_cases() {
        let headless = resolve_opener(&FakeOpenEnv::new("linux"))
            .unwrap_err()
            .to_string();
        assert!(headless.contains("graphical Linux session"));
        assert!(headless.contains("DISPLAY"));

        let missing = resolve_opener(&FakeOpenEnv::new("linux").with_var("DISPLAY", ":1"))
            .unwrap_err()
            .to_string();
        assert!(missing.contains("could not find a desktop opener"));
        assert!(missing.contains("xdg-open"));

        let unsupported = resolve_opener(&FakeOpenEnv::new("windows"))
            .unwrap_err()
            .to_string();
        assert!(unsupported.contains("unsupported on windows"));
    }

    #[test]
    fn opener_spawn_failure_is_reported_without_a_gui_process() {
        let opener = OpenerCommand {
            program: "/definitely/missing/sideshow-opener".into(),
            args: Vec::new(),
        };

        let error = run_opener(&opener, "http://127.0.0.1:1/")
            .unwrap_err()
            .to_string();

        assert!(error.contains("could not run opener"), "{error}");
    }

    #[cfg(unix)]
    #[test]
    fn opener_nonzero_exit_is_reported_without_a_gui_process() {
        let opener = OpenerCommand {
            program: "/bin/sh".into(),
            args: vec!["-c".into(), "exit 23".into()],
        };

        let error = run_opener(&opener, "http://127.0.0.1:1/")
            .unwrap_err()
            .to_string();

        assert!(error.contains("exited with"), "{error}");
        assert!(error.contains("23"), "{error}");
    }

    #[test]
    fn rebuild_watcher_guard_stops_and_joins_worker() {
        let (shutdown, rx) = mpsc::channel();
        let stopped = Arc::new(AtomicU64::new(0));
        let worker_stopped = Arc::clone(&stopped);
        let worker = thread::spawn(move || {
            rx.recv().unwrap();
            worker_stopped.store(1, Ordering::SeqCst);
        });
        drop(RebuildWatcher {
            shutdown,
            worker: Some(worker),
        });
        assert_eq!(stopped.load(Ordering::SeqCst), 1);
    }

    fn staged_build_fixture() -> tempfile::TempDir {
        let deck = tempfile::tempdir().unwrap();
        fs::create_dir(deck.path().join("slides")).unwrap();
        fs::create_dir(deck.path().join("assets")).unwrap();
        fs::create_dir(deck.path().join("dist")).unwrap();
        fs::write(deck.path().join("deck.toml"), "[deck]\ntitle='Stable'\n").unwrap();
        fs::write(deck.path().join("theme.css"), "body {}\n").unwrap();
        fs::write(deck.path().join("slides/01.html"), "<h1>old</h1>\n").unwrap();
        fs::write(deck.path().join("dist/stable.html"), "accepted old\n").unwrap();
        deck
    }

    #[test]
    fn stable_build_does_not_replace_same_path_until_publish() {
        let deck = staged_build_fixture();
        let built = stable_build_deck_with(deck.path(), false, |_root, staging| {
            let output = staging.join("stable.html");
            fs::write(&output, "candidate new\n")?;
            Ok(output)
        })
        .unwrap();

        assert_eq!(
            fs::read_to_string(deck.path().join("dist/stable.html")).unwrap(),
            "accepted old\n"
        );
        let accepted = built.publish().unwrap();
        assert_eq!(
            accepted.output,
            deck.path().canonicalize().unwrap().join("dist/stable.html")
        );
        assert_eq!(
            fs::read_to_string(deck.path().join("dist/stable.html")).unwrap(),
            "candidate new\n"
        );
    }

    #[test]
    fn accepted_candidate_is_revalidated_before_publication() {
        let deck = staged_build_fixture();
        let built = stable_build_deck_with(deck.path(), false, |_root, staging| {
            let output = staging.join("stable.html");
            fs::write(&output, "obsolete candidate\n")?;
            Ok(output)
        })
        .unwrap();
        fs::write(deck.path().join("theme.css"), "body { color: newer; }\n").unwrap();

        let error = format!("{:#}", built.publish().unwrap_err());

        assert!(error.contains("inputs changed"), "{error}");
        assert_eq!(
            fs::read_to_string(deck.path().join("dist/stable.html")).unwrap(),
            "accepted old\n"
        );
    }

    #[test]
    fn exhausted_input_race_preserves_last_accepted_output() {
        let deck = staged_build_fixture();
        let mut attempt = 0;
        let error = stable_build_deck_with(deck.path(), false, |root, staging| {
            attempt += 1;
            let output = staging.join("stable.html");
            fs::write(&output, format!("unstable candidate {attempt}\n"))?;
            fs::write(
                root.join("theme.css"),
                format!("body {{ order: {attempt}; }}\n"),
            )?;
            Ok(output)
        })
        .unwrap_err()
        .to_string();

        assert!(error.contains("kept changing"), "{error}");
        assert_eq!(attempt, MAX_STABLE_BUILD_ATTEMPTS);
        assert_eq!(
            fs::read_to_string(deck.path().join("dist/stable.html")).unwrap(),
            "accepted old\n"
        );
        assert!(fs::read_dir(deck.path()).unwrap().all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".sideshow-stage-")
        }));
    }

    #[test]
    fn review_manifest_failure_rejects_candidate_deck() {
        let deck = staged_build_fixture();
        let state = tempfile::tempdir().unwrap();
        let repository =
            sideshow::review::ReviewRepository::with_state_root(deck.path(), state.path()).unwrap();
        repository.load_artifact().unwrap();
        fs::write(repository.artifact_path(), b"not valid JSON\n").unwrap();

        let staging_dir = create_build_staging_dir(deck.path()).unwrap();
        let staged_output = staging_dir.join("stable.html");
        fs::write(&staged_output, "candidate with unsynchronized identity\n").unwrap();
        let built = StableBuild {
            deck_root: deck.path().canonicalize().unwrap(),
            staged_output,
            output: deck.path().join("dist/stable.html"),
            staging_dir: staging_dir.clone(),
            manifest: Some(sideshow::review::ReviewBuildManifest {
                build_id: "candidate-build".into(),
                built_at_ms: 1,
                slides: vec![sideshow::review::ReviewSlideManifest {
                    slide_id: "s-01".into(),
                    source_path: "slides/01.html".into(),
                    source_digest: "candidate-digest".into(),
                }],
                verification_commands: Vec::new(),
            }),
            input_digest: "accepted-input".into(),
            output_bytes: Arc::from(b"candidate with unsynchronized identity\n".as_slice()),
        };

        let error = accept_stable_build(built, Some(&repository))
            .unwrap_err()
            .to_string();

        assert!(
            error.contains("not synchronized") || error.contains("synchronize"),
            "{error}"
        );
        assert_eq!(
            fs::read_to_string(deck.path().join("dist/stable.html")).unwrap(),
            "accepted old\n"
        );
        assert!(!staging_dir.exists());
    }

    #[test]
    fn inject_livereload_places_script_before_body_close() {
        let html = b"<!doctype html><body><h1>Hi</h1></body>";

        let injected = String::from_utf8(inject_livereload(html, 7)).unwrap();

        assert!(injected.contains("let g='7'"));
        assert!(injected.contains("/__sideshow/reload"));
        assert!(injected.contains("async function poll()"));
        assert!(injected.contains("document.hidden?3000:1000"));
        assert!(injected.contains("setTimeout(poll,d)"));
        assert!(injected.contains("setTimeout(poll,1000)"));
        assert!(injected.contains("n!==g"));
        assert!(!injected.contains("setInterval"));
        assert!(!injected.contains("n>g"));
        assert!(injected.contains("</script></body>"));
    }

    #[test]
    fn inject_livereload_appends_when_body_close_is_missing() {
        let html = b"<!doctype html><h1>Hi</h1>";

        let injected = String::from_utf8(inject_livereload(html, 1)).unwrap();

        assert!(injected.starts_with("<!doctype html><h1>Hi</h1>"));
        assert!(injected.ends_with("</script>"));
    }

    #[test]
    fn inject_livereload_leaves_non_utf8_unchanged() {
        let body = b"\xff\xfe";

        assert_eq!(inject_livereload(body, 1), body);
    }

    #[test]
    fn review_injection_is_served_only_and_carries_nonce() {
        let html = b"<!doctype html><body><h1>Hi</h1></body>";

        let injected = String::from_utf8(inject_review(html, "deadbeef")).unwrap();

        assert!(injected.contains("id=\"sideshow-review-style\""));
        assert!(injected.contains("id=\"sideshow-review-script\""));
        assert!(injected.contains("data-nonce=\"deadbeef\""));
        assert!(injected.contains("/__sideshow/review"));
        assert!(injected.contains("</script></body>"));
        assert!(!String::from_utf8_lossy(html).contains("sideshow-review"));
    }

    #[test]
    fn review_manifest_uses_exact_output_and_global_slide_inputs() {
        let deck = tempfile::tempdir().unwrap();
        fs::create_dir(deck.path().join("slides")).unwrap();
        fs::create_dir(deck.path().join("assets")).unwrap();
        fs::create_dir(deck.path().join("dist")).unwrap();
        fs::write(deck.path().join("deck.toml"), "[deck]\ntitle='Hash'\n").unwrap();
        fs::write(deck.path().join("theme.css"), "body { color: red; }").unwrap();
        fs::write(deck.path().join("slides/01.html"), "<h1>One</h1>").unwrap();
        fs::write(deck.path().join("assets/picture.bin"), b"asset one").unwrap();
        let output = deck.path().join("dist/hash.html");
        let output_bytes = b"exact built bytes\0\xff";
        fs::write(&output, output_bytes).unwrap();

        let first = review_build_manifest(deck.path(), &output).unwrap();
        assert_eq!(first.build_id, sha256_hex(output_bytes));
        assert_eq!(first.slides.len(), 1);
        assert_eq!(first.slides[0].slide_id, "s-01");
        assert_eq!(first.slides[0].source_path, "slides/01.html");

        fs::write(deck.path().join("theme.css"), "body { color: blue; }").unwrap();
        let changed_global = review_build_manifest(deck.path(), &output).unwrap();
        assert_eq!(changed_global.build_id, first.build_id);
        assert_ne!(
            changed_global.slides[0].source_digest,
            first.slides[0].source_digest
        );

        fs::write(deck.path().join("assets/picture.bin"), b"asset two").unwrap();
        let changed_asset = review_build_manifest(deck.path(), &output).unwrap();
        assert_ne!(
            changed_asset.slides[0].source_digest, changed_global.slides[0].source_digest,
            "asset-only changes must conservatively stale slide annotations"
        );
    }

    #[test]
    fn deck_input_digest_is_sorted_and_content_sensitive() {
        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        for deck in [first.path(), second.path()] {
            fs::create_dir(deck.join("slides")).unwrap();
            fs::create_dir_all(deck.join("assets/nested")).unwrap();
            fs::create_dir(deck.join("content")).unwrap();
            fs::write(
                deck.join("deck.toml"),
                "[deck]\ntitle='Stable'\nslides=['content/custom.html']\n",
            )
            .unwrap();
            fs::write(deck.join("theme.css"), "body {}").unwrap();
        }
        // Deliberately create the same trees in opposite orders.
        fs::write(first.path().join("slides/02.html"), "two").unwrap();
        fs::write(first.path().join("slides/01.html"), "one").unwrap();
        fs::write(first.path().join("assets/nested/b.bin"), b"b").unwrap();
        fs::write(first.path().join("assets/a.bin"), b"a").unwrap();
        fs::write(first.path().join("content/custom.html"), "custom").unwrap();
        fs::write(second.path().join("content/custom.html"), "custom").unwrap();
        fs::write(second.path().join("assets/a.bin"), b"a").unwrap();
        fs::write(second.path().join("assets/nested/b.bin"), b"b").unwrap();
        fs::write(second.path().join("slides/01.html"), "one").unwrap();
        fs::write(second.path().join("slides/02.html"), "two").unwrap();

        let digest = deck_input_digest(first.path()).unwrap();
        assert_eq!(digest, deck_input_digest(second.path()).unwrap());
        fs::write(second.path().join("content/custom.html"), b"changed").unwrap();
        assert_ne!(digest, deck_input_digest(second.path()).unwrap());
    }

    #[test]
    fn review_mutations_require_exact_loopback_origin() {
        let request = |host: &str, origin: &str| HttpRequest {
            method: "POST".into(),
            path: "__sideshow/review".into(),
            headers: BTreeMap::from([
                ("host".into(), host.into()),
                ("origin".into(), origin.into()),
            ]),
            body: Vec::new(),
        };

        assert!(request_is_same_origin(&request(
            "localhost:8000",
            "http://localhost:8000"
        )));
        assert!(request_is_same_origin(&request(
            "127.0.0.1:9976",
            "http://127.0.0.1:9976"
        )));
        assert!(!request_is_same_origin(&request(
            "localhost:8000",
            "https://evil.example"
        )));
        assert!(!request_is_same_origin(&request(
            "evil.example",
            "http://evil.example"
        )));
    }

    #[test]
    fn served_routes_reject_unexpected_host_authorities() {
        let request = b"GET / HTTP/1.1\r\nHost: attacker.example:8000\r\n\r\n";
        let fixture = review_fixture();

        let response = exchange_with_review(request, Arc::clone(&fixture.server));

        assert!(response.starts_with("HTTP/1.1 421 Misdirected Request"));
        assert!(response.contains("\r\nX-Frame-Options: DENY\r\n"));
        assert!(response.contains("frame-ancestors 'none'"));
    }

    #[test]
    fn review_http_api_checks_nonce_origin_and_revision() {
        let fixture = review_fixture();
        let review = &fixture.server;
        let body = serde_json::json!({
            "operation": "create",
            "revision": 0,
            "annotation": {
                "slide_id": "s-01-title",
                "source_path": "slides/01-title.html",
                "target": { "type": "point", "x": 100, "y": 200 },
                "body": "Tighten the title",
                "kind": "issue",
                "action": "fix"
            }
        })
        .to_string();
        let request = format!(
            "POST /__sideshow/review HTTP/1.1\r\nHost: localhost:8000\r\nOrigin: http://localhost:8000\r\nContent-Type: application/json; charset=utf-8\r\nIf-Match: \"0\"\r\nX-Sideshow-Review: test-nonce\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );

        let response = exchange_with_review(request.as_bytes(), Arc::clone(review));

        assert!(response.starts_with("HTTP/1.1 200 OK"));
        assert!(response.contains("\r\nETag: \"1\"\r\n"));
        let snapshot = review.repository.load_snapshot().unwrap();
        assert_eq!(snapshot.revision, 1);
        assert_eq!(snapshot.annotations[0].body, "Tighten the title");

        let bad_nonce = request.replace("test-nonce", "wrong-nonce");
        let response = exchange_with_review(bad_nonce.as_bytes(), Arc::clone(review));
        assert!(response.starts_with("HTTP/1.1 403 Forbidden"));

        let bad_origin = request.replace("http://localhost:8000", "https://evil.example");
        let response = exchange_with_review(bad_origin.as_bytes(), Arc::clone(review));
        assert!(response.starts_with("HTTP/1.1 403 Forbidden"));
    }

    #[test]
    fn review_http_create_rejects_target_outside_active_manifest() {
        let fixture = review_fixture();
        let repository = &fixture.server.repository;
        let built = repository
            .update_build_manifest(
                0,
                sideshow::review::ReviewBuildManifest {
                    build_id: "active-build".into(),
                    built_at_ms: 1,
                    slides: vec![sideshow::review::ReviewSlideManifest {
                        slide_id: "s-01-title".into(),
                        source_path: "slides/01-title.html".into(),
                        source_digest: "digest".into(),
                    }],
                    verification_commands: vec![],
                },
            )
            .unwrap();
        let body = serde_json::json!({
            "operation": "create",
            "revision": built.revision,
            "annotation": {
                "slide_id": "s-01-title",
                "source_path": "slides/moved-title.html",
                "target": { "type": "point", "x": 10, "y": 20 },
                "body": "invalid target",
                "kind": "issue",
                "action": "fix"
            }
        })
        .to_string();
        let request = format!(
            "POST /__sideshow/review HTTP/1.1\r\nHost: localhost:8000\r\nOrigin: http://localhost:8000\r\nContent-Type: application/json\r\nIf-Match: \"{}\"\r\nX-Sideshow-Review: test-nonce\r\nContent-Length: {}\r\n\r\n{body}",
            built.revision,
            body.len()
        );

        let response = exchange_with_review(request.as_bytes(), Arc::clone(&fixture.server));
        assert!(
            response.starts_with("HTTP/1.1 422 Unprocessable Content"),
            "{response}"
        );
        let after = repository.load_artifact().unwrap();
        assert_eq!(after.revision, built.revision);
        assert!(after.annotations.is_empty());
    }

    #[test]
    fn review_http_get_and_head_return_persistent_revision_etags() {
        let fixture = review_fixture();
        let get = b"GET /__sideshow/review HTTP/1.1\r\nHost: localhost:8000\r\nX-Sideshow-Review: test-nonce\r\n\r\n";
        let response = exchange_with_review(get, Arc::clone(&fixture.server));
        assert!(response.starts_with("HTTP/1.1 200 OK"));
        assert!(response.contains("\r\nETag: \"0\"\r\n"));
        assert!(response.contains("\r\nX-Frame-Options: DENY\r\n"));
        assert!(response.contains("frame-ancestors 'none'"));
        assert!(response.contains("\"schema_version\":2"));

        let head = b"HEAD /__sideshow/review HTTP/1.1\r\nHost: localhost:8000\r\nX-Sideshow-Review: test-nonce\r\n\r\n";
        let response = exchange_with_review(head, Arc::clone(&fixture.server));
        assert!(response.starts_with("HTTP/1.1 200 OK"));
        assert!(response.contains("\r\nETag: \"0\"\r\n"));
        assert!(response.ends_with("\r\n\r\n"));
    }

    #[test]
    fn review_http_requires_well_formed_matching_if_match() {
        let fixture = review_fixture();
        let body = r#"{"operation":"delete","revision":0,"id":"missing"}"#;
        let request = |if_match: Option<&str>| {
            let header = if_match
                .map(|value| format!("If-Match: {value}\r\n"))
                .unwrap_or_default();
            format!(
                "POST /__sideshow/review HTTP/1.1\r\nHost: localhost:8000\r\nOrigin: http://localhost:8000\r\nContent-Type: application/json\r\nX-Sideshow-Review: test-nonce\r\n{header}Content-Length: {}\r\n\r\n{body}",
                body.len()
            )
        };
        let missing = exchange_with_review(request(None).as_bytes(), Arc::clone(&fixture.server));
        assert!(missing.starts_with("HTTP/1.1 428 Precondition Required"));
        let malformed =
            exchange_with_review(request(Some("0")).as_bytes(), Arc::clone(&fixture.server));
        assert!(malformed.starts_with("HTTP/1.1 400 Bad Request"));
        let mismatch = exchange_with_review(
            request(Some("\"1\"")).as_bytes(),
            Arc::clone(&fixture.server),
        );
        assert!(mismatch.starts_with("HTTP/1.1 400 Bad Request"));
        assert_eq!(
            fixture.server.repository.load_snapshot().unwrap().revision,
            0
        );
    }

    #[test]
    fn review_http_cross_repository_stale_write_returns_latest_snapshot() {
        let fixture = review_fixture();
        let other = sideshow::review::ReviewRepository::with_state_root(
            fixture.server.repository.deck_root(),
            fixture._state.path(),
        )
        .unwrap();
        let created = other
            .apply_mutation(sideshow::review::ReviewMutation::Create {
                revision: 0,
                annotation: sideshow::review::NewReviewAnnotation {
                    slide_id: "s-01-title".into(),
                    source_path: "slides/01-title.html".into(),
                    target: sideshow::review::ReviewTarget::Point {
                        x: 1.0,
                        y: 2.0,
                        selector_hint: None,
                        text_hint: None,
                    },
                    body: "other process".into(),
                    kind: sideshow::review::ReviewKind::Note,
                    action: None,
                },
            })
            .unwrap();
        assert_eq!(created.revision, 1);
        let body = r#"{"operation":"delete","revision":0,"id":"missing"}"#;
        let request = format!(
            "POST /__sideshow/review HTTP/1.1\r\nHost: localhost:8000\r\nOrigin: http://localhost:8000\r\nContent-Type: application/json\r\nIf-Match: \"0\"\r\nX-Sideshow-Review: test-nonce\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );
        let response = exchange_with_review(request.as_bytes(), Arc::clone(&fixture.server));
        assert!(response.starts_with("HTTP/1.1 409 Conflict"));
        assert!(response.contains("\r\nETag: \"1\"\r\n"));
        assert!(response.contains("other process"));
        assert_eq!(
            fixture
                .server
                .repository
                .load_snapshot()
                .unwrap()
                .annotations
                .len(),
            1
        );
    }

    #[test]
    fn review_http_api_rejects_oversized_bodies_before_reading_them() {
        let request = format!(
            "POST /__sideshow/review HTTP/1.1\r\nHost: localhost:8000\r\nContent-Length: {}\r\n\r\n",
            MAX_HTTP_BODY_BYTES + 1
        );

        let fixture = review_fixture();
        let response = exchange_with_review(request.as_bytes(), Arc::clone(&fixture.server));

        assert!(response.starts_with("HTTP/1.1 413 Payload Too Large"));
    }

    #[test]
    fn event_relevance_accepts_build_inputs_and_ignores_dist_and_noise() {
        let deck = Path::new("/deck");
        let dist = deck.join("dist");
        let relevant = notify::Event::new(EventKind::Modify(notify::event::ModifyKind::Data(
            notify::event::DataChange::Content,
        )))
        .add_path(deck.join("slides/one.md"));
        assert!(event_is_relevant(Ok(relevant), deck, &dist));

        for input in ["deck.toml", "theme.css", "assets/logo.svg"] {
            let event = notify::Event::new(EventKind::Create(notify::event::CreateKind::File))
                .add_path(deck.join(input));
            assert!(event_is_relevant(Ok(event), deck, &dist), "{input}");
        }

        let dist_event = notify::Event::new(EventKind::Modify(notify::event::ModifyKind::Data(
            notify::event::DataChange::Content,
        )))
        .add_path(deck.join("dist/deck.html"));
        assert!(!event_is_relevant(Ok(dist_event), deck, &dist));

        let notes_event = notify::Event::new(EventKind::Modify(notify::event::ModifyKind::Data(
            notify::event::DataChange::Content,
        )))
        .add_path(deck.join("notes/talk.md"));
        assert!(!event_is_relevant(Ok(notes_event), deck, &dist));

        let metadata_event = notify::Event::new(EventKind::Modify(
            notify::event::ModifyKind::Metadata(notify::event::MetadataKind::Any),
        ))
        .add_path(deck.join("slides/one.md"));
        assert!(!event_is_relevant(Ok(metadata_event), deck, &dist));

        let access_event = notify::Event::new(EventKind::Access(notify::event::AccessKind::Open(
            notify::event::AccessMode::Any,
        )))
        .add_path(deck.join("slides/one.md"));
        assert!(!event_is_relevant(Ok(access_event), deck, &dist));

        let any_modify_event =
            notify::Event::new(EventKind::Modify(notify::event::ModifyKind::Any))
                .add_path(deck.join("slides/one.md"));
        assert!(event_is_relevant(Ok(any_modify_event), deck, &dist));
    }

    #[test]
    fn normalized_serve_paths_make_relative_deck_match_absolute_events() {
        let cwd = std::env::current_dir().unwrap();
        let tmp = tempfile::Builder::new()
            .prefix("sideshow-serve-normalize-")
            .tempdir_in(&cwd)
            .unwrap();
        let deck_name = tmp.path().file_name().unwrap();
        let relative_deck = PathBuf::from(deck_name);
        let dist = tmp.path().join("dist");
        fs::create_dir(&dist).unwrap();
        let output = dist.join("deck.html");
        fs::write(&output, "deck").unwrap();

        let (deck_dir, dist_dir, current_output) =
            normalized_serve_paths(&relative_deck, relative_deck.join("dist/deck.html")).unwrap();

        assert!(deck_dir.is_absolute());
        assert_eq!(deck_dir, tmp.path().canonicalize().unwrap());
        assert_eq!(dist_dir, deck_dir.join("dist"));
        assert_eq!(current_output, output.canonicalize().unwrap());

        let event = notify::Event::new(EventKind::Modify(notify::event::ModifyKind::Any))
            .add_path(deck_dir.join("slides/one.md"));
        assert!(event_is_relevant(Ok(event), &deck_dir, &dist_dir));
    }

    #[test]
    fn rescan_is_relevant_even_without_paths() {
        let event = notify::Event::new(EventKind::Any).set_flag(notify::event::Flag::Rescan);
        assert!(event_is_relevant(
            Ok(event),
            Path::new("/deck"),
            Path::new("/deck/dist")
        ));
    }

    #[test]
    fn build_input_path_matrix_excludes_tapes() {
        let deck = Path::new("/deck");
        let dist = deck.join("dist");
        for good in ["deck.toml", "theme.css", "slides/a.md", "assets/a.png"] {
            assert!(
                build_input_path_is_relevant(&deck.join(good), deck, &dist),
                "{good}"
            );
        }
        for bad in [
            "dist/out.html",
            "tapes/demo.yaml",
            "README.md",
            "src/main.rs",
            "slides-old/a.md",
            "assets-backup/a.png",
        ] {
            assert!(
                !build_input_path_is_relevant(&deck.join(bad), deck, &dist),
                "{bad}"
            );
        }
    }

    #[test]
    fn safe_request_path_rejects_traversal_absolute_and_encoded_traversal() {
        assert_eq!(
            safe_request_path("/slides/a.html").as_deref(),
            Some("slides/a.html")
        );
        assert_eq!(safe_request_path("/").as_deref(), Some(""));
        for bad in [
            "slides/a.html",
            "//evil",
            "/../secret",
            "/slides/../secret",
            "/%2e%2e/secret",
            "/slides/%2E%2E/secret",
            "/%ZZ",
        ] {
            assert!(safe_request_path(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn parse_request_line_limits_request_target_and_methods_are_visible() {
        let req =
            parse_request_line("HEAD /__sideshow/reload?x=1 HTTP/1.1\r\nHost: x\r\n").unwrap();
        assert_eq!(req.method, "HEAD");
        assert_eq!(req.path, "__sideshow/reload");
        assert!(parse_request_line("GET /%2e%2e/secret HTTP/1.1\r\n").is_none());
    }

    #[test]
    fn resolved_served_file_rejects_symlink_escape() {
        let tmp = tempfile::tempdir().unwrap();
        let dist = tmp.path().join("dist");
        fs::create_dir(&dist).unwrap();
        let inside = dist.join("inside.html");
        let outside = tmp.path().join("outside.html");
        fs::write(&inside, "inside").unwrap();
        fs::write(&outside, "outside").unwrap();

        assert!(resolve_served_file(&dist, &inside).is_some());

        #[cfg(unix)]
        {
            let link = dist.join("escape.html");
            std::os::unix::fs::symlink(&outside, &link).unwrap();
            assert!(resolve_served_file(&dist, &link).is_none());
        }
    }
}

#[cfg(test)]
mod publish_srht_tests {
    use super::*;
    use flate2::read::GzDecoder;

    #[test]
    fn token_first_field_extracts_first_whitespace_field() {
        assert_eq!(first_token_field("  tok extra\n"), Some("tok".into()));
        assert_eq!(first_token_field(" \t\n"), None);
    }

    #[test]
    fn subdir_validation_matrix() {
        for good in ["deck", "decks/foo", "a..b"] {
            validate_srht_subdir(good).unwrap();
        }
        for bad in ["", "/deck", "deck/", "../evil", "a/../b", "two words"] {
            assert!(validate_srht_subdir(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn tarball_roundtrip_has_single_deterministic_index() {
        let gz = srht_site_tar_gz(b"hello").unwrap();
        let mut archive = tar::Archive::new(GzDecoder::new(Cursor::new(gz)));
        let mut entries = archive.entries().unwrap();
        let mut entry = entries.next().unwrap().unwrap();
        assert_eq!(entry.path().unwrap().to_string_lossy(), "index.html");
        assert_eq!(entry.header().mode().unwrap(), 0o644);
        let mut body = String::new();
        entry.read_to_string(&mut body).unwrap();
        assert_eq!(body, "hello");
        drop(entry);
        assert!(entries.next().is_none());
    }
}
