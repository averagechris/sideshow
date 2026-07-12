use std::process::Command;

fn sideshow() -> Command {
    Command::new(env!("CARGO_BIN_EXE_sideshow"))
}

fn assert_help(args: &[&str]) {
    let output = sideshow().args(args).arg("--help").output().unwrap();
    assert!(
        output.status.success(),
        "help failed for {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn deck_author_skill_references_current_cli_contracts() {
    let skill = include_str!("../skills/sideshow-deck-author/SKILL.md");

    for phrase in [
        "sideshow registry list",
        "sideshow registry explain theme signal",
        "sideshow registry sources",
        "sideshow registry apply-theme --deck mydeck local-theme --force",
        "sideshow compose add",
        "sideshow compose update",
        "sideshow compose explain",
        "sideshow compose remove",
        "sideshow plan mutate my-plan add-outcome",
        "sideshow plan mutate my-plan add-workstream",
        "sideshow plan mutate my-plan update-task",
        "sideshow plan check my-plan --strict --format json",
    ] {
        assert!(
            skill.contains(phrase),
            "skill lost command phrase: {phrase}"
        );
    }

    for args in [
        vec!["registry"],
        vec!["registry", "list"],
        vec!["registry", "explain"],
        vec!["registry", "sources"],
        vec!["registry", "apply-theme"],
        vec!["compose"],
        vec!["compose", "add"],
        vec!["compose", "update"],
        vec!["compose", "explain"],
        vec!["compose", "remove"],
        vec!["plan", "mutate"],
        vec!["plan", "mutate", "my-plan", "add-outcome"],
        vec!["plan", "mutate", "my-plan", "add-workstream"],
        vec!["plan", "mutate", "my-plan", "update-task"],
    ] {
        assert_help(&args);
    }
}

#[test]
fn deck_author_skill_names_available_bundled_registry_entries() {
    let skill = include_str!("../skills/sideshow-deck-author/SKILL.md");
    let output = sideshow().args(["registry", "list"]).output().unwrap();
    assert!(output.status.success());
    let registry: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let entries = registry["entries"].as_array().unwrap();

    for (kind, name) in [
        ("theme", "ledger"),
        ("theme", "poster"),
        ("theme", "signal"),
        ("theme", "terminal"),
        ("component", "literal-card"),
        ("component", "plan-primitives"),
        ("component", "plan-record-card"),
    ] {
        assert!(skill.contains(name), "skill no longer names {kind}/{name}");
        let entry = entries
            .iter()
            .find(|entry| entry["kind"] == kind && entry["name"] == name)
            .unwrap_or_else(|| panic!("registry missing {kind}/{name}"));
        assert_eq!(entry["provenance"]["source"], "bundled");
    }
}
