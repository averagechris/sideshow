use anyhow::Context;
use clap::{Parser, Subcommand, ValueEnum};
use flate2::{Compression, write::GzEncoder};
use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use sideshow::find_tool;
use std::{
    fs,
    io::{Cursor, Read, Write},
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
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
    /// Build and serve dist/ over localhost.
    Serve {
        dir: PathBuf,
        #[arg(long, default_value_t = 8000)]
        port: u16,
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
            let output = sideshow::build_deck(&dir)?;
            println!("built {}", output.display());
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
        Command::Serve { dir, port } => serve(&dir, port),
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

const CSP: &str = "default-src 'self' data: blob:; script-src 'self' 'unsafe-eval' 'unsafe-inline'; style-src 'self' 'unsafe-inline'; worker-src 'self' 'unsafe-eval' 'unsafe-inline' data: blob:; frame-src https:; img-src data: https:; media-src https:; object-src 'none'; sandbox allow-downloads allow-forms allow-modals allow-pointer-lock allow-popups allow-presentation allow-same-origin allow-scripts;";

fn serve(dir: &Path, port: u16) -> anyhow::Result<()> {
    let out = sideshow::build_deck(dir)?;
    let (dir, root, out) = normalized_serve_paths(dir, out)?;
    let generation = Arc::new(AtomicU64::new(reload_session_id() << 32));
    let current_output = Arc::new(Mutex::new(out));
    start_rebuild_watcher(
        dir.to_path_buf(),
        root.clone(),
        Arc::clone(&generation),
        Arc::clone(&current_output),
    )?;
    let listener = TcpListener::bind(("127.0.0.1", port))?;
    println!("serving {} at http://localhost:{port}/", root.display());
    for stream in listener.incoming() {
        let root = root.clone();
        let generation = Arc::clone(&generation);
        let current_output = Arc::clone(&current_output);
        thread::spawn(move || {
            if let Ok(mut stream) = stream {
                let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
                let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));
                let _ = handle_stream(&mut stream, &root, &current_output, &generation);
            }
        });
    }
    Ok(())
}

fn start_rebuild_watcher(
    dir: PathBuf,
    dist: PathBuf,
    generation: Arc<AtomicU64>,
    current_output: Arc<Mutex<PathBuf>>,
) -> anyhow::Result<()> {
    let (tx, rx) = mpsc::channel::<notify::Result<Event>>();
    let mut watcher = RecommendedWatcher::new(
        move |res| {
            let _ = tx.send(res);
        },
        Config::default(),
    )?;
    watcher.watch(&dir, RecursiveMode::Recursive)?;
    thread::spawn(move || {
        let _watcher = watcher;
        while let Ok(res) = rx.recv() {
            let deadline = std::time::Instant::now() + Duration::from_secs(2);
            let mut should_rebuild = event_is_relevant(res, &dir, &dist);
            while let Some(remaining) = deadline.checked_duration_since(std::time::Instant::now()) {
                let timeout = remaining.min(Duration::from_millis(250));
                let Ok(res) = rx.recv_timeout(timeout) else {
                    break;
                };
                should_rebuild |= event_is_relevant(res, &dir, &dist);
            }
            if should_rebuild {
                eprintln!("change detected; rebuilding deck...");
                match sideshow::build_deck(&dir) {
                    Ok(path) => {
                        if let Ok(mut current) = current_output.lock() {
                            *current = path.clone();
                        }
                        generation.fetch_add(1, Ordering::Relaxed);
                        eprintln!("rebuilt {}", path.display());
                    }
                    Err(err) => eprintln!("rebuild failed: {err:#}"),
                }
            }
        }
    });
    Ok(())
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

fn read_req(stream: &mut TcpStream) -> anyhow::Result<String> {
    let mut b = [0; 1024];
    let n = stream.read(&mut b)?;
    Ok(String::from_utf8_lossy(&b[..n]).into())
}

fn handle_stream(
    stream: &mut TcpStream,
    root: &Path,
    current_output: &Mutex<PathBuf>,
    generation: &AtomicU64,
) -> anyhow::Result<()> {
    let req = read_req(stream)?;
    let Some(request) = parse_request_line(&req) else {
        return respond_status(stream, "400 Bad Request");
    };
    if request.method != "GET" && request.method != "HEAD" {
        return respond_status(stream, "405 Method Not Allowed");
    }
    if request.path == "__sideshow/reload" {
        return respond_reload(
            stream,
            generation.load(Ordering::Relaxed),
            request.method == "HEAD",
        );
    }
    let default = current_output
        .lock()
        .map(|p| p.clone())
        .unwrap_or_else(|_| root.join("index.html"));
    respond(
        stream,
        root,
        &request.path,
        &default,
        generation.load(Ordering::Relaxed),
        request.method == "HEAD",
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
        "HTTP/1.1 {code}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nX-Content-Type-Options: nosniff\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )?;
    Ok(())
}
fn respond(
    stream: &mut TcpStream,
    root: &Path,
    path: &str,
    default: &Path,
    generation: u64,
    head_only: bool,
) -> anyhow::Result<()> {
    let requested = if path.is_empty() {
        default.to_path_buf()
    } else {
        root.join(path)
    };
    let file = resolve_served_file(root, &requested);
    let (code, mut body) = if let Some(file) = file.as_ref() {
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
        body = inject_livereload(&body, generation);
    }
    write!(
        stream,
        "HTTP/1.1 {code}\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nContent-Security-Policy: {CSP}\r\nX-Content-Type-Options: nosniff\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
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
        "HTTP/1.1 200 OK\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n\r\n",
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
