use std::process::Command;

fn review_command(state: &std::path::Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_sideshow"));
    command.env("XDG_STATE_HOME", state);
    command.arg("review");
    command
}

fn sideshow_command() -> Command {
    Command::new(env!("CARGO_BIN_EXE_sideshow"))
}

#[test]
fn plan_new_check_and_export_are_agent_consumable() {
    let temp = tempfile::tempdir().unwrap();
    let deck = temp.path().join("plan");
    let status = sideshow_command()
        .args(["plan", "new"])
        .arg(&deck)
        .status()
        .unwrap();
    assert!(status.success());
    assert!(
        std::fs::read_to_string(deck.join("deck.toml"))
            .unwrap()
            .contains("title = \"Plan\"")
    );
    assert!(!deck.join("slides/02-content.md").exists());

    let plan_path = deck.join("plan.json");
    let mut authored: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&plan_path).unwrap()).unwrap();
    let exact_command = "  cargo test plan_ --test cli && printf '%s' \"$HOME\"  ";
    authored["workstreams"][0]["tasks"][0]["verification"]["commands"][0] =
        serde_json::json!(exact_command);
    std::fs::write(&plan_path, serde_json::to_vec_pretty(&authored).unwrap()).unwrap();

    let status = sideshow_command()
        .args(["plan", "check", "--format", "json", "--strict"])
        .arg(&deck)
        .status()
        .unwrap();
    assert!(status.success());

    let json = sideshow_command()
        .args(["plan", "export", "--format", "json"])
        .arg(&deck)
        .output()
        .unwrap();
    assert!(json.status.success());
    let value: serde_json::Value = serde_json::from_slice(&json.stdout).unwrap();
    assert_eq!(value["schema_version"], 2);
    assert_eq!(value["status"], "draft");
    assert!(
        value["objective"]
            .as_str()
            .unwrap()
            .contains("framing scope")
    );
    assert!(
        value["objective"]
            .as_str()
            .unwrap()
            .contains("planning digest")
    );
    assert_eq!(value["outcomes"][1]["id"], "outcome-proposal");
    assert_eq!(value["outcomes"][2]["id"], "outcome-reviewed-digest");
    assert_eq!(value["workstreams"][0]["id"], "ws-alignment");
    assert_eq!(value["workstreams"][0]["owner"], "planning-agent");
    assert_eq!(value["workstreams"][0]["tasks"][1]["id"], "task-explore");
    assert_eq!(
        value["workstreams"][0]["tasks"][2]["id"],
        "task-refine-digest"
    );
    assert_eq!(
        value["workstreams"][0]["tasks"][1]["files"],
        serde_json::json!(["plan.json", "slides/"])
    );
    assert_eq!(
        value["workstreams"][0]["tasks"][2]["files"],
        serde_json::json!(["plan.json", "slides/"])
    );
    assert_eq!(
        value["outcomes"][2]["proof"][1],
        "Markdown digest preserves verification intent without claiming execution status"
    );
    assert_eq!(
        value["workstreams"][0]["tasks"][2]["verification"]["commands"],
        serde_json::json!([
            "sideshow plan check . --strict",
            "sideshow build .",
            "sideshow plan export . --format markdown"
        ])
    );
    let json_text = serde_json::to_string(&value).unwrap();
    for obsolete in [
        "implementation-agent",
        "Delivery sequence",
        "execute it in dependency order",
        "implementation evidence",
        "Review before execution",
        "notes/",
        "planning-digest.md",
    ] {
        assert!(
            !json_text.contains(obsolete),
            "obsolete scaffold phrase: {obsolete}"
        );
    }
    assert!(!json_text.contains("\"dist/\""));
    let task = &value["workstreams"][0]["tasks"][0];
    assert_eq!(task["id"], "task-align");
    assert_eq!(
        task["verification"]["intent"],
        "Confirm the planning contract is internally consistent before proposal exploration starts."
    );
    assert_eq!(task["verification"]["commands"][0], exact_command);
    assert!(task.get("verification_commands").is_none());

    let md = sideshow_command()
        .args(["plan", "export", "--format", "markdown"])
        .arg(&deck)
        .output()
        .unwrap();
    assert!(md.status.success());
    let md = String::from_utf8(md.stdout).unwrap();
    assert!(md.contains("task-align"));
    assert!(md.contains("outcome-alignment"));
    assert!(md.contains("planning-agent"));
    assert!(md.contains("## Proposed work"));
    assert!(md.contains("planning digest"));
    assert!(!md.contains("## Execution"));
    assert!(!md.contains("implementation-agent"));
    assert!(md.contains("Verification intent: Confirm the planning contract"));
    assert!(md.contains("Agent commands:"));
    assert!(md.contains("sideshow plan export . --format markdown"));
    assert!(md.contains("Markdown digest preserves verification intent"));
    assert!(md.contains("- Files: plan.json, slides/"));
    assert!(!md.contains("notes/"));
    assert!(!md.contains("planning-digest.md"));
    assert!(!md.contains("- Files: plan.json, dist/"));

    let slide_text = [
        "slides/01-title.html",
        "slides/02-plan.html",
        "slides/03-verify.html",
    ]
    .into_iter()
    .map(|path| std::fs::read_to_string(deck.join(path)).unwrap())
    .collect::<Vec<_>>()
    .join("\n");
    for id in [
        "outcome-alignment",
        "outcome-proposal",
        "outcome-reviewed-digest",
        "ws-alignment",
        "task-align",
        "task-explore",
        "task-refine-digest",
        "constraint-authored-projection",
        "constraint-strict-clean",
        "decision-json-canonical",
        "decision-verify-intent",
        "risk-drift",
    ] {
        assert!(
            slide_text.contains(&format!("data-plan-id=\"{id}\"")),
            "missing slide anchor for {id}"
        );
    }
    assert!(slide_text.contains("Frame → explore → refine digest"));
    assert!(slide_text.contains("tracker-neutral"));
    assert!(slide_text.contains("reviewed planning digest"));
    for obsolete in [
        "implementation-agent",
        "Delivery sequence",
        "execute it in dependency order",
        "implementation evidence",
        "Review before execution",
    ] {
        assert!(
            !slide_text.contains(obsolete),
            "obsolete slide phrase: {obsolete}"
        );
    }

    let overwrite = sideshow_command()
        .args(["plan", "new"])
        .arg(&deck)
        .output()
        .unwrap();
    assert!(!overwrite.status.success());
}

#[cfg(unix)]
#[test]
fn plan_scaffold_templates_escape_titles_and_build_once() {
    let temp = tempfile::tempdir().unwrap();
    let hostile = "<img src=x onerror=alert(1)><script>alert(2)</script>";
    let deck = temp.path().join("hostile-plan");
    assert!(
        sideshow_command()
            .args(["plan", "new"])
            .arg(&deck)
            .status()
            .unwrap()
            .success()
    );

    let title = std::fs::read_to_string(deck.join("slides/01-title.html")).unwrap();
    assert!(title.contains("<h1 class=\"plan-title\">Hostile Plan</h1>"));
    assert!(!title.contains("<script>"));
    assert!(!title.contains("onerror="));
    assert_eq!(std::fs::read_dir(deck.join("slides")).unwrap().count(), 3);

    std::fs::write(
        deck.join("deck.toml"),
        format!("[deck]\ntitle = {:?}\n", hostile),
    )
    .unwrap();

    let tw = fake_tailwind(temp.path());
    let config = temp.path().join("config.toml");
    std::fs::write(
        &config,
        format!("[tools]\ntailwindcss = '{}'\n", tw.display()),
    )
    .unwrap();
    let out = sideshow_command()
        .env("SIDESHOW_CONFIG", &config)
        .arg("build")
        .arg(&deck)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let html_path = std::fs::read_dir(deck.join("dist"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let html = std::fs::read_to_string(html_path).unwrap();
    assert!(html.starts_with("<!doctype html>\n<html lang=\"en\">"));
    assert!(html.contains("data-runtime=\"sideshow-runtime-v1\""));
    assert!(html.contains("@import \"tailwindcss\" source(none);"));
    assert!(html.contains("(() => {"));
    assert!(html.contains("<main id=\"stage\" aria-live=\"polite\">"));
    assert_eq!(html.matches("id=\"s-01-title\"").count(), 1);
    assert_eq!(html.matches("<h1 class=\"plan-title\">").count(), 1);
    let title = &html[html.find("<title>").unwrap()..html.find("</title>").unwrap()];
    assert!(title.contains("img src=x onerror=alert(1)"));
    assert!(title.contains("script"));
    assert!(!html.contains("<img src=x"));
    assert!(!html.contains("<script>alert(2)</script>"));
    assert!(
        html.contains(
            "<section class=\"slide\" id=\"s-02-plan\" data-src=\"slides/02-plan.html\">"
        )
    );

    assert!(
        sideshow_command()
            .args(["plan", "check", "--strict"])
            .arg(&deck)
            .status()
            .unwrap()
            .success()
    );
    assert!(
        sideshow_command()
            .args(["plan", "export", "--format", "markdown"])
            .arg(&deck)
            .status()
            .unwrap()
            .success()
    );
    assert!(
        sideshow_command()
            .args(["plan", "export", "--format", "json"])
            .arg(&deck)
            .status()
            .unwrap()
            .success()
    );
}

#[test]
fn plan_markdown_export_appends_tracker_neutral_issue_packets() {
    let temp = tempfile::tempdir().unwrap();
    let deck = temp.path().join("plan");
    assert!(
        sideshow_command()
            .args(["plan", "new"])
            .arg(&deck)
            .status()
            .unwrap()
            .success()
    );

    let plan_path = deck.join("plan.json");
    let mut plan: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&plan_path).unwrap()).unwrap();
    let awkward = " printf 'one two' && cargo test -- --exact 'name with spaces' # keep $HOME `literal` \nnext line with ``` and ```` backticks\ntrailing space follows ";
    plan["workstreams"][0]["tasks"][0]["verification"]["commands"] =
        serde_json::json!([awkward, "second command --flag='still exact'"]);
    plan["workstreams"][0]["tasks"][1]["dependencies"] = serde_json::json!(["task-align"]);
    plan["workstreams"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({
            "id": "ws-parallel",
            "title": "Parallel lane",
            "status": "todo",
            "owner": "parallel-owner",
            "tasks": [{
                "id": "task-parallel",
                "title": "Start without dependencies",
                "status": "todo",
                "owner": "parallel-owner",
                "outcomes": ["outcome-alignment"],
                "dependencies": [],
                "files": ["src/main.rs"],
                "acceptance_checks": ["Packet explains parallel-start semantics."],
                "verification": {
                    "intent": "Show no-dependency work can be drafted independently.",
                    "commands": ["cargo test plan_ --test cli"]
                }
            }]
        }));
    std::fs::write(&plan_path, serde_json::to_vec_pretty(&plan).unwrap()).unwrap();

    let json = sideshow_command()
        .args(["plan", "export", "--format", "json"])
        .arg(&deck)
        .output()
        .unwrap();
    assert!(json.status.success());
    let exported: serde_json::Value = serde_json::from_slice(&json.stdout).unwrap();
    assert_eq!(exported["schema_version"], 2);
    assert!(exported.get("issue_packets").is_none());
    assert!(exported.get("manual_issue_drafting_packets").is_none());
    assert!(
        exported
            .get("proposed_decomposition_and_dependency_overview")
            .is_none()
    );
    assert!(
        exported["workstreams"][0]["tasks"][0]
            .get("resolved_outcomes")
            .is_none()
    );
    assert!(
        exported["workstreams"][0]["tasks"][0]
            .get("enables")
            .is_none()
    );
    assert!(
        exported["workstreams"][0]["tasks"][0]
            .get("derived_enables")
            .is_none()
    );
    assert_eq!(
        exported["workstreams"][0]["tasks"][0]["verification"]["commands"][0],
        awkward
    );

    let md_output = sideshow_command()
        .args(["plan", "export", "--format", "markdown"])
        .arg(&deck)
        .output()
        .unwrap();
    assert!(md_output.status.success());
    let md = String::from_utf8(md_output.stdout).unwrap();
    let md_again = sideshow_command()
        .args(["plan", "export", "--format", "markdown"])
        .arg(&deck)
        .output()
        .unwrap();
    assert!(md_again.status.success());
    assert_eq!(md.as_bytes(), md_again.stdout.as_slice());
    assert!(md.contains("## Objective\n"));
    let rendered = comrak::markdown_to_html(&md, &comrak::Options::default());
    assert!(rendered.contains("Exact commands (verbatim, authored order):"));
    assert!(rendered.contains("<code class=\"language-sh\">"));
    assert!(md.contains("## Risks\n"));
    assert!(md.contains("## Manual issue-drafting packets"));
    assert!(md.contains("Tracker-neutral disclaimer: these packets are source material"));
    assert!(md.contains("may be split or combined"));
    assert!(md.contains("translator chooses issue boundaries, labels, teams, priority, milestones, and tracker conventions"));
    assert!(md.contains("no live tracker state"));
    let disclaimer = md.find("Tracker-neutral disclaimer:").unwrap();
    let overview = md
        .find("### Proposed decomposition and dependency overview")
        .unwrap();
    let first_packet_heading = md.find("### Issue source packet:").unwrap();
    assert!(disclaimer < overview && overview < first_packet_heading);
    assert!(md.contains("- Derived dependency overview for manual drafting only; reverse `Enables` edges are not live tracker blockers, scheduling instructions, or JSON fields."));
    assert!(md.contains("- Workstream ws-alignment — Alignment proposal\n  - Task task-align — Frame scope, risks, and review criteria"));
    assert!(md.contains("    - Canonical source: plan.json → workstream ws-alignment → task task-align\n    - Depends on:\n      - none — root/parallel-start candidate\n    - Enables:\n      - task-explore — Explore proposal options"));
    assert!(md.contains("  - Task task-explore — Explore proposal options\n    - Canonical source: plan.json → workstream ws-alignment → task task-explore\n    - Depends on:\n      - task-align — Frame scope, risks, and review criteria\n    - Enables:\n      - task-refine-digest — Refine reviewed planning digest"));
    assert!(md.contains("  - Task task-refine-digest — Refine reviewed planning digest\n    - Canonical source: plan.json → workstream ws-alignment → task task-refine-digest\n    - Depends on:\n      - task-explore — Explore proposal options\n    - Enables:\n      - none"));
    assert!(md.contains("- Workstream ws-parallel — Parallel lane\n  - Task task-parallel — Start without dependencies\n    - Canonical source: plan.json → workstream ws-parallel → task task-parallel\n    - Depends on:\n      - none — root/parallel-start candidate\n    - Enables:\n      - none"));
    assert!(md.contains("- Objective: Align stakeholders on a focused, reviewable increment"));
    assert!(
        md.contains("- Canonical source: plan.json → workstream ws-alignment → task task-align")
    );
    assert!(md.contains("Use the plan-level constraints, non-goals, decisions, and risks above as the canonical source context"));

    let align = md
        .find("### Issue source packet: Frame scope, risks, and review criteria (task-align)")
        .unwrap();
    let implement = md
        .find("### Issue source packet: Explore proposal options (task-explore)")
        .unwrap();
    let parallel = md
        .find("### Issue source packet: Start without dependencies (task-parallel)")
        .unwrap();
    assert!(align < implement && implement < parallel);
    assert!(md.contains("- Workstream: ws-alignment — Alignment proposal"));
    assert!(md.contains("- Task: task-align — Frame scope, risks, and review criteria"));
    assert!(md.contains("- Proposed planning status: todo"));
    assert!(md.contains("- Proposed owner: planning-agent"));
    assert!(md.contains("- outcome-alignment: Readers understand the goal"));
    assert!(md.contains("    - Proof:\n      - slides/01-title.html"));
    assert!(md.contains("- task-align — Frame scope, risks, and review criteria"));
    assert!(md.contains("no dependencies means this task can start in parallel"));
    assert!(md.contains("- Files:\n  - src/main.rs"));
    assert!(md.contains("- Acceptance checks:\n  - Packet explains parallel-start semantics."));
    assert!(
        md.contains("- Verification intent: Show no-dependency work can be drafted independently.")
    );
    let align_packet = &md[align..implement];
    assert_command_fence_round_trips(align_packet, 1, awkward);
    assert_command_fence_round_trips(align_packet, 2, "second command --flag='still exact'");
    assert!(
        align_packet.find("Command 1:\n\n").unwrap() < align_packet.find("Command 2:\n\n").unwrap(),
        "authored command order should be preserved"
    );
    assert!(align_packet.contains("Exact commands (verbatim, authored order):\nCommand 1:\n\n"));
    assert!(!align_packet.contains("- Exact commands:\n  1."));
    assert!(!align_packet.contains("Command 1:\n\n    ```"));
}

#[test]
fn plan_markdown_export_hardens_hostile_authored_prose() {
    let temp = tempfile::tempdir().unwrap();
    let deck = temp.path().join("plan");
    assert!(
        sideshow_command()
            .args(["plan", "new"])
            .arg(&deck)
            .status()
            .unwrap()
            .success()
    );

    let plan_path = deck.join("plan.json");
    let original_json = std::fs::read(&plan_path).unwrap();
    let mut plan: serde_json::Value = serde_json::from_slice(&original_json).unwrap();
    let hostile = "visible # heading\n# heading\n- list\n+ list\n> quote\n1. ordered\n[link](https://example.test)|pipe `tick` & <script>alert(1)</script> <img src=x onerror=alert(1)>";
    plan["title"] = serde_json::json!(format!("Plan {hostile}"));
    plan["objective"] = serde_json::json!(format!("Objective\n{hostile}"));
    plan["outcomes"][0]["description"] = serde_json::json!(format!("Outcome {hostile}"));
    plan["outcomes"][0]["proof"][0] = serde_json::json!(format!("Proof\n{hostile}"));
    plan["constraints"][0]["description"] = serde_json::json!(format!("Constraint {hostile}"));
    plan["non_goals"][0] = serde_json::json!(format!("Non-goal\n{hostile}"));
    plan["workstreams"][0]["title"] = serde_json::json!(format!("Workstream {hostile}"));
    plan["workstreams"][0]["owner"] = serde_json::json!(format!("Owner {hostile}"));
    plan["workstreams"][0]["tasks"][0]["title"] = serde_json::json!(format!("Task {hostile}"));
    plan["workstreams"][0]["tasks"][0]["owner"] =
        serde_json::json!(format!("Task owner {hostile}"));
    plan["workstreams"][0]["tasks"][0]["files"][0] =
        serde_json::json!(format!("src/lib.rs\n{hostile}"));
    plan["workstreams"][0]["tasks"][0]["acceptance_checks"][0] =
        serde_json::json!(format!("Acceptance\n{hostile}"));
    plan["workstreams"][0]["tasks"][0]["verification"]["intent"] =
        serde_json::json!(format!("Verify {hostile}"));
    let exact_command =
        " printf '<script inert>' && printf '` ``` ````'\n# authored comment\n  trailing  ";
    plan["workstreams"][0]["tasks"][0]["verification"]["commands"] =
        serde_json::json!([exact_command]);
    plan["decisions"][0]["title"] = serde_json::json!(format!("Decision {hostile}"));
    plan["decisions"][0]["rationale"] = serde_json::json!(format!("Rationale {hostile}"));
    plan["risks"][0]["description"] = serde_json::json!(format!("Risk {hostile}"));
    plan["risks"][0]["mitigation"] = serde_json::json!(format!("Mitigation {hostile}"));
    let authored_json = serde_json::to_vec_pretty(&plan).unwrap();
    std::fs::write(&plan_path, &authored_json).unwrap();

    let json = sideshow_command()
        .args(["plan", "export", "--format", "json"])
        .arg(&deck)
        .output()
        .unwrap();
    assert!(json.status.success());
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&json.stdout).unwrap(),
        plan
    );

    let md_output = sideshow_command()
        .args(["plan", "export", "--format", "markdown"])
        .arg(&deck)
        .output()
        .unwrap();
    assert!(md_output.status.success());
    let md = String::from_utf8(md_output.stdout).unwrap();
    let md_again = sideshow_command()
        .args(["plan", "export", "--format", "markdown"])
        .arg(&deck)
        .output()
        .unwrap();
    assert_eq!(md.as_bytes(), md_again.stdout.as_slice());

    let rendered = comrak::markdown_to_html(&md, &comrak::Options::default());
    assert!(rendered.contains("visible # heading"));
    assert!(rendered.contains("# heading"));
    assert!(rendered.contains("- list"));
    assert!(rendered.contains("+ list"));
    assert!(rendered.contains("&gt; quote"));
    assert!(rendered.contains("1. ordered"));
    assert!(rendered.contains("&lt;script&gt;alert(1)&lt;/script&gt;"));
    assert!(!rendered.contains("<script"));
    assert!(!rendered.contains("<img"));
    assert!(!rendered.contains("<h1>heading</h1>"));
    assert!(!rendered.contains("<li>list</li>"));
    assert!(!rendered.contains("<li>ordered</li>"));
    assert!(!rendered.contains("<blockquote>"));
    assert!(rendered.contains("Objective<br />\nvisible # heading<br />"));
    assert_eq!(md.matches("### Issue source packet:").count(), 3);
    assert_eq!(markdown_heading_count_outside_fences(&md, "# "), 1);
    assert_eq!(markdown_heading_count_outside_fences(&md, "## "), 8);
    assert!(md.contains("  - Acceptance"));
    assert!(md.contains("      visible # heading  \n      \\# heading  \n      \\- list  \n      \\+ list  \n      &gt; quote  \n      1\\. ordered"));
    assert!(md.contains("- Files:\n  - src/lib.rs  \n      visible"));

    let proposed_work = &md[md.find("## Proposed work").unwrap()..md.find("## Decisions").unwrap()];
    assert!(proposed_work.contains("Agent commands:\n\nCommand 1:\n\n"));
    assert_command_fence_round_trips(proposed_work, 1, exact_command);
    let first_packet = md.find("### Issue source packet:").unwrap();
    let second_packet = md[first_packet + 1..]
        .find("### Issue source packet:")
        .map(|i| first_packet + 1 + i)
        .unwrap();
    assert_command_fence_round_trips(&md[first_packet..second_packet], 1, exact_command);
}

fn markdown_heading_count_outside_fences(md: &str, prefix: &str) -> usize {
    let mut in_fence = false;
    md.lines()
        .filter(|line| {
            if line.starts_with("```") {
                in_fence = !in_fence;
                return false;
            }
            !in_fence && line.starts_with(prefix)
        })
        .count()
}

fn assert_command_fence_round_trips(packet: &str, number: usize, expected: &str) {
    let marker = format!("Command {number}:\n\n");
    let start = packet
        .find(&marker)
        .unwrap_or_else(|| panic!("missing {marker:?}"));
    let block = &packet[start + marker.len()..];
    let first_newline = block.find('\n').unwrap();
    let opening = &block[..first_newline];
    assert!(
        opening.ends_with("sh"),
        "opening fence should be sh: {opening:?}"
    );
    let fence = opening.strip_suffix("sh").unwrap();
    assert!(fence.chars().all(|ch| ch == '`'));
    assert!(fence.len() >= 3);
    let closing = format!("\n{fence}");
    let body = &block[first_newline + 1..];
    let end = body.find(&closing).unwrap();
    assert_eq!(&body[..end], expected);
    let longest_backtick_run = expected
        .split(|ch| ch != '`')
        .map(str::len)
        .max()
        .unwrap_or(0);
    assert!(fence.len() > longest_backtick_run);
}

#[test]
fn repository_planning_example_stays_strict_check_clean() {
    let example =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/planning-sideshow");
    let output = sideshow_command()
        .args(["plan", "check"])
        .arg(&example)
        .arg("--strict")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn plan_dogfood_markdown_export_includes_dependency_overview_chain() {
    let deck = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/planning-sideshow");
    let output = sideshow_command()
        .args(["plan", "export", "--format", "markdown"])
        .arg(&deck)
        .output()
        .unwrap();
    assert!(output.status.success());
    let md = String::from_utf8(output.stdout).unwrap();

    let disclaimer = md.find("Tracker-neutral disclaimer:").unwrap();
    let overview = md
        .find("### Proposed decomposition and dependency overview")
        .unwrap();
    let packet = md.find("### Issue source packet:").unwrap();
    assert!(disclaimer < overview && overview < packet);

    let model = md.find("Task task-model-canonical-plan").unwrap();
    let slides = md.find("Task task-author-expressive-slides").unwrap();
    let docs = md.find("Task task-document-repeatable-loop").unwrap();
    let digest = md.find("Task task-dogfood-manual-digest").unwrap();
    assert!(model < slides && slides < docs && docs < digest);
    assert!(md.contains("Task task-model-canonical-plan — Model the Phase 2 work as strict canonical plan data\n    - Canonical source: plan.json → workstream ws-verification-scaffold → task task-model-canonical-plan\n    - Depends on:\n      - none — root/parallel-start candidate\n    - Enables:\n      - task-author-expressive-slides — Author expressive anchored slides"));
    assert!(md.contains("Task task-author-expressive-slides — Author expressive anchored slides\n    - Canonical source: plan.json → workstream ws-visual-grammar → task task-author-expressive-slides\n    - Depends on:\n      - task-model-canonical-plan — Model the Phase 2 work as strict canonical plan data\n    - Enables:\n      - task-document-repeatable-loop — Document the repeatable local workflow"));
    assert!(md.contains("Task task-document-repeatable-loop — Document the repeatable local workflow\n    - Canonical source: plan.json → workstream ws-repository-example → task task-document-repeatable-loop\n    - Depends on:\n      - task-author-expressive-slides — Author expressive anchored slides\n    - Enables:\n      - task-dogfood-manual-digest — Dogfood the tracker-neutral planning digest increment"));
    assert!(md.contains("Task task-dogfood-manual-digest — Dogfood the tracker-neutral planning digest increment\n    - Canonical source: plan.json → workstream ws-repository-example → task task-dogfood-manual-digest\n    - Depends on:\n      - task-document-repeatable-loop — Document the repeatable local workflow\n    - Enables:\n      - none"));
}

#[test]
fn plan_check_rejects_malformed_verification_contract() {
    let temp = tempfile::tempdir().unwrap();
    let deck = temp.path().join("plan");
    assert!(
        sideshow_command()
            .args(["plan", "new"])
            .arg(&deck)
            .status()
            .unwrap()
            .success()
    );
    let path = deck.join("plan.json");
    let mut plan: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();

    plan["workstreams"][0]["tasks"][0]["verification"]["commands"] = serde_json::json!(["   "]);
    std::fs::write(&path, serde_json::to_vec_pretty(&plan).unwrap()).unwrap();
    let blank_command = sideshow_command()
        .args(["plan", "check"])
        .arg(&deck)
        .output()
        .unwrap();
    assert!(!blank_command.status.success());
    assert!(String::from_utf8_lossy(&blank_command.stdout).contains("verification.intent"));

    plan["workstreams"][0]["tasks"][0]
        .as_object_mut()
        .unwrap()
        .remove("verification");
    plan["workstreams"][0]["tasks"][0]["verification_commands"] =
        serde_json::json!(["cargo test plan_ --test cli"]);
    std::fs::write(&path, serde_json::to_vec_pretty(&plan).unwrap()).unwrap();
    let old_field = sideshow_command()
        .args(["plan", "check"])
        .arg(&deck)
        .output()
        .unwrap();
    assert!(!old_field.status.success());
    assert!(String::from_utf8_lossy(&old_field.stdout).contains("verification_commands"));
}

#[test]
fn plan_check_rejects_unknown_statuses_and_dependency_cycles() {
    let temp = tempfile::tempdir().unwrap();
    let deck = temp.path().join("plan");
    assert!(
        sideshow_command()
            .args(["plan", "new"])
            .arg(&deck)
            .status()
            .unwrap()
            .success()
    );
    let path = deck.join("plan.json");
    let mut plan: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    plan["status"] = serde_json::json!("looks_good_to_me");
    std::fs::write(&path, serde_json::to_vec_pretty(&plan).unwrap()).unwrap();
    let invalid_status = sideshow_command()
        .args(["plan", "check"])
        .arg(&deck)
        .output()
        .unwrap();
    assert!(!invalid_status.status.success());
    assert!(String::from_utf8_lossy(&invalid_status.stdout).contains("unknown variant"));

    plan["status"] = serde_json::json!("draft");
    plan["workstreams"][0]["tasks"][0]["dependencies"] = serde_json::json!(["task-align"]);
    std::fs::write(&path, serde_json::to_vec_pretty(&plan).unwrap()).unwrap();
    let cycle = sideshow_command()
        .args(["plan", "check"])
        .arg(&deck)
        .output()
        .unwrap();
    assert!(!cycle.status.success());
    assert!(String::from_utf8_lossy(&cycle.stdout).contains("dependency cycle"));
}

#[test]
fn plan_check_rejects_bad_references_and_export_refuses_overwrite() {
    let temp = tempfile::tempdir().unwrap();
    let deck = temp.path().join("plan");
    assert!(
        sideshow_command()
            .args(["plan", "new"])
            .arg(&deck)
            .status()
            .unwrap()
            .success()
    );
    std::fs::write(
        deck.join("slides/99-bad.html"),
        "<div data-plan-id=\"missing-id\"></div>",
    )
    .unwrap();
    let check = sideshow_command()
        .args(["plan", "check"])
        .arg(&deck)
        .output()
        .unwrap();
    assert!(!check.status.success());
    assert!(String::from_utf8_lossy(&check.stdout).contains("missing-id"));

    let out = temp.path().join("export.md");
    std::fs::write(&out, "keep").unwrap();
    let export = sideshow_command()
        .args(["plan", "export", "--output"])
        .arg(&out)
        .arg(&deck)
        .output()
        .unwrap();
    assert!(!export.status.success());
    assert_eq!(std::fs::read_to_string(out).unwrap(), "keep");
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

#[cfg(unix)]
#[test]
fn plan_component_fixture_builds_with_css_and_no_component_js() {
    let tmp = tempfile::tempdir().unwrap();
    let deck = tmp.path().join("plan-components");
    copy_dir(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/plan-components"),
        &deck,
    );
    let tw = fake_tailwind(tmp.path());
    let config = tmp.path().join("config.toml");
    std::fs::write(
        &config,
        format!("[tools]\ntailwindcss = '{}'\n", tw.display()),
    )
    .unwrap();

    let out = Command::new(env!("CARGO_BIN_EXE_sideshow"))
        .env("SIDESHOW_CONFIG", &config)
        .args(["build", deck.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let html = std::fs::read_to_string(deck.join("dist/plan-components.html")).unwrap();
    assert!(html.contains(".plan-shell"));
    assert!(html.contains(".plan-status"));
    assert!(html.contains("data-state=\"watch\""));
    assert!(html.contains("Watch: dependency in flight"));
    assert!(html.contains("Critical dependency"));
    assert!(!html.contains("plan-components.js"));
    assert!(!html.contains("customElements.define"));
}

fn copy_dir(from: impl AsRef<std::path::Path>, to: impl AsRef<std::path::Path>) {
    std::fs::create_dir_all(&to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let dest = to.as_ref().join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(entry.path(), dest);
        } else {
            std::fs::copy(entry.path(), dest).unwrap();
        }
    }
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
    let exported_handoff: serde_json::Value =
        serde_json::from_slice(&std::fs::read(exported).unwrap()).unwrap();
    assert!(exported_handoff.get("security_notice").is_some());
    let exported_artifact: sideshow::review::ReviewArtifact =
        serde_json::from_value(exported_handoff["UNTRUSTED_REVIEW_ARTIFACT"].clone()).unwrap();
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
    let exported_handoff: serde_json::Value = serde_json::from_slice(&export.stdout).unwrap();
    let exported: sideshow::review::ReviewArtifact =
        serde_json::from_value(exported_handoff["UNTRUSTED_REVIEW_ARTIFACT"].clone()).unwrap();
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

#[cfg(unix)]
#[test]
fn standalone_build_without_review_ignores_unavailable_xdg_configuration() {
    let tmp = tempfile::tempdir().unwrap();
    let deck = prebuilt_t_deck(tmp.path());
    let tailwind = fake_tailwind(tmp.path());
    for invalid_xdg in [None, Some("relative/state")] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_sideshow"));
        command
            .env_remove("HOME")
            .env_remove("XDG_STATE_HOME")
            .env("SIDESHOW_TAILWINDCSS", &tailwind)
            .args(["build", deck.to_str().unwrap()]);
        if let Some(value) = invalid_xdg {
            command.env("XDG_STATE_HOME", value);
        }
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
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
