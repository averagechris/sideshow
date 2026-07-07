use std::process::Command;

#[test]
fn new_then_build_outputs_single_file() {
    if which::which("tailwindcss").is_err() {
        eprintln!("skipping integration test: tailwindcss not on PATH");
        return;
    }
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
    assert!(
        Command::new(bin)
            .args(["build", deck.to_str().unwrap()])
            .status()
            .unwrap()
            .success()
    );
    let out = deck.join("dist/signal-demo.html");
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
