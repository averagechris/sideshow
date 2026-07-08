use anyhow::Context;
use clap::{Parser, Subcommand, ValueEnum};
use sideshow::find_tool;
use std::{
    fs,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    time::SystemTime,
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
    /// Publish an existing dist output to S3 or the srht pages handoff.
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
        /// srht pages allowlisted output name.
        #[arg(long, default_value = "demo")]
        name: String,
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
            name,
        } => publish(
            &dir,
            target,
            bucket.as_deref(),
            key.as_deref(),
            expires,
            &name,
        ),
    }
}

const SRHT_PAGE_NAMES: &[&str] = &[
    "overview",
    "examples",
    "example",
    "demo",
    "changelog",
    "tour",
    "sample-review",
];
const SRHT_MAX_DOC_BYTES: u64 = 2_000_000;

fn publish(
    dir: &Path,
    target: PublishTarget,
    bucket: Option<&str>,
    key: Option<&str>,
    expires: u32,
    name: &str,
) -> anyhow::Result<()> {
    let dist = sideshow::deck_dist_path(dir)?;
    if !dist.is_file() {
        anyhow::bail!(
            "dist output not found: {}; run sideshow build {} first",
            dist.display(),
            dir.display()
        );
    }
    warn_if_stale(dir, &dist)?;
    match target {
        PublishTarget::S3 => publish_s3(&dist, bucket, key, expires),
        PublishTarget::Srht => publish_srht(dir, &dist, name),
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

fn validate_srht_name(name: &str) -> anyhow::Result<()> {
    if !SRHT_PAGE_NAMES.contains(&name) {
        anyhow::bail!(
            "--name must be one of {}; the central pages publisher only fetches these docs/pages/*.html filenames",
            SRHT_PAGE_NAMES.join(", ")
        );
    }
    Ok(())
}

fn publish_srht(dir: &Path, dist: &Path, name: &str) -> anyhow::Result<()> {
    validate_srht_name(name)?;
    let bytes = fs::metadata(dist)?.len();
    if bytes > SRHT_MAX_DOC_BYTES {
        anyhow::bail!(
            "dist output is {bytes} bytes, exceeding the 2000000-byte srht publisher cap; optimize images and rebuild"
        );
    }
    let root = find_repo_root(dir)?;
    let dest = root.join("docs/pages").join(format!("{name}.html"));
    fs::create_dir_all(dest.parent().unwrap())?;
    fs::copy(dist, &dest)?;
    println!("copied {} -> {}", dist.display(), dest.display());
    println!(
        "next: commit and push to main; the central pages publisher picks it up on its next refresh (published as {name}.html under this project's pages path)"
    );
    Ok(())
}

fn find_repo_root(start: &Path) -> anyhow::Result<PathBuf> {
    for p in start.ancestors() {
        if p.join(".jj").exists() || p.join(".git").exists() {
            return Ok(p.to_path_buf());
        }
    }
    anyhow::bail!(
        "could not find an enclosing repository root from {}; expected a parent with .jj or .git",
        start.display()
    )
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
    let mut last = newest_mtime(dir)?;
    let out = sideshow::build_deck(dir)?;
    let root = dir.join("dist");
    let listener = TcpListener::bind(("127.0.0.1", port))?;
    println!("serving {} at http://localhost:{port}/", root.display());
    for stream in listener.incoming() {
        let mut stream = stream?;
        let req = read_req(&mut stream)?;
        let path = req
            .split_whitespace()
            .nth(1)
            .unwrap_or("/")
            .trim_start_matches('/');
        if path.is_empty() || path.ends_with(".html") {
            let now = newest_mtime(dir)?;
            if now > last {
                let _ = sideshow::build_deck(dir);
                last = now;
            }
        }
        respond(&mut stream, &root, path, &out)?;
    }
    Ok(())
}

fn read_req(stream: &mut TcpStream) -> anyhow::Result<String> {
    let mut b = [0; 1024];
    let n = stream.read(&mut b)?;
    Ok(String::from_utf8_lossy(&b[..n]).into())
}
fn respond(stream: &mut TcpStream, root: &Path, path: &str, default: &Path) -> anyhow::Result<()> {
    let file = if path.is_empty() {
        default.to_path_buf()
    } else {
        root.join(path)
    };
    let (code, body) = if file.is_file() {
        ("200 OK", fs::read(&file)?)
    } else {
        (
            "404 Not Found",
            b"<!doctype html><title>404 Not Found</title><h1>404 Not Found</h1>".to_vec(),
        )
    };
    let mime = match file.extension().and_then(|s| s.to_str()).unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "css" => "text/css",
        "js" => "text/javascript",
        "json" => "application/json",
        "svg" => "image/svg+xml",
        _ => "application/octet-stream",
    };
    write!(
        stream,
        "HTTP/1.1 {code}\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nContent-Security-Policy: {CSP}\r\naccess-control-allow-origin: *\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    stream.write_all(&body)?;
    Ok(())
}
fn newest_mtime(dir: &Path) -> anyhow::Result<SystemTime> {
    fn walk(p: &Path, max: &mut SystemTime) -> anyhow::Result<()> {
        for e in fs::read_dir(p)? {
            let e = e?;
            let p = e.path();
            if p.file_name().and_then(|s| s.to_str()) == Some("dist") {
                continue;
            }
            if p.is_dir() {
                walk(&p, max)?;
            } else if let Ok(m) = e.metadata()?.modified()
                && m > *max
            {
                *max = m;
            }
        }
        Ok(())
    }
    let mut max = SystemTime::UNIX_EPOCH;
    walk(dir, &mut max)?;
    Ok(max)
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
