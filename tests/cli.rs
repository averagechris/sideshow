use std::process::Command;

fn review_command(state: &std::path::Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_sideshow"));
    command.env("XDG_STATE_HOME", state);
    command.arg("review");
    command
}

fn seed_review(
    deck: &std::path::Path,
    state: &std::path::Path,
) -> sideshow::review::ReviewArtifact {
    use sideshow::review::{
        NewReviewAnnotation, ReviewAction, ReviewBuildManifest, ReviewKind, ReviewMutation,
        ReviewRepository, ReviewSlideManifest, ReviewTarget,
    };

    let repository = ReviewRepository::with_state_root(deck, state).unwrap();
    let built = repository
        .update_build_manifest(
            0,
            ReviewBuildManifest {
                build_id: "build-from-test-output".into(),
                built_at_ms: 1,
                slides: vec![ReviewSlideManifest {
                    slide_id: "s-01".into(),
                    source_path: "slides/01.html".into(),
                    source_digest: "digest-one".into(),
                }],
                verification_commands: vec![
                    format!("sideshow check {}", deck.display()),
                    format!("sideshow build {}", deck.display()),
                ],
            },
        )
        .unwrap();
    repository
        .apply_mutation(ReviewMutation::Create {
            revision: built.revision,
            annotation: NewReviewAnnotation {
                slide_id: "s-01".into(),
                source_path: "slides/01.html".into(),
                target: ReviewTarget::Region {
                    x: 10.0,
                    y: 20.0,
                    width: 30.0,
                    height: 40.0,
                    selector_hint: Some("h1".into()),
                    text_hint: Some("T".into()),
                },
                body: "private annotation body".into(),
                kind: ReviewKind::Issue,
                action: Some(ReviewAction::Fix),
            },
        })
        .unwrap()
}

#[derive(Debug)]
struct CapturedRequest {
    path: String,
    authorization: String,
    body: Vec<u8>,
}

fn srht_test_server(
    status: &'static str,
    response_body: &'static str,
) -> (String, std::sync::mpsc::Receiver<CapturedRequest>) {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buf = Vec::new();
        let mut tmp = [0u8; 1024];
        let head_end = loop {
            let n = stream.read(&mut tmp).unwrap();
            assert!(n > 0);
            buf.extend_from_slice(&tmp[..n]);
            if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                break pos + 4;
            }
        };
        let head = String::from_utf8_lossy(&buf[..head_end]);
        let path = head
            .lines()
            .next()
            .unwrap()
            .split_whitespace()
            .nth(1)
            .unwrap()
            .to_owned();
        let authorization = head
            .lines()
            .find_map(|l| l.strip_prefix("Authorization: "))
            .unwrap_or("")
            .to_owned();
        let content_length: usize = head
            .lines()
            .find_map(|l| l.strip_prefix("Content-Length: "))
            .unwrap()
            .parse()
            .unwrap();
        let mut body = buf[head_end..].to_vec();
        while body.len() < content_length {
            let n = stream.read(&mut tmp).unwrap();
            assert!(n > 0);
            body.extend_from_slice(&tmp[..n]);
        }
        body.truncate(content_length);
        tx.send(CapturedRequest {
            path,
            authorization,
            body,
        })
        .unwrap();
        write!(
            stream,
            "HTTP/1.1 {status}\r\nContent-Length: {}\r\n\r\n{}",
            response_body.len(),
            response_body
        )
        .unwrap();
    });
    (format!("http://{addr}"), rx)
}

#[cfg(unix)]
fn make_executable(path: &std::path::Path, body: &str) {
    use std::os::unix::fs::PermissionsExt;

    std::fs::write(path, body).unwrap();
    let mut permissions = std::fs::metadata(path).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(path, permissions).unwrap();
}

fn prebuilt_t_deck(root: &std::path::Path) -> std::path::PathBuf {
    let deck = root.join("deck");
    std::fs::create_dir_all(deck.join("slides")).unwrap();
    std::fs::create_dir_all(deck.join("dist")).unwrap();
    std::fs::write(deck.join("deck.toml"), "[deck]\ntitle='T'\n").unwrap();
    std::fs::write(deck.join("theme.css"), "").unwrap();
    std::fs::write(deck.join("slides/01.html"), "<h1>T</h1>").unwrap();
    std::fs::write(deck.join("dist/t.html"), "<!doctype html><title>T</title>").unwrap();
    deck
}

#[cfg(unix)]
fn fake_tailwind(root: &std::path::Path) -> std::path::PathBuf {
    let path = root.join("tailwindcss");
    make_executable(
        &path,
        "#!/bin/sh\nin=''\nout=''\nwhile [ $# -gt 0 ]; do\n  case \"$1\" in\n    -i) in=$2; shift 2 ;;\n    -o) out=$2; shift 2 ;;\n    *) shift ;;\n  esac\ndone\ncp \"$in\" \"$out\"\n",
    );
    path
}

#[test]
fn new_then_build_outputs_single_file() {
    if which::which("tailwindcss").is_err() {
        eprintln!("skipping integration test: tailwindcss not on PATH");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let deck = tmp.path().join("deck");
    let bin = env!("CARGO_BIN_EXE_sideshow");
    let new = Command::new(bin)
        .args(["new", "--theme", "terminal", deck.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(new.status.success());
    assert_eq!(
        String::from_utf8_lossy(&new.stdout),
        format!(
            "created {} (theme: terminal)\nnext: sideshow check {} && sideshow build {}\n",
            deck.display(),
            deck.display(),
            deck.display()
        )
    );
    assert!(
        Command::new(bin)
            .args(["build", deck.to_str().unwrap()])
            .status()
            .unwrap()
            .success()
    );
    let out = deck.join("dist/terminal-demo.html");
    assert!(out.exists());
    let html = std::fs::read_to_string(out).unwrap();
    let first = html.find("data-src=\"slides/01-title.html\"").unwrap();
    let second = html.find("data-src=\"slides/02-content.md\"").unwrap();
    assert!(first < second);
    assert!(!html.contains("src=\"assets/"));
    assert!(!html.contains("http://"));
    assert!(html.contains("sideshow-runtime-v1"));
    assert!(html.contains("data-runtime=\"sideshow-runtime-v1\""));
    assert!(html.contains("sideshow.audit"));
    assert!(html.contains("sideshow.goto"));
}

#[test]
fn build_scans_deck_slides_from_other_cwd_without_cwd_decoys() {
    if which::which("tailwindcss").is_err() {
        eprintln!("skipping integration test: tailwindcss not on PATH");
        return;
    }

    let tmp = tempfile::tempdir().unwrap();
    let deck = tmp.path().join("deck");
    let cwd = tmp.path().join("cwd");
    std::fs::create_dir_all(&cwd).unwrap();
    std::fs::write(
        cwd.join("decoy.html"),
        r#"<div class="backdrop-invert"></div>"#,
    )
    .unwrap();

    let bin = env!("CARGO_BIN_EXE_sideshow");
    assert!(
        Command::new(bin)
            .args(["new", deck.to_str().unwrap()])
            .status()
            .unwrap()
            .success()
    );
    std::fs::write(
        deck.join("slides/01-title.html"),
        r#"<h1 class="text-[7.2rem] grid grid-cols-2">Scoped</h1>"#,
    )
    .unwrap();
    assert!(
        Command::new(bin)
            .current_dir(&cwd)
            .args(["build", deck.to_str().unwrap()])
            .status()
            .unwrap()
            .success()
    );

    let html = std::fs::read_to_string(deck.join("dist/signal-demo.html")).unwrap();
    assert!(
        html.contains("7.2rem"),
        "arbitrary utility was not generated"
    );
    assert!(
        html.contains("grid-cols-2"),
        "deck slide utility was not generated"
    );
    assert!(
        !html.contains("backdrop-invert"),
        "Tailwind scanned a decoy file outside the deck"
    );
}

#[test]
fn check_success_message_mentions_static_findings() {
    let tmp = tempfile::tempdir().unwrap();
    let deck = tmp.path().join("deck");
    let bin = env!("CARGO_BIN_EXE_sideshow");
    assert!(
        Command::new(bin)
            .args(["new", deck.to_str().unwrap()])
            .status()
            .unwrap()
            .success()
    );

    let check = Command::new(bin)
        .args(["check", deck.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(check.status.success());
    assert_eq!(
        String::from_utf8_lossy(&check.stdout),
        "ok: no static findings (run the browser audit for visual verification)\n"
    );
}

#[test]
fn build_highlights_recognized_markdown_code_fences() {
    if which::which("tailwindcss").is_err() {
        eprintln!("skipping integration test: tailwindcss not on PATH");
        return;
    }

    let tmp = tempfile::tempdir().unwrap();
    let deck = tmp.path().join("deck");
    std::fs::create_dir_all(deck.join("slides")).unwrap();
    std::fs::write(deck.join("deck.toml"), "[deck]\ntitle='Syntax'\n").unwrap();
    std::fs::write(deck.join("theme.css"), "").unwrap();
    std::fs::write(
        deck.join("slides/01-code.md"),
        "# Code\n\n```rust\nfn main() { let n = 1; }\n```\n",
    )
    .unwrap();

    let bin = env!("CARGO_BIN_EXE_sideshow");
    assert!(
        Command::new(bin)
            .args(["build", deck.to_str().unwrap()])
            .status()
            .unwrap()
            .success()
    );

    let html = std::fs::read_to_string(deck.join("dist/syntax.html")).unwrap();
    assert!(html.contains("class=\"language-rust\""));
    assert!(html.contains("tok-kw"));
    assert!(html.contains("tok-fn"));
}

#[test]
fn build_optimizes_large_png_when_enabled_and_check_warns() {
    if which::which("tailwindcss").is_err() {
        eprintln!("skipping integration test: tailwindcss not on PATH");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let deck = tmp.path().join("deck");
    std::fs::create_dir_all(deck.join("slides")).unwrap();
    std::fs::create_dir_all(deck.join("assets")).unwrap();
    std::fs::write(deck.join("deck.toml"), "[deck]\ntitle='T'\n").unwrap();
    std::fs::write(deck.join("theme.css"), "").unwrap();
    std::fs::write(
        deck.join("slides/01.html"),
        "<img src='assets/huge.png'><a href='assets/big.bin'>download</a>",
    )
    .unwrap();
    std::fs::write(deck.join("assets/big.bin"), vec![0u8; 400_000]).unwrap();
    let img = image::RgbaImage::from_fn(1400, 900, |x, y| {
        image::Rgba([((x / 8) % 255) as u8, ((y / 8) % 255) as u8, 120, 255])
    });
    img.save(deck.join("assets/huge.png")).unwrap();
    let bin = env!("CARGO_BIN_EXE_sideshow");
    let check = Command::new(bin)
        .args(["check", deck.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(check.status.success(), "warnings should not fail check");
    assert!(String::from_utf8_lossy(&check.stdout).contains("Warning"));
    std::fs::write(
        deck.join("deck.toml"),
        "[deck]\ntitle='T'\n\n[images]\noptimize=false\n",
    )
    .unwrap();
    assert!(
        Command::new(bin)
            .args(["build", deck.to_str().unwrap()])
            .status()
            .unwrap()
            .success()
    );
    let unoptimized = std::fs::metadata(deck.join("dist/t.html")).unwrap().len();
    std::fs::write(
        deck.join("deck.toml"),
        "[deck]\ntitle='T'\n\n[images]\noptimize=true\nquality=80\nmax_dim=3840\n",
    )
    .unwrap();
    assert!(
        Command::new(bin)
            .args(["build", deck.to_str().unwrap()])
            .status()
            .unwrap()
            .success()
    );
    let optimized = std::fs::metadata(deck.join("dist/t.html")).unwrap().len();
    assert!(optimized < unoptimized, "{optimized} !< {unoptimized}");
}

#[test]
fn build_inlines_video_assets_and_runtime_wires_playback() {
    if which::which("tailwindcss").is_err() {
        eprintln!("skipping integration test: tailwindcss not on PATH");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let deck = tmp.path().join("deck");
    std::fs::create_dir_all(deck.join("slides")).unwrap();
    std::fs::create_dir_all(deck.join("assets")).unwrap();
    std::fs::write(deck.join("deck.toml"), "[deck]\ntitle='Video'\n").unwrap();
    std::fs::write(deck.join("theme.css"), "").unwrap();
    std::fs::write(
        deck.join("slides/01.html"),
        "<video src=\"assets/demo.webm\"></video>",
    )
    .unwrap();
    std::fs::write(deck.join("assets/demo.webm"), b"demo video bytes").unwrap();

    let bin = env!("CARGO_BIN_EXE_sideshow");
    assert!(
        Command::new(bin)
            .args(["build", deck.to_str().unwrap()])
            .status()
            .unwrap()
            .success()
    );

    let html = std::fs::read_to_string(deck.join("dist/video.html")).unwrap();
    assert!(html.contains("src=\"data:video/webm;base64,"));
    assert!(html.contains("initVideos"));
    assert!(html.contains("prefers-reduced-motion: reduce"));
    assert!(html.contains("video.play()"));
}

#[test]
fn publish_missing_dist_mentions_build_first() {
    let tmp = tempfile::tempdir().unwrap();
    let deck = tmp.path().join("deck");
    std::fs::create_dir_all(deck.join("slides")).unwrap();
    std::fs::write(deck.join("deck.toml"), "[deck]\ntitle='T'\n").unwrap();
    std::fs::write(deck.join("theme.css"), "").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_sideshow"))
        .args([
            "publish",
            deck.to_str().unwrap(),
            "--target",
            "srht",
            "--domain",
            "example.srht.site",
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("sideshow build"));
}

#[cfg(unix)]
#[test]
fn publish_s3_uses_configured_aws_and_prints_presigned_url() {
    let tmp = tempfile::tempdir().unwrap();
    let deck = prebuilt_t_deck(tmp.path());
    let log = tmp.path().join("aws.log");
    let aws = tmp.path().join("aws");
    make_executable(
        &aws,
        &format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> {}\nif [ \"$2\" = presign ]; then printf '%s\\n' 'https://example.invalid/t'; fi\n",
            log.display()
        ),
    );
    let config = tmp.path().join("config.toml");
    std::fs::write(&config, format!("[tools]\naws = '{}'\n", aws.display())).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_sideshow"))
        .args([
            "publish",
            deck.to_str().unwrap(),
            "--target",
            "s3",
            "--bucket",
            "bucket",
            "--key",
            "custom.html",
            "--expires",
            "60",
        ])
        .env("SIDESHOW_CONFIG", &config)
        .env_remove("SIDESHOW_AWS")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let log = std::fs::read_to_string(log).unwrap();
    assert!(log.contains(&format!(
        "s3 cp {} s3://bucket/custom.html --content-type text/html --no-progress",
        deck.join("dist/t.html").display()
    )));
    assert!(log.contains("s3 presign s3://bucket/custom.html --expires-in 60"));
    assert!(String::from_utf8_lossy(&output.stdout).contains("https://example.invalid/t"));
}

#[test]
fn publish_s3_rejects_invalid_expires() {
    let tmp = tempfile::tempdir().unwrap();
    let deck = prebuilt_t_deck(tmp.path());
    for expires in ["0", "604801"] {
        let output = Command::new(env!("CARGO_BIN_EXE_sideshow"))
            .args([
                "publish",
                deck.to_str().unwrap(),
                "--target",
                "s3",
                "--bucket",
                "b",
                "--expires",
                expires,
            ])
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("604800"));
    }
}

#[test]
fn publish_srht_uploads_to_pages_api() {
    let tmp = tempfile::tempdir().unwrap();
    let deck = prebuilt_t_deck(tmp.path());
    let (url, rx) = srht_test_server("200 OK", "v123\n");
    let output = Command::new(env!("CARGO_BIN_EXE_sideshow"))
        .args([
            "publish",
            deck.to_str().unwrap(),
            "--target",
            "srht",
            "--domain",
            "example.srht.site",
            "--pages-url",
            &url,
        ])
        .env("SRHT_TOKEN", "tok123")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let request = rx.recv().unwrap();
    assert_eq!(request.path, "/publish/example.srht.site/t");
    assert_eq!(request.authorization, "Bearer tok123");
    assert!(
        request
            .body
            .windows(b"name=\"content\"".len())
            .any(|w| w == b"name=\"content\"")
    );
    assert!(request.body.windows(2).any(|w| w == b"\x1f\x8b"));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("https://example.srht.site/t/"));
    assert!(stdout.contains("v123"));
}

#[test]
fn publish_srht_requires_token() {
    let tmp = tempfile::tempdir().unwrap();
    let deck = prebuilt_t_deck(tmp.path());
    let config = tmp.path().join("config.toml");
    std::fs::write(&config, "").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_sideshow"))
        .args([
            "publish",
            deck.to_str().unwrap(),
            "--target",
            "srht",
            "--domain",
            "example.srht.site",
        ])
        .env_remove("SRHT_TOKEN")
        .env("SIDESHOW_CONFIG", &config)
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("SRHT_TOKEN"));
    assert!(stderr.contains("meta.sr.ht/oauth2"));
}

#[cfg(unix)]
#[test]
fn publish_srht_token_cmd_from_config() {
    let tmp = tempfile::tempdir().unwrap();
    let deck = prebuilt_t_deck(tmp.path());
    let script = tmp.path().join("tok");
    make_executable(&script, "#!/bin/sh\necho 'tok-from-cmd extra'\n");
    let config = tmp.path().join("config.toml");
    std::fs::write(
        &config,
        format!("[srht]\ntoken-cmd = ['{}']\n", script.display()),
    )
    .unwrap();
    let (url, rx) = srht_test_server("200 OK", "v456");
    let output = Command::new(env!("CARGO_BIN_EXE_sideshow"))
        .args([
            "publish",
            deck.to_str().unwrap(),
            "--target",
            "srht",
            "--domain",
            "example.srht.site",
            "--pages-url",
            &url,
        ])
        .env_remove("SRHT_TOKEN")
        .env("SIDESHOW_CONFIG", &config)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(rx.recv().unwrap().authorization, "Bearer tok-from-cmd");
}

#[test]
fn publish_srht_rejects_bad_subdir() {
    let tmp = tempfile::tempdir().unwrap();
    let deck = prebuilt_t_deck(tmp.path());
    for (subdir, expected) in [
        ("../evil", ".."),
        ("", "root publishing is intentionally unsupported"),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_sideshow"))
            .args([
                "publish",
                deck.to_str().unwrap(),
                "--target",
                "srht",
                "--domain",
                "example.srht.site",
                "--subdir",
                subdir,
            ])
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains(expected));
    }
}

#[test]
fn publish_srht_surfaces_api_errors() {
    let tmp = tempfile::tempdir().unwrap();
    let deck = prebuilt_t_deck(tmp.path());
    let (url, _rx) = srht_test_server("401 Unauthorized", "bad token");
    let output = Command::new(env!("CARGO_BIN_EXE_sideshow"))
        .args([
            "publish",
            deck.to_str().unwrap(),
            "--target",
            "srht",
            "--domain",
            "example.srht.site",
            "--pages-url",
            &url,
        ])
        .env("SRHT_TOKEN", "tok123")
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("401"));
    assert!(stderr.contains("pages.sr.ht/PAGES:RW"));
}

#[test]
fn video_optimize_errors_helpfully_without_ffmpeg() {
    if which::which("ffmpeg").is_ok() {
        eprintln!("skipping ffmpeg-missing test: ffmpeg is on PATH");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let input = tmp.path().join("demo.mp4");
    std::fs::write(&input, b"not a real mp4").unwrap();
    let bin = env!("CARGO_BIN_EXE_sideshow");
    let out = Command::new(bin)
        .env_remove("SIDESHOW_FFMPEG")
        .env("SIDESHOW_CONFIG", tmp.path().join("missing-config.toml"))
        .args(["video", "optimize", input.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr
            .contains("ffmpeg not found: sideshow video optimize needs ffmpeg to re-encode videos"),
        "{stderr}"
    );
    assert!(
        stderr.contains("https://ffmpeg.org/download.html"),
        "{stderr}"
    );
    assert!(stderr.contains("SIDESHOW_FFMPEG"), "{stderr}");
}

#[test]
fn video_optimize_uses_ffmpeg_from_config() {
    let tmp = tempfile::tempdir().unwrap();
    let input = tmp.path().join("demo.mp4");
    let ffmpeg = tmp.path().join("ffmpeg");
    let config = tmp.path().join("config.toml");
    std::fs::write(&input, b"not a real mp4").unwrap();
    make_executable(&ffmpeg, "#!/bin/sh\nexit 1\n");
    std::fs::write(
        &config,
        format!("[tools]\nffmpeg = '{}'\n", ffmpeg.display()),
    )
    .unwrap();

    let bin = env!("CARGO_BIN_EXE_sideshow");
    let out = Command::new(bin)
        .env_remove("SIDESHOW_FFMPEG")
        .env("SIDESHOW_CONFIG", &config)
        .args(["video", "optimize", input.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("ffmpeg failed while optimizing video"),
        "{stderr}"
    );
    assert!(!stderr.contains("ffmpeg not found"), "{stderr}");
}

#[test]
fn video_optimize_errors_when_config_ffmpeg_is_not_file() {
    let tmp = tempfile::tempdir().unwrap();
    let input = tmp.path().join("demo.mp4");
    let missing = tmp.path().join("missing-ffmpeg");
    let config = tmp.path().join("config.toml");
    std::fs::write(&input, b"not a real mp4").unwrap();
    std::fs::write(
        &config,
        format!("[tools]\nffmpeg = '{}'\n", missing.display()),
    )
    .unwrap();

    let bin = env!("CARGO_BIN_EXE_sideshow");
    let out = Command::new(bin)
        .env_remove("SIDESHOW_FFMPEG")
        .env("SIDESHOW_CONFIG", &config)
        .args(["video", "optimize", input.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains(&format!(
            "{} [tools] ffmpeg points to {}, but it is not a file",
            config.display(),
            missing.display()
        )),
        "{stderr}"
    );
}

#[test]
fn video_optimize_env_ffmpeg_beats_config() {
    let tmp = tempfile::tempdir().unwrap();
    let input = tmp.path().join("demo.mp4");
    let ffmpeg = tmp.path().join("ffmpeg");
    let missing = tmp.path().join("missing-ffmpeg");
    let config = tmp.path().join("config.toml");
    std::fs::write(&input, b"not a real mp4").unwrap();
    make_executable(&ffmpeg, "#!/bin/sh\nexit 1\n");
    std::fs::write(
        &config,
        format!("[tools]\nffmpeg = '{}'\n", missing.display()),
    )
    .unwrap();

    let bin = env!("CARGO_BIN_EXE_sideshow");
    let out = Command::new(bin)
        .env("SIDESHOW_FFMPEG", &ffmpeg)
        .env("SIDESHOW_CONFIG", &config)
        .args(["video", "optimize", input.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("ffmpeg failed while optimizing video"),
        "{stderr}"
    );
    assert!(!stderr.contains("points to"), "{stderr}");
}

#[test]
fn tape_render_errors_helpfully_without_vhs() {
    if which::which("vhs").is_ok() {
        eprintln!("skipping vhs-missing test: vhs is on PATH");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let deck = tmp.path().join("deck");
    std::fs::create_dir_all(deck.join("tapes")).unwrap();
    std::fs::write(deck.join("deck.toml"), "[deck]\ntitle='Tape'\n").unwrap();
    std::fs::write(
        deck.join("tapes/demo.tape"),
        "Output \"assets/demo.webm\"\n",
    )
    .unwrap();
    let bin = env!("CARGO_BIN_EXE_sideshow");
    let out = Command::new(bin)
        .env_remove("SIDESHOW_VHS")
        .env("SIDESHOW_CONFIG", tmp.path().join("missing-config.toml"))
        .args(["tape", "render", deck.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("vhs not found: sideshow tape render needs vhs to render terminal demos"),
        "{stderr}"
    );
    assert!(
        stderr.contains("https://github.com/charmbracelet/vhs"),
        "{stderr}"
    );
    assert!(stderr.contains("SIDESHOW_VHS"), "{stderr}");
}

#[test]
fn review_artifact_list_and_export_are_stable_and_deck_read_only() {
    let tmp = tempfile::tempdir().unwrap();
    let deck = prebuilt_t_deck(tmp.path());
    let state = tmp.path().join("state");
    std::fs::create_dir(&state).unwrap();
    let source_before = std::fs::read(deck.join("slides/01.html")).unwrap();
    let built_before = std::fs::read(deck.join("dist/t.html")).unwrap();
    let seeded = seed_review(&deck, &state);

    let artifact_output = review_command(&state)
        .args(["artifact", deck.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        artifact_output.status.success(),
        "{}",
        String::from_utf8_lossy(&artifact_output.stderr)
    );
    let locator: serde_json::Value = serde_json::from_slice(&artifact_output.stdout).unwrap();
    assert_eq!(locator["schema_version"], 2);
    assert_eq!(locator["revision"], seeded.revision);
    assert_eq!(
        locator["canonical_root"],
        deck.canonicalize().unwrap().to_str().unwrap()
    );
    assert_eq!(locator["root_key"].as_str().unwrap().len(), 64);
    let artifact_path = std::path::PathBuf::from(locator["artifact_path"].as_str().unwrap());
    assert!(artifact_path.starts_with(state.canonicalize().unwrap()));
    assert!(!artifact_path.starts_with(deck.canonicalize().unwrap()));

    let list = review_command(&state)
        .args(["list", deck.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(list.status.success());
    let listed: sideshow::review::ReviewArtifact = serde_json::from_slice(&list.stdout).unwrap();
    assert_eq!(listed, seeded);
    assert_eq!(
        listed.annotations[0].freshness,
        sideshow::review::ReviewFreshness::Current
    );

    let markdown = review_command(&state)
        .args(["export", deck.to_str().unwrap(), "--format", "markdown"])
        .output()
        .unwrap();
    assert!(markdown.status.success());
    let markdown = String::from_utf8(markdown.stdout).unwrap();
    for expected in [
        "Deck root:",
        "Source:",
        "Slide:",
        "Workflow:",
        "Freshness:",
        "Disposition:",
        "Target:",
        "private annotation body",
        "sideshow check",
        "sideshow build",
    ] {
        assert!(
            markdown.contains(expected),
            "missing {expected}: {markdown}"
        );
    }

    let exported = tmp.path().join("handoff.json");
    let export = review_command(&state)
        .args([
            "export",
            deck.to_str().unwrap(),
            "--output",
            exported.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(export.status.success());
    let exported_artifact: sideshow::review::ReviewArtifact =
        serde_json::from_slice(&std::fs::read(exported).unwrap()).unwrap();
    assert_eq!(exported_artifact, seeded);

    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt, symlink};

        let exported = tmp.path().join("private-export.json");
        let export = review_command(&state)
            .args([
                "export",
                deck.to_str().unwrap(),
                "--output",
                exported.to_str().unwrap(),
            ])
            .output()
            .unwrap();
        assert!(export.status.success());
        assert_eq!(
            std::fs::metadata(&exported).unwrap().permissions().mode() & 0o777,
            0o600
        );

        let dangling_target = deck.join("must-not-be-created.json");
        let dangling_export = tmp.path().join("dangling-export.json");
        symlink(&dangling_target, &dangling_export).unwrap();
        let export = review_command(&state)
            .args([
                "export",
                deck.to_str().unwrap(),
                "--output",
                dangling_export.to_str().unwrap(),
            ])
            .output()
            .unwrap();
        assert!(
            export.status.success(),
            "{}",
            String::from_utf8_lossy(&export.stderr)
        );
        assert!(!dangling_target.exists());
        assert!(
            !std::fs::symlink_metadata(&dangling_export)
                .unwrap()
                .file_type()
                .is_symlink()
        );

        let hardlink_export = tmp.path().join("hardlink-export.json");
        std::fs::hard_link(deck.join("slides/01.html"), &hardlink_export).unwrap();
        assert!(std::fs::metadata(&hardlink_export).unwrap().nlink() >= 2);
        let export = review_command(&state)
            .args([
                "export",
                deck.to_str().unwrap(),
                "--output",
                hardlink_export.to_str().unwrap(),
            ])
            .output()
            .unwrap();
        assert!(
            export.status.success(),
            "{}",
            String::from_utf8_lossy(&export.stderr)
        );
        assert_eq!(
            std::fs::read(deck.join("slides/01.html")).unwrap(),
            source_before
        );
        assert_eq!(std::fs::metadata(&hardlink_export).unwrap().nlink(), 1);

        let parent_link = tmp.path().join("deck-parent-link");
        symlink(&deck, &parent_link).unwrap();
        let rejected = review_command(&state)
            .args([
                "export",
                deck.to_str().unwrap(),
                "--output",
                parent_link.join("review.json").to_str().unwrap(),
            ])
            .output()
            .unwrap();
        assert!(!rejected.status.success());
        assert!(String::from_utf8_lossy(&rejected.stderr).contains("inside the deck"));
    }

    let rejected = review_command(&state)
        .args([
            "export",
            deck.to_str().unwrap(),
            "--output",
            deck.join("review.json").to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(!rejected.status.success());
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("inside the deck"));
    assert!(!deck.join("review.json").exists());
    assert_eq!(
        std::fs::read(deck.join("slides/01.html")).unwrap(),
        source_before
    );
    assert_eq!(
        std::fs::read(deck.join("dist/t.html")).unwrap(),
        built_before
    );
    let built = String::from_utf8(built_before).unwrap();
    assert!(!built.contains("private annotation body"));
    assert!(!built.contains("schema_version"));
}

#[test]
fn review_artifact_materializes_empty_v2_for_direct_consumers() {
    let tmp = tempfile::tempdir().unwrap();
    let deck = prebuilt_t_deck(tmp.path());
    let state = tmp.path().join("state");
    std::fs::create_dir(&state).unwrap();

    let locator_output = review_command(&state)
        .args(["artifact", deck.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(locator_output.status.success());
    let locator: serde_json::Value = serde_json::from_slice(&locator_output.stdout).unwrap();
    let artifact_path = std::path::PathBuf::from(locator["artifact_path"].as_str().unwrap());
    assert!(artifact_path.is_file());
    let direct: sideshow::review::ReviewArtifact =
        serde_json::from_slice(&std::fs::read(&artifact_path).unwrap()).unwrap();
    assert_eq!(direct.schema_version, 2);
    assert_eq!(direct.revision, 0);

    let list = review_command(&state)
        .args(["list", deck.to_str().unwrap()])
        .output()
        .unwrap();
    let listed: sideshow::review::ReviewArtifact = serde_json::from_slice(&list.stdout).unwrap();
    let export = review_command(&state)
        .args(["export", deck.to_str().unwrap()])
        .output()
        .unwrap();
    let exported: sideshow::review::ReviewArtifact =
        serde_json::from_slice(&export.stdout).unwrap();
    assert_eq!(direct, listed);
    assert_eq!(listed, exported);
}

#[test]
fn review_xdg_empty_falls_back_to_home_and_relative_nonempty_rejects() {
    let tmp = tempfile::tempdir().unwrap();
    let deck = prebuilt_t_deck(tmp.path());
    let home = tmp.path().join("home");
    std::fs::create_dir(&home).unwrap();
    let bin = env!("CARGO_BIN_EXE_sideshow");

    let fallback = Command::new(bin)
        .env("XDG_STATE_HOME", "")
        .env("HOME", &home)
        .args(["review", "artifact", deck.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        fallback.status.success(),
        "{}",
        String::from_utf8_lossy(&fallback.stderr)
    );
    let locator: serde_json::Value = serde_json::from_slice(&fallback.stdout).unwrap();
    assert!(
        std::path::Path::new(locator["artifact_path"].as_str().unwrap()).starts_with(
            home.canonicalize()
                .unwrap()
                .join(".local/state/sideshow/reviews")
        )
    );

    let relative = Command::new(bin)
        .env("XDG_STATE_HOME", "relative/state")
        .env("HOME", &home)
        .args(["review", "artifact", deck.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!relative.status.success());
    assert!(String::from_utf8_lossy(&relative.stderr).contains("must be absolute"));
}

#[cfg(unix)]
#[test]
fn standalone_build_refreshes_existing_review_but_does_not_create_one() {
    let tmp = tempfile::tempdir().unwrap();
    let deck = prebuilt_t_deck(tmp.path());
    let state = tmp.path().join("state");
    std::fs::create_dir(&state).unwrap();
    let seeded = seed_review(&deck, &state);
    let tailwind = fake_tailwind(tmp.path());
    std::fs::write(deck.join("slides/01.html"), "<h1>Changed</h1>").unwrap();

    let build = Command::new(env!("CARGO_BIN_EXE_sideshow"))
        .env("XDG_STATE_HOME", &state)
        .env("SIDESHOW_TAILWINDCSS", &tailwind)
        .args(["build", deck.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        build.status.success(),
        "{}",
        String::from_utf8_lossy(&build.stderr)
    );
    let refreshed = sideshow::review::ReviewRepository::with_state_root(&deck, &state)
        .unwrap()
        .load_artifact()
        .unwrap();
    assert!(refreshed.revision > seeded.revision);
    assert_eq!(
        refreshed.annotations[0].freshness,
        sideshow::review::ReviewFreshness::Stale
    );
    assert!(
        refreshed
            .build
            .as_ref()
            .unwrap()
            .build_id
            .ne(&seeded.build.as_ref().unwrap().build_id)
    );

    let untouched_state = tmp.path().join("untouched-state");
    std::fs::create_dir(&untouched_state).unwrap();
    let absent_repository =
        sideshow::review::ReviewRepository::with_state_root(&deck, &untouched_state).unwrap();
    assert!(!absent_repository.artifact_path().exists());
    let build = Command::new(env!("CARGO_BIN_EXE_sideshow"))
        .env("XDG_STATE_HOME", &untouched_state)
        .env("SIDESHOW_TAILWINDCSS", &tailwind)
        .args(["build", deck.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(build.status.success());
    assert!(!absent_repository.artifact_path().exists());
    assert!(!untouched_state.join("sideshow").exists());
}

#[test]
fn review_cli_mutations_require_revisions_and_keep_workflow_orthogonal() {
    let tmp = tempfile::tempdir().unwrap();
    let deck = prebuilt_t_deck(tmp.path());
    let state = tmp.path().join("state");
    std::fs::create_dir(&state).unwrap();
    let seeded = seed_review(&deck, &state);
    let id = seeded.annotations[0].id.clone();

    let resolve = review_command(&state)
        .args([
            "resolve",
            deck.to_str().unwrap(),
            &id,
            "--revision",
            &seeded.revision.to_string(),
        ])
        .output()
        .unwrap();
    assert!(resolve.status.success());
    let resolved: sideshow::review::ReviewArtifact =
        serde_json::from_slice(&resolve.stdout).unwrap();
    assert_eq!(
        resolved.annotations[0].state,
        sideshow::review::ReviewState::Resolved
    );
    assert_eq!(
        resolved.annotations[0].disposition,
        sideshow::review::ReviewDisposition::Pending
    );

    let disposition = review_command(&state)
        .args([
            "disposition",
            deck.to_str().unwrap(),
            &id,
            "--status",
            "addressed",
            "--note",
            "verified with focused tests",
            "--revision",
            &resolved.revision.to_string(),
        ])
        .output()
        .unwrap();
    assert!(disposition.status.success());
    let dispositioned: sideshow::review::ReviewArtifact =
        serde_json::from_slice(&disposition.stdout).unwrap();
    assert_eq!(
        dispositioned.annotations[0].state,
        sideshow::review::ReviewState::Resolved
    );
    assert_eq!(
        dispositioned.annotations[0].disposition,
        sideshow::review::ReviewDisposition::Addressed
    );
    assert_eq!(
        dispositioned.annotations[0].disposition_note.as_deref(),
        Some("verified with focused tests")
    );

    let reopen = review_command(&state)
        .args([
            "reopen",
            deck.to_str().unwrap(),
            &id,
            "--revision",
            &dispositioned.revision.to_string(),
        ])
        .output()
        .unwrap();
    assert!(reopen.status.success());
    let reopened: sideshow::review::ReviewArtifact =
        serde_json::from_slice(&reopen.stdout).unwrap();
    assert_eq!(
        reopened.annotations[0].state,
        sideshow::review::ReviewState::Todo
    );
    assert_eq!(
        reopened.annotations[0].disposition,
        sideshow::review::ReviewDisposition::Addressed
    );

    let stale = review_command(&state)
        .args([
            "resolve",
            deck.to_str().unwrap(),
            &id,
            "--revision",
            &seeded.revision.to_string(),
        ])
        .output()
        .unwrap();
    assert!(!stale.status.success());
    let stderr = String::from_utf8_lossy(&stale.stderr);
    assert!(stderr.contains(&format!("current revision is {}", reopened.revision)));
    assert!(!stderr.contains("private annotation body"));

    let unconfirmed = review_command(&state)
        .args([
            "clear",
            deck.to_str().unwrap(),
            "--revision",
            &reopened.revision.to_string(),
        ])
        .output()
        .unwrap();
    assert!(!unconfirmed.status.success());
    assert!(String::from_utf8_lossy(&unconfirmed.stderr).contains("requires --yes"));

    let clear = review_command(&state)
        .args([
            "clear",
            deck.to_str().unwrap(),
            "--revision",
            &reopened.revision.to_string(),
            "--yes",
        ])
        .output()
        .unwrap();
    assert!(clear.status.success());
    let cleared: sideshow::review::ReviewArtifact = serde_json::from_slice(&clear.stdout).unwrap();
    assert!(cleared.annotations.is_empty());
    assert!(cleared.cleared_at_ms.is_some());
    assert!(
        std::path::Path::new(&cleared.deck.canonical_root)
            .join("slides/01.html")
            .is_file()
    );
    assert!(deck.join("dist/t.html").is_file());
}
