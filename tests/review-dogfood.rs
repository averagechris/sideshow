#![cfg(unix)]

use sideshow::review::{
    NewReviewAnnotation, ReviewAction, ReviewDisposition, ReviewFreshness, ReviewKind,
    ReviewMutation, ReviewRepository, ReviewState, ReviewTarget,
};
use std::{
    fs,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

struct Server(Child);

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn fake_tailwind(root: &Path) -> PathBuf {
    let path = root.join("tailwindcss");
    fs::write(
        &path,
        "#!/bin/sh\nin=''\nout=''\nwhile [ $# -gt 0 ]; do\n  case \"$1\" in\n    -i) in=$2; shift 2 ;;\n    -o) out=$2; shift 2 ;;\n    *) shift ;;\n  esac\ndone\ncp \"$in\" \"$out\"\n",
    )
    .unwrap();
    let mut permissions = fs::metadata(&path).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&path, permissions).unwrap();
    path
}

fn launch(deck: &Path, state: &Path, tailwind: &Path, port: u16) -> Server {
    let child = Command::new(env!("CARGO_BIN_EXE_sideshow"))
        .args([
            "serve",
            deck.to_str().unwrap(),
            "--port",
            &port.to_string(),
            "--review",
        ])
        .env("XDG_STATE_HOME", state)
        .env("SIDESHOW_TAILWINDCSS", tailwind)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    Server(child)
}

fn wait_for<T>(label: &str, mut check: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < deadline {
        if let Some(value) = check() {
            return value;
        }
        thread::sleep(Duration::from_millis(100));
    }
    panic!("timed out waiting for {label}");
}

fn review_cli(state: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_sideshow"));
    command.env("XDG_STATE_HOME", state).arg("review");
    command
}

fn http_get(port: u16, path: &str) -> Option<String> {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).ok()?;
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: localhost:{port}\r\nConnection: close\r\n\r\n"
    )
    .ok()?;
    let mut response = String::new();
    stream.read_to_string(&mut response).ok()?;
    response.starts_with("HTTP/1.1 200 OK").then_some(response)
}

#[test]
fn persisted_review_survives_restart_rebuild_and_explicit_handoff() {
    let tmp = tempfile::tempdir().unwrap();
    let deck = tmp.path().join("deck");
    let state = tmp.path().join("state");
    fs::create_dir_all(deck.join("slides")).unwrap();
    fs::create_dir(&state).unwrap();
    fs::write(deck.join("deck.toml"), "[deck]\ntitle='Dogfood'\n").unwrap();
    fs::write(deck.join("theme.css"), ":root { color: black; }\n").unwrap();
    fs::write(deck.join("slides/01-title.html"), "<h1>Before</h1>\n").unwrap();
    let tailwind = fake_tailwind(tmp.path());
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let repository = ReviewRepository::with_state_root(&deck, &state).unwrap();

    let first_server = launch(&deck, &state, &tailwind, port);
    let initial = wait_for("initial build manifest", || {
        repository
            .load_artifact()
            .ok()
            .filter(|artifact| artifact.build.is_some())
    });
    let created = repository
        .apply_mutation(ReviewMutation::Create {
            revision: initial.revision,
            annotation: NewReviewAnnotation {
                slide_id: "s-01-title".into(),
                source_path: "slides/01-title.html".into(),
                target: ReviewTarget::Region {
                    x: 100.0,
                    y: 100.0,
                    width: 300.0,
                    height: 120.0,
                    selector_hint: Some("h1".into()),
                    text_hint: Some("Before".into()),
                },
                body: "Make the title concrete".into(),
                kind: ReviewKind::Issue,
                action: Some(ReviewAction::Fix),
            },
        })
        .unwrap();
    let annotation_id = created.annotations[0].id.clone();
    let initial_build_id = created.build.as_ref().unwrap().build_id.clone();
    assert_eq!(created.annotations[0].freshness, ReviewFreshness::Current);
    drop(first_server);

    let second_server = launch(&deck, &state, &tailwind, port);
    wait_for("restarted server", || http_get(port, "/").map(|_| ()));
    let restarted = wait_for("restart artifact", || {
        repository
            .load_artifact()
            .ok()
            .filter(|artifact| artifact.annotations.len() == 1)
    });
    assert_eq!(restarted.revision, created.revision);
    assert_eq!(restarted.annotations[0].state, ReviewState::Todo);
    assert_eq!(restarted.annotations[0].freshness, ReviewFreshness::Current);

    fs::write(deck.join("slides/01-title.html"), "<h1>After</h1>\n").unwrap();
    let rebuilt = wait_for("watcher manifest refresh", || {
        repository.load_artifact().ok().filter(|artifact| {
            artifact
                .build
                .as_ref()
                .is_some_and(|build| build.build_id != initial_build_id)
                && artifact
                    .annotations
                    .first()
                    .is_some_and(|annotation| annotation.freshness == ReviewFreshness::Stale)
        })
    });
    assert_eq!(rebuilt.annotations.len(), 1);
    assert_eq!(rebuilt.annotations[0].state, ReviewState::Todo);
    assert_eq!(rebuilt.annotations[0].freshness, ReviewFreshness::Stale);

    let stale_build_id = rebuilt.build.as_ref().unwrap().build_id.clone();
    fs::rename(
        deck.join("slides/01-title.html"),
        deck.join("slides/02-replacement.html"),
    )
    .unwrap();
    let orphaned = wait_for("orphan manifest refresh", || {
        repository.load_artifact().ok().filter(|artifact| {
            artifact
                .build
                .as_ref()
                .is_some_and(|build| build.build_id != stale_build_id)
                && artifact
                    .annotations
                    .first()
                    .is_some_and(|annotation| annotation.freshness == ReviewFreshness::Orphaned)
        })
    });
    assert_eq!(orphaned.annotations.len(), 1);
    assert_eq!(orphaned.annotations[0].state, ReviewState::Todo);
    assert_eq!(orphaned.annotations[0].freshness, ReviewFreshness::Orphaned);

    let json_export = tmp.path().join("review-export.json");
    let json = review_cli(&state)
        .args([
            "export",
            deck.to_str().unwrap(),
            "--format",
            "json",
            "--output",
            json_export.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        json.status.success(),
        "{}",
        String::from_utf8_lossy(&json.stderr)
    );
    let exported: sideshow::review::ReviewArtifact =
        serde_json::from_slice(&fs::read(&json_export).unwrap()).unwrap();
    assert_eq!(exported, orphaned);

    let markdown_export = tmp.path().join("review-export.md");
    let markdown = review_cli(&state)
        .args([
            "export",
            deck.to_str().unwrap(),
            "--format",
            "markdown",
            "--output",
            markdown_export.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        markdown.status.success(),
        "{}",
        String::from_utf8_lossy(&markdown.stderr)
    );
    let handoff = fs::read_to_string(&markdown_export).unwrap();
    assert!(handoff.contains("slides/01-title.html"));
    assert!(handoff.contains("Freshness: `Orphaned`"));
    assert!(handoff.contains("Selector hint: ` h1 `"));
    assert!(handoff.contains("Text hint: ` Before `"));
    assert!(handoff.contains("sideshow check"));
    assert!(handoff.contains("sideshow build"));

    let disposition = review_cli(&state)
        .args([
            "disposition",
            deck.to_str().unwrap(),
            &annotation_id,
            "--status",
            "addressed",
            "--note",
            "Source updated; verify the new title",
            "--revision",
            &orphaned.revision.to_string(),
        ])
        .output()
        .unwrap();
    assert!(
        disposition.status.success(),
        "{}",
        String::from_utf8_lossy(&disposition.stderr)
    );
    let dispositioned: sideshow::review::ReviewArtifact =
        serde_json::from_slice(&disposition.stdout).unwrap();
    let resolve = review_cli(&state)
        .args([
            "resolve",
            deck.to_str().unwrap(),
            &annotation_id,
            "--revision",
            &dispositioned.revision.to_string(),
        ])
        .output()
        .unwrap();
    assert!(
        resolve.status.success(),
        "{}",
        String::from_utf8_lossy(&resolve.stderr)
    );
    let resolved: sideshow::review::ReviewArtifact =
        serde_json::from_slice(&resolve.stdout).unwrap();
    assert_eq!(resolved.annotations[0].state, ReviewState::Resolved);
    assert_eq!(resolved.annotations[0].freshness, ReviewFreshness::Orphaned);
    assert_eq!(
        resolved.annotations[0].disposition,
        ReviewDisposition::Addressed
    );
    let built_html = fs::read_to_string(deck.join("dist/dogfood.html")).unwrap();
    assert!(built_html.contains("<h1>After</h1>"));
    for sentinel in [
        "Make the title concrete",
        "schema_version",
        "sideshow-review-style",
        "sideshow-review-script",
        "data-nonce",
        "/__sideshow/review",
        repository.artifact_path().to_str().unwrap(),
        state.to_str().unwrap(),
        "Sideshow review handoff",
        "Source updated; verify the new title",
    ] {
        assert!(!built_html.contains(sentinel), "leaked {sentinel}");
    }

    // Review persistence is ancillary: a successful watcher build must still update the served
    // output and reload generation when the XDG artifact cannot be replaced.
    let generation_before = http_get(port, "/__sideshow/reload")
        .unwrap()
        .split("\r\n\r\n")
        .nth(1)
        .unwrap()
        .trim()
        .to_owned();
    let reviews_dir = repository.artifact_path().parent().unwrap();
    let mut permissions = fs::metadata(reviews_dir).unwrap().permissions();
    let original_mode = permissions.mode();
    permissions.set_mode(0o500);
    fs::set_permissions(reviews_dir, permissions).unwrap();
    fs::write(
        deck.join("deck.toml"),
        "[deck]\ntitle='Dogfood Published'\n",
    )
    .unwrap();
    fs::write(
        deck.join("slides/02-replacement.html"),
        "<h1>Published despite review failure</h1>\n",
    )
    .unwrap();
    let published = wait_for("published rebuild after review failure", || {
        let root = http_get(port, "/")?;
        let generation = http_get(port, "/__sideshow/reload")?
            .split("\r\n\r\n")
            .nth(1)?
            .trim()
            .to_owned();
        (root.contains("Published despite review failure") && generation != generation_before)
            .then_some(root)
    });
    assert!(published.contains("Dogfood Published"));
    let mut permissions = fs::metadata(reviews_dir).unwrap().permissions();
    permissions.set_mode(original_mode);
    fs::set_permissions(reviews_dir, permissions).unwrap();
    drop(second_server);
}
