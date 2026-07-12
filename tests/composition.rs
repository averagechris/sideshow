use std::process::Command;

fn sideshow() -> Command {
    Command::new(env!("CARGO_BIN_EXE_sideshow"))
}

fn build_output(deck: &std::path::Path) -> String {
    let status = sideshow().args(["build"]).arg(deck).status().unwrap();
    assert!(status.success());
    let path = std::fs::read_dir(deck.join("dist"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| path.extension().and_then(|ext| ext.to_str()) == Some("html"))
        .unwrap();
    std::fs::read_to_string(path).unwrap()
}

fn deck() -> tempfile::TempDir {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    std::fs::create_dir(root.join("slides")).unwrap();
    std::fs::create_dir(root.join("assets")).unwrap();
    std::fs::write(root.join("deck.toml"), "[deck]\ntitle = \"Test Deck\"\n").unwrap();
    std::fs::write(root.join("theme.css"), "").unwrap();
    temp
}

#[test]
fn literal_component_escapes_and_preserves_raw_compatibility() {
    let temp = deck();
    let root = temp.path();
    std::fs::write(root.join("slides/00.html"), "<h1>Raw OK</h1>").unwrap();
    let out = sideshow()
        .args(["compose", "add"])
        .arg(root.join("slides/01.slide.toml"))
        .args([
            "--component",
            "literal-card",
            "--prop",
            "eyebrow=<bad \"quoted\" & risky>",
            "--prop",
            "title=A & B",
            "--prop",
            "body=<script data-x=\"&\">alert(1)</script>",
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let html = build_output(root);
    assert!(html.contains("<h1>Raw OK</h1>"));
    assert!(html.contains("A &amp; B"));
    assert!(html.contains("&lt;script data-x=\"&amp;\"&gt;alert(1)&lt;/script&gt;"));
    assert!(!html.contains("<script>alert(1)</script>"));
    assert!(html.contains("<article class=\"plan-card\">"));
    assert!(!html.contains("&lt;article class=\"plan-card"));
    assert!(!html.contains("component-card"));
    assert!(!html.contains("<script data-x"));
    let second = build_output(root);
    assert_eq!(html, second);
}

#[test]
fn plan_bound_component_anchors_and_rejects_unknowns() {
    let temp = tempfile::tempdir().unwrap();
    let deck = temp.path().join("plan");
    assert!(
        sideshow()
            .args(["plan", "new"])
            .arg(&deck)
            .status()
            .unwrap()
            .success()
    );
    std::fs::write(
        deck.join("slides/04.slide.toml"),
        "component = \"plan-record-card\"\nprops.label = \"Outcome <x>\"\n[bind]\nkind = \"outcome\"\nid = \"outcome-alignment\"\n",
    )
    .unwrap();
    let plan_path = deck.join("plan.json");
    let mut plan: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&plan_path).unwrap()).unwrap();
    plan["outcomes"][0]["description"] =
        serde_json::json!("Plan says <script data-x=\"&\">bad</script> & stays text");
    std::fs::write(&plan_path, serde_json::to_string_pretty(&plan).unwrap()).unwrap();
    let html = build_output(&deck);
    assert!(html.contains("data-plan-kind=\"outcome\" data-plan-id=\"outcome-alignment\""));
    assert!(html.contains("Outcome &lt;x&gt;"));
    assert!(
        html.contains(
            "Plan says &lt;script data-x=\"&amp;\"&gt;bad&lt;/script&gt; &amp; stays text"
        )
    );
    assert!(html.contains("--plan-bg"));

    std::fs::write(
        deck.join("slides/05.slide.toml"),
        "component = \"plan-record-card\"\n[bind]\nkind = \"outcome\"\nid = \"missing\"\n",
    )
    .unwrap();
    assert!(
        !sideshow()
            .args(["build"])
            .arg(&deck)
            .status()
            .unwrap()
            .success()
    );

    std::fs::write(
        deck.join("slides/05.slide.toml"),
        "component = \"literal-card\"\nprops.nope = \"x\"\n",
    )
    .unwrap();
    assert!(
        !sideshow()
            .args(["build"])
            .arg(&deck)
            .status()
            .unwrap()
            .success()
    );
}

#[test]
fn compose_add_update_remove_have_safe_file_semantics() {
    let temp = deck();
    let root = temp.path();
    let source = root.join("slides/01.slide.toml");
    assert!(
        sideshow()
            .args(["compose", "add"])
            .arg(&source)
            .args([
                "--component",
                "literal-card",
                "--prop",
                "eyebrow=One",
                "--prop",
                "title=Two",
                "--prop",
                "body=Three"
            ])
            .status()
            .unwrap()
            .success()
    );
    let original = std::fs::read(&source).unwrap();
    assert!(
        !sideshow()
            .args(["compose", "add"])
            .arg(&source)
            .args([
                "--component",
                "literal-card",
                "--prop",
                "eyebrow=X",
                "--prop",
                "title=Y",
                "--prop",
                "body=Z"
            ])
            .status()
            .unwrap()
            .success()
    );
    assert_eq!(original, std::fs::read(&source).unwrap());
    assert!(
        !sideshow()
            .args(["compose", "update"])
            .arg(root.join("slides/missing.slide.toml"))
            .args([
                "--component",
                "literal-card",
                "--prop",
                "eyebrow=X",
                "--prop",
                "title=Y",
                "--prop",
                "body=Z"
            ])
            .status()
            .unwrap()
            .success()
    );
    assert!(
        !sideshow()
            .args(["compose", "update"])
            .arg(&source)
            .args(["--component", "literal-card", "--prop", "nope=X"])
            .status()
            .unwrap()
            .success()
    );
    assert_eq!(original, std::fs::read(&source).unwrap());
    assert!(
        !sideshow()
            .args(["compose", "add"])
            .arg(root.join("slides/not-toml.html"))
            .args([
                "--component",
                "literal-card",
                "--prop",
                "eyebrow=X",
                "--prop",
                "title=Y",
                "--prop",
                "body=Z"
            ])
            .status()
            .unwrap()
            .success()
    );
    let removed = sideshow()
        .args(["compose", "remove"])
        .arg(&source)
        .output()
        .unwrap();
    assert!(removed.status.success());
    let value: serde_json::Value = serde_json::from_slice(&removed.stdout).unwrap();
    assert_eq!(value["removed"], source.to_string_lossy().as_ref());
    assert!(!source.exists());
}

#[test]
fn compose_writes_require_complete_valid_structure_and_preserve_bytes() {
    let temp = deck();
    let root = temp.path();
    let source = root.join("slides/01.slide.toml");
    for args in [
        vec![
            "--component",
            "literal-card",
            "--prop",
            "eyebrow=missing title/body",
        ],
        vec![
            "--component",
            "literal-card",
            "--prop",
            "eyebrow=e",
            "--prop",
            "title=t",
            "--prop",
            "body=b",
            "--bind-kind",
            "outcome",
            "--bind-id",
            "o1",
        ],
        vec!["--component", "plan-record-card", "--prop", "label=x"],
        vec![
            "--component",
            "plan-record-card",
            "--prop",
            "label=x",
            "--bind-kind",
            "bogus",
            "--bind-id",
            "o1",
        ],
    ] {
        assert!(
            !sideshow()
                .args(["compose", "add"])
                .arg(&source)
                .args(args)
                .status()
                .unwrap()
                .success()
        );
        assert!(!source.exists());
    }

    assert!(
        sideshow()
            .args(["compose", "add"])
            .arg(&source)
            .args([
                "--component",
                "literal-card",
                "--prop",
                "eyebrow=e",
                "--prop",
                "title={{body}} literal braces",
                "--prop",
                "body=b"
            ])
            .status()
            .unwrap()
            .success()
    );
    let original = std::fs::read(&source).unwrap();
    assert!(
        !sideshow()
            .args(["compose", "update"])
            .arg(&source)
            .args(["--component", "plan-record-card", "--prop", "label=x"])
            .status()
            .unwrap()
            .success()
    );
    assert_eq!(original, std::fs::read(&source).unwrap());

    let explain = sideshow()
        .args(["compose", "explain"])
        .arg(&source)
        .output()
        .unwrap();
    assert!(explain.status.success());
    let value: serde_json::Value = serde_json::from_slice(&explain.stdout).unwrap();
    assert_eq!(value["props"]["title"], "{{body}} literal braces");
    let html = build_output(root);
    assert!(html.contains("{{body}} literal braces"));
}
