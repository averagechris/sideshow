use std::process::Command;

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
        .args(["publish", deck.to_str().unwrap(), "--target", "srht"])
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
fn publish_srht_copies_to_docs_pages_and_prints_next_steps() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir(tmp.path().join(".git")).unwrap();
    let deck = prebuilt_t_deck(tmp.path());
    let output = Command::new(env!("CARGO_BIN_EXE_sideshow"))
        .args(["publish", deck.to_str().unwrap(), "--target", "srht"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        std::fs::read_to_string(tmp.path().join("docs/pages/demo.html")).unwrap(),
        "<!doctype html><title>T</title>"
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("next: commit and push to main"));
}

#[test]
fn publish_srht_rejects_bad_name() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir(tmp.path().join(".git")).unwrap();
    let deck = prebuilt_t_deck(tmp.path());
    let output = Command::new(env!("CARGO_BIN_EXE_sideshow"))
        .args([
            "publish",
            deck.to_str().unwrap(),
            "--target",
            "srht",
            "--name",
            "nope",
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("central pages publisher"));
}

#[test]
fn publish_srht_rejects_oversized_dist() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir(tmp.path().join(".git")).unwrap();
    let deck = prebuilt_t_deck(tmp.path());
    std::fs::write(deck.join("dist/t.html"), vec![b'x'; 2_000_001]).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_sideshow"))
        .args(["publish", deck.to_str().unwrap(), "--target", "srht"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("2000000-byte"));
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
