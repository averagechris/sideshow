use std::process::Command;

fn sideshow() -> Command {
    Command::new(env!("CARGO_BIN_EXE_sideshow"))
}

fn new_plan_deck() -> (tempfile::TempDir, std::path::PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    let deck = temp.path().join("plan");
    let status = sideshow()
        .args(["plan", "new"])
        .arg(&deck)
        .status()
        .unwrap();
    assert!(status.success());
    (temp, deck)
}

fn mutate(deck: &std::path::Path, args: &[&str]) -> std::process::Output {
    sideshow()
        .args(["plan", "mutate"])
        .arg(deck)
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn typed_plan_record_crud_serializes_enums_and_preserves_order() {
    let (_temp, deck) = new_plan_deck();

    let out = mutate(
        &deck,
        &[
            "update-plan",
            "--title",
            "Typed plan",
            "--status",
            "in-review",
            "--objective",
            "Exercise typed mutations",
        ],
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["status"], "in_review");
    assert_eq!(out.stdout, std::fs::read(deck.join("plan.json")).unwrap());

    for args in [
        vec![
            "add-constraint",
            "--id",
            "constraint-z",
            "--description",
            "Zed",
        ],
        vec![
            "update-constraint",
            "--id",
            "constraint-z",
            "--description",
            "Zed updated",
        ],
        vec![
            "add-decision",
            "--id",
            "decision-z",
            "--title",
            "Pick Z",
            "--status",
            "deferred",
            "--rationale",
            "Waiting",
        ],
        vec![
            "update-decision",
            "--id",
            "decision-z",
            "--title",
            "Pick Z",
            "--status",
            "superseded",
            "--rationale",
            "Replaced",
        ],
        vec![
            "add-risk",
            "--id",
            "risk-z",
            "--description",
            "Z risk",
            "--likelihood",
            "low",
            "--impact",
            "high",
            "--mitigation",
            "Watch it",
        ],
        vec![
            "update-risk",
            "--id",
            "risk-z",
            "--description",
            "Z risk updated",
            "--likelihood",
            "medium",
            "--impact",
            "medium",
            "--mitigation",
            "Monitor",
        ],
    ] {
        let out = mutate(&deck, &args);
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(out.stdout, std::fs::read(deck.join("plan.json")).unwrap());
    }

    let value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(deck.join("plan.json")).unwrap()).unwrap();
    assert_eq!(value["constraints"][2]["id"], "constraint-z");
    assert_eq!(value["decisions"][2]["status"], "superseded");
    assert_eq!(value["risks"][1]["likelihood"], "medium");
    assert_eq!(value["risks"][1]["impact"], "medium");

    for args in [
        vec!["remove-constraint", "--id", "constraint-z"],
        vec!["remove-decision", "--id", "decision-z"],
        vec!["remove-risk", "--id", "risk-z"],
    ] {
        let out = mutate(&deck, &args);
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(out.stdout, std::fs::read(deck.join("plan.json")).unwrap());
    }
}

#[test]
fn typed_plan_record_mutations_reject_duplicates_unknowns_invalids_and_blanks_without_writing() {
    let (_temp, deck) = new_plan_deck();
    for args in [
        vec![
            "add-constraint",
            "--id",
            "task-align",
            "--description",
            "dup",
        ],
        vec!["update-constraint", "--id", "missing", "--description", "x"],
        vec!["remove-decision", "--id", "missing"],
        vec![
            "add-risk",
            "--id",
            "risk-bad",
            "--description",
            " ",
            "--likelihood",
            "low",
            "--impact",
            "high",
            "--mitigation",
            "m",
        ],
        vec![
            "add-decision",
            "--id",
            "decision-bad",
            "--title",
            " ",
            "--status",
            "accepted",
            "--rationale",
            "r",
        ],
        vec![
            "update-plan",
            "--title",
            " ",
            "--status",
            "draft",
            "--objective",
            "objective",
        ],
    ] {
        let before = std::fs::read(deck.join("plan.json")).unwrap();
        let out = mutate(&deck, &args);
        assert!(!out.status.success(), "unexpected success for {args:?}");
        assert_eq!(before, std::fs::read(deck.join("plan.json")).unwrap());
    }
}

#[test]
fn bindable_components_still_resolve_newly_mutated_bound_records() {
    let (_temp, deck) = new_plan_deck();
    let out = mutate(
        &deck,
        &[
            "add-decision",
            "--id",
            "decision-bind",
            "--title",
            "Bindable",
            "--status",
            "accepted",
            "--rationale",
            "Component compatibility",
        ],
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let source = deck.join("slides/10-bound.slide.toml");
    let out = sideshow()
        .args(["compose", "add"])
        .arg(&source)
        .args([
            "--component",
            "plan-record-card",
            "--bind-kind",
            "decision",
            "--bind-id",
            "decision-bind",
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}
