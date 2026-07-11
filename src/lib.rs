use anyhow::{Context, bail};
use askama::Template;
use base64::Engine;
use comrak::{Options, Plugins, markdown_to_html_with_plugins};
use lol_html::{RewriteStrSettings, element, html_content::ContentType};
use regex::{Captures, Regex};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::LazyLock,
};

mod fonts;
mod highlight;
pub mod review;
#[doc(hidden)]
pub mod secure_fs;

pub use fonts::{FontFaceConfig, FontStyle};

fn render_template<T: Template>(
    template: &T,
    output: &str,
    template_path: &str,
) -> anyhow::Result<String> {
    template
        .render()
        .with_context(|| format!("failed to render {output} from {template_path}"))
}

#[derive(Template)]
#[template(path = "deck.html")]
struct DeckTemplate<'a> {
    title: &'a str,
    css: TrustedCompilerOutput<'a>,
    runtime_marker: &'a str,
    slides: &'a [SlideSectionView<'a>],
    runtime_js: TrustedCompilerOutput<'a>,
}

struct SlideSectionView<'a> {
    class: &'a str,
    stem: &'a str,
    source: &'a str,
    html: TrustedSlideHtml<'a>,
}

#[derive(Clone, Copy)]
struct TrustedCompilerOutput<'a>(&'a str);

#[derive(Clone, Copy)]
struct TrustedSlideHtml<'a>(&'a str);

impl std::fmt::Display for TrustedCompilerOutput<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}

impl std::fmt::Display for TrustedSlideHtml<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}

#[cfg(test)]
mod compiler_template_tests {
    use super::*;

    #[test]
    fn deck_template_escapes_metadata_and_preserves_audited_compiler_output() {
        let slide = SlideSectionView {
            class: "slide",
            stem: "01\" onmouseover=\"alert(1)",
            source: "slides/01\" onfocus=\"alert(2).html",
            html: TrustedSlideHtml("<h1>Rendered once</h1>"),
        };
        let rendered = render_template(
            &DeckTemplate {
                title: "<img src=x onerror=alert(3)>",
                css: TrustedCompilerOutput(".slide > h1 { color: red; }"),
                runtime_marker: RUNTIME_MARKER,
                slides: &[slide],
                runtime_js: TrustedCompilerOutput("window.sideshowTemplateTest = true;"),
            },
            "test deck",
            "templates/deck.html",
        )
        .unwrap();

        assert!(!rendered.contains("<img src=x"));
        assert!(!rendered.contains(r#"id="s-01" onmouseover="#));
        assert!(!rendered.contains(r#"data-src="slides/01" onfocus="#));
        assert_eq!(rendered.matches("<h1>Rendered once</h1>").count(), 1);
        assert!(rendered.contains(".slide > h1 { color: red; }"));
        assert!(rendered.contains("window.sideshowTemplateTest = true;"));
        assert!(!rendered.contains("&lt;h1&gt;Rendered once"));
    }
}

pub mod plan {
    use super::*;

    #[derive(Template)]
    #[template(path = "plan/slides/title.html")]
    struct PlanTitleSlideTemplate<'a> {
        title: &'a str,
    }

    #[derive(Template)]
    #[template(path = "plan/slides/proposal.html")]
    struct PlanProposalSlideTemplate;

    #[derive(Template)]
    #[template(path = "plan/slides/guardrails.html")]
    struct PlanGuardrailsSlideTemplate;

    #[derive(Debug, Clone, Copy, Serialize, Deserialize)]
    #[serde(rename_all = "snake_case")]
    pub enum PlanStatus {
        Draft,
        InReview,
        Approved,
        Active,
        Blocked,
        Completed,
        Superseded,
    }

    #[derive(Debug, Clone, Copy, Serialize, Deserialize)]
    #[serde(rename_all = "snake_case")]
    pub enum DecisionStatus {
        Proposed,
        Accepted,
        Rejected,
        Deferred,
        Superseded,
    }

    #[derive(Debug, Clone, Copy, Serialize, Deserialize)]
    #[serde(rename_all = "snake_case")]
    pub enum WorkStatus {
        Todo,
        InProgress,
        Blocked,
        InReview,
        Done,
        Dropped,
    }

    #[derive(Debug, Clone, Copy, Serialize, Deserialize)]
    #[serde(rename_all = "snake_case")]
    pub enum RiskLevel {
        Low,
        Medium,
        High,
    }

    macro_rules! display_enum {
        ($type:ty, {$($variant:path => $value:literal),+ $(,)?}) => {
            impl std::fmt::Display for $type {
                fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                    f.write_str(match self {$($variant => $value),+})
                }
            }
        };
    }
    display_enum!(PlanStatus, {
        PlanStatus::Draft => "draft",
        PlanStatus::InReview => "in_review",
        PlanStatus::Approved => "approved",
        PlanStatus::Active => "active",
        PlanStatus::Blocked => "blocked",
        PlanStatus::Completed => "completed",
        PlanStatus::Superseded => "superseded",
    });
    display_enum!(DecisionStatus, {
        DecisionStatus::Proposed => "proposed",
        DecisionStatus::Accepted => "accepted",
        DecisionStatus::Rejected => "rejected",
        DecisionStatus::Deferred => "deferred",
        DecisionStatus::Superseded => "superseded",
    });
    display_enum!(WorkStatus, {
        WorkStatus::Todo => "todo",
        WorkStatus::InProgress => "in_progress",
        WorkStatus::Blocked => "blocked",
        WorkStatus::InReview => "in_review",
        WorkStatus::Done => "done",
        WorkStatus::Dropped => "dropped",
    });
    display_enum!(RiskLevel, {
        RiskLevel::Low => "low",
        RiskLevel::Medium => "medium",
        RiskLevel::High => "high",
    });

    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(deny_unknown_fields)]
    pub struct Plan {
        pub schema_version: u32,
        pub title: String,
        pub status: PlanStatus,
        pub objective: String,
        #[serde(default)]
        pub outcomes: Vec<Outcome>,
        #[serde(default)]
        pub constraints: Vec<Constraint>,
        #[serde(default)]
        pub non_goals: Vec<String>,
        #[serde(default)]
        pub decisions: Vec<Decision>,
        #[serde(default)]
        pub workstreams: Vec<Workstream>,
        #[serde(default)]
        pub risks: Vec<Risk>,
    }
    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(deny_unknown_fields)]
    pub struct Outcome {
        pub id: String,
        pub description: String,
        #[serde(default)]
        pub proof: Vec<String>,
    }
    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(deny_unknown_fields)]
    pub struct Constraint {
        pub id: String,
        pub description: String,
    }
    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(deny_unknown_fields)]
    pub struct Decision {
        pub id: String,
        pub title: String,
        pub status: DecisionStatus,
        pub rationale: String,
    }
    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(deny_unknown_fields)]
    pub struct Workstream {
        pub id: String,
        pub title: String,
        pub status: WorkStatus,
        #[serde(default)]
        pub owner: Option<String>,
        #[serde(default)]
        pub tasks: Vec<Task>,
    }
    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(deny_unknown_fields)]
    pub struct Task {
        pub id: String,
        pub title: String,
        pub status: WorkStatus,
        #[serde(default)]
        pub owner: Option<String>,
        #[serde(default)]
        pub outcomes: Vec<String>,
        #[serde(default)]
        pub dependencies: Vec<String>,
        #[serde(default)]
        pub files: Vec<String>,
        #[serde(default)]
        pub acceptance_checks: Vec<String>,
        pub verification: Verification,
    }
    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(deny_unknown_fields)]
    pub struct Verification {
        pub intent: String,
        pub commands: Vec<String>,
    }
    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(deny_unknown_fields)]
    pub struct Risk {
        pub id: String,
        pub description: String,
        pub likelihood: RiskLevel,
        pub impact: RiskLevel,
        pub mitigation: String,
    }

    pub fn new_plan_deck(dir: &Path, theme: &str) -> anyhow::Result<()> {
        if dir.exists() {
            bail!(
                "refusing to overwrite existing plan directory {}",
                dir.display()
            );
        }
        new_deck(dir, theme)?;
        let title = dir
            .file_name()
            .and_then(|name| name.to_str())
            .map(plan_title)
            .unwrap_or_else(|| "Alignment Plan".into());
        let deck_toml_path = dir.join("deck.toml");
        let deck_toml = fs::read_to_string(&deck_toml_path)?;
        let title_line = format!("title = \"{} Demo\"", title_case(theme));
        fs::write(
            &deck_toml_path,
            deck_toml.replacen(&title_line, &format!("title = \"{title}\""), 1),
        )?;
        let _ = fs::remove_file(dir.join("slides/02-content.md"));
        let plan = Plan {
            schema_version: 2,
            title: title.clone(),
            status: PlanStatus::Draft,
            objective: "Align stakeholders on a focused, reviewable increment by framing scope, exploring proposal options, and refining a strict-check-clean planning digest before execution begins."
                .into(),
            outcomes: vec![
                Outcome { id: "outcome-alignment".into(), description: "Readers understand the goal, scope boundaries, proposal shape, and review criteria before work begins.".into(), proof: vec!["slides/01-title.html and slides/02-plan.html reference alignment anchors".into(), "plan.json captures constraints, decisions, risks, owners, and dependencies without live tracker state".into()] },
                Outcome { id: "outcome-proposal".into(), description: "The proposed increment is expressed as tracker-neutral work that can be translated after alignment.".into(), proof: vec!["proposal task acceptance checks explain scope and dependency intent".into(), "slides/02-plan.html anchors the proposal workstream and exploration task".into()] },
                Outcome { id: "outcome-reviewed-digest".into(), description: "A reviewed planning digest is strict-check-clean enough for humans to decide what happens next.".into(), proof: vec!["sideshow plan check . --strict succeeds".into(), "Markdown digest preserves verification intent without claiming execution status".into()] },
            ],
            constraints: vec![
                Constraint { id: "constraint-authored-projection".into(), description: "Slides remain authored projection; plan.json is the canonical machine-readable contract.".into() },
                Constraint { id: "constraint-strict-clean".into(), description: "Do not hand off until generated output and strict planning checks are clean.".into() },
            ],
            non_goals: vec!["Do not store private review annotations or ad-hoc agent notes in plan.json.".into(), "Do not model live assignment, status, blocker, completion, or tracker workflow state.".into(), "Do not assume a 1:1 mapping between plan tasks and issues; translators may split, combine, or omit packets.".into()],
            decisions: vec![
                Decision { id: "decision-json-canonical".into(), title: "Use plan.json as canonical data".into(), status: DecisionStatus::Accepted, rationale: "Slides persuade humans, but agents need stable structured input with exact commands.".into() },
                Decision { id: "decision-verify-intent".into(), title: "Separate verification intent from commands".into(), status: DecisionStatus::Accepted, rationale: "Human reviewers need the purpose of verification while agents need verbatim commands.".into() },
            ],
            workstreams: vec![Workstream {
                id: "ws-alignment".into(),
                title: "Alignment proposal".into(),
                status: WorkStatus::Todo,
                owner: Some("planning-agent".into()),
                tasks: vec![
                  Task {
                    id: "task-align".into(),
                    title: "Frame scope, risks, and review criteria".into(),
                    status: WorkStatus::Todo,
                    owner: Some("planning-agent".into()),
                    outcomes: vec!["outcome-alignment".into()],
                    dependencies: vec![],
                    files: vec!["plan.json".into(), "slides/".into()],
                    acceptance_checks: vec!["Outcomes, constraints, decisions, and risks explain where planning stops and later execution decisions begin.".into(), "Slides contain stable data-plan-id anchors for the framing artifacts.".into()],
                    verification: Verification { intent: "Confirm the planning contract is internally consistent before proposal exploration starts.".into(), commands: vec!["sideshow plan check . --strict".into()] },
                  },
                  Task { id: "task-explore".into(), title: "Explore proposal options".into(), status: WorkStatus::Todo, owner: Some("planning-agent".into()), outcomes: vec!["outcome-proposal".into()], dependencies: vec!["task-align".into()], files: vec!["plan.json".into(), "slides/".into()], acceptance_checks: vec!["Proposal options explain tradeoffs, dependencies, and tracker-neutral translation choices.".into(), "No option claims live assignee, blocker, status, or completion state.".into()], verification: Verification { intent: "Check that proposal exploration remains canonical, structured, and tracker-neutral.".into(), commands: vec!["sideshow plan check . --strict".into()] } },
                  Task { id: "task-refine-digest".into(), title: "Refine reviewed planning digest".into(), status: WorkStatus::Todo, owner: Some("planning-agent".into()), outcomes: vec!["outcome-reviewed-digest".into()], dependencies: vec!["task-explore".into()], files: vec!["plan.json".into(), "slides/".into()], acceptance_checks: vec!["Digest separates alignment, proposal rationale, constraints, risks, and verification intent.".into(), "Digest is ready for human review without implying execution has started.".into()], verification: Verification { intent: "Prove the deck and plan are strict-check-clean for a reviewed planning digest.".into(), commands: vec!["sideshow plan check . --strict".into(), "sideshow build .".into(), "sideshow plan export . --format markdown".into()] } },
                ],
            }],
            risks: vec![Risk { id: "risk-drift".into(), description: "Slides drift from canonical plan data or generated output is not refreshed.".into(),
                likelihood: RiskLevel::Medium,
                impact: RiskLevel::High,
                mitigation: "Run strict plan checks, keep data-plan-id references current, and report generated-output status in handoff.".into(),
            }],
        };
        fs::write(dir.join("plan.json"), canonical_json(&plan)?)?;
        fs::write(
            dir.join("slides/01-title.html"),
            render_template(
                &PlanTitleSlideTemplate { title: &title },
                "slides/01-title.html",
                "templates/plan/slides/title.html",
            )?,
        )?;
        fs::write(
            dir.join("slides/02-plan.html"),
            render_template(
                &PlanProposalSlideTemplate,
                "slides/02-plan.html",
                "templates/plan/slides/proposal.html",
            )?,
        )?;
        fs::write(
            dir.join("slides/03-verify.html"),
            render_template(
                &PlanGuardrailsSlideTemplate,
                "slides/03-verify.html",
                "templates/plan/slides/guardrails.html",
            )?,
        )?;
        Ok(())
    }

    fn plan_title(name: &str) -> String {
        name.split(|c: char| !c.is_alphanumeric())
            .filter(|part| !part.is_empty())
            .map(title_case)
            .collect::<Vec<_>>()
            .join(" ")
    }

    pub fn load(dir: &Path) -> anyhow::Result<Plan> {
        serde_json::from_slice(
            &fs::read(dir.join("plan.json"))
                .with_context(|| format!("failed to read {}", dir.join("plan.json").display()))?,
        )
        .context("failed to parse plan.json")
    }
    pub fn canonical_json(plan: &Plan) -> anyhow::Result<String> {
        Ok(format!("{}\n", serde_json::to_string_pretty(plan)?))
    }

    pub fn markdown(plan: &Plan) -> String {
        let mut out = format!(
            "# {}\n\n- Schema: {}\n- Status: {}\n\n## Objective\n{}\n",
            md_inline(&plan.title),
            plan.schema_version,
            plan.status,
            md_paragraph(&plan.objective)
        );
        out.push_str("\n## Outcomes\n");
        for outcome in &plan.outcomes {
            out.push_str(&format!(
                "\n### {}\n{}\n\nProof:\n{}\n",
                md_inline(&outcome.id),
                md_paragraph(&outcome.description),
                bullets(&outcome.proof)
            ));
        }
        out.push_str("\n## Constraints\n");
        for constraint in &plan.constraints {
            out.push_str(&format!(
                "- **{}:** {}\n",
                md_inline(&constraint.id),
                md_inline(&constraint.description)
            ));
        }
        out.push_str("\n## Non-goals\n");
        out.push_str(&format!("{}\n", bullets(&plan.non_goals)));
        out.push_str("\n## Proposed work\n");
        for ws in &plan.workstreams {
            out.push_str(&format!(
                "\n### {} ({})\n\n- Status: {}\n- Owner: {}\n",
                md_inline(&ws.title),
                md_inline(&ws.id),
                ws.status,
                md_inline(ws.owner.as_deref().unwrap_or("unassigned"))
            ));
            for t in &ws.tasks {
                out.push_str(&format!("\n#### {} ({})\n- Status: {}\n- Owner: {}\n- Outcomes: {}\n- Dependencies: {}\n- Files: {}\n- Acceptance:\n{}\n- Verification intent: {}\n- Agent commands:\n\n{}\n", md_inline(&t.title), md_inline(&t.id), t.status, md_inline(t.owner.as_deref().unwrap_or("unassigned")), list(&t.outcomes), list(&t.dependencies), list(&t.files), bullets(&t.acceptance_checks), md_inline(&t.verification.intent), fenced_commands(&t.verification.commands)));
            }
        }
        out.push_str("\n## Decisions\n");
        for d in &plan.decisions {
            out.push_str(&format!(
                "- {} ({}): {} — {}\n",
                md_inline(&d.title),
                md_inline(&d.id),
                d.status,
                md_inline(&d.rationale)
            ));
        }
        out.push_str("\n## Risks\n");
        for r in &plan.risks {
            out.push_str(&format!(
                "- **{}** (likelihood: {}, impact: {}): {} Mitigation: {}\n",
                md_inline(&r.id),
                r.likelihood,
                r.impact,
                md_inline(&r.description),
                md_inline(&r.mitigation)
            ));
        }
        out.push_str("\n## Manual issue-drafting packets\n\n");
        out.push_str("Tracker-neutral disclaimer: these packets are source material for manual issue drafting and may be split or combined. The translator chooses issue boundaries, labels, teams, priority, milestones, and tracker conventions. This export contains no live tracker state.\n");
        let outcome_by_id: BTreeMap<_, _> = plan
            .outcomes
            .iter()
            .map(|outcome| (outcome.id.as_str(), outcome))
            .collect();
        let task_by_id: BTreeMap<_, _> = plan
            .workstreams
            .iter()
            .flat_map(|workstream| workstream.tasks.iter())
            .map(|task| (task.id.as_str(), task))
            .collect();
        let mut enables_by_id: BTreeMap<&str, Vec<&Task>> = BTreeMap::new();
        for ws in &plan.workstreams {
            for task in &ws.tasks {
                for dependency_id in &task.dependencies {
                    enables_by_id
                        .entry(dependency_id.as_str())
                        .or_default()
                        .push(task);
                }
            }
        }
        out.push_str("\n### Proposed decomposition and dependency overview\n\n");
        out.push_str("- Derived dependency overview for manual drafting only; reverse `Enables` edges are not live tracker blockers, scheduling instructions, or JSON fields.\n");
        for ws in &plan.workstreams {
            out.push_str(&format!(
                "- Workstream {} — {}\n",
                md_inline(&ws.id),
                md_inline(&ws.title)
            ));
            for task in &ws.tasks {
                out.push_str(&format!(
                    "  - Task {} — {}\n    - Canonical source: plan.json → workstream {} → task {}\n    - Depends on:\n",
                    md_inline(&task.id), md_inline(&task.title), md_inline(&ws.id), md_inline(&task.id)
                ));
                if task.dependencies.is_empty() {
                    out.push_str("      - none — root/parallel-start candidate\n");
                } else {
                    for dependency_id in &task.dependencies {
                        if let Some(dependency) = task_by_id.get(dependency_id.as_str()) {
                            out.push_str(&format!(
                                "      - {} — {}\n",
                                md_inline(&dependency.id),
                                md_inline(&dependency.title)
                            ));
                        } else {
                            out.push_str(&format!(
                                "      - {} — unresolved reference\n",
                                md_inline(dependency_id)
                            ));
                        }
                    }
                }
                out.push_str("    - Enables:\n");
                if let Some(enabled_tasks) = enables_by_id.get(task.id.as_str()) {
                    for enabled in enabled_tasks {
                        out.push_str(&format!(
                            "      - {} — {}\n",
                            md_inline(&enabled.id),
                            md_inline(&enabled.title)
                        ));
                    }
                } else {
                    out.push_str("      - none\n");
                }
            }
        }
        for ws in &plan.workstreams {
            for task in &ws.tasks {
                out.push_str(&format!(
                    "\n### Issue source packet: {} ({})\n\n- Objective: {}\n- Canonical source: plan.json → workstream {} → task {}\n- Drafting context: Use the plan-level constraints, non-goals, decisions, and risks above as the canonical source context; do not infer tracker IDs, labels, teams, priority, milestones, or live tracker state.\n- Workstream: {} — {}\n- Task: {} — {}\n- Proposed planning status: {}\n- Proposed owner: {}\n",
                    md_inline(&task.title),
                    md_inline(&task.id),
                    md_inline(&plan.objective),
                    md_inline(&ws.id),
                    md_inline(&task.id),
                    md_inline(&ws.id),
                    md_inline(&ws.title),
                    md_inline(&task.id),
                    md_inline(&task.title),
                    task.status,
                    md_inline(task.owner
                        .as_deref()
                        .or(ws.owner.as_deref())
                        .unwrap_or("unassigned"))
                ));
                out.push_str("- Resolved outcomes:\n");
                if task.outcomes.is_empty() {
                    out.push_str("  - none\n");
                } else {
                    for outcome_id in &task.outcomes {
                        if let Some(outcome) = outcome_by_id.get(outcome_id.as_str()) {
                            out.push_str(&format!(
                                "  - {}: {}\n    - Proof:\n{}\n",
                                md_inline(&outcome.id),
                                md_inline(&outcome.description),
                                bullets_indent(&outcome.proof, 6)
                            ));
                        } else {
                            out.push_str(&format!(
                                "  - {}: unresolved reference\n",
                                md_inline(outcome_id)
                            ));
                        }
                    }
                }
                out.push_str("- Resolved dependencies:\n");
                if task.dependencies.is_empty() {
                    out.push_str("  - none — no dependencies means this task can start in parallel with any other no-dependency task when capacity is available.\n");
                } else {
                    for dependency_id in &task.dependencies {
                        if let Some(dependency) = task_by_id.get(dependency_id.as_str()) {
                            out.push_str(&format!(
                                "  - {} — {}\n",
                                md_inline(&dependency.id),
                                md_inline(&dependency.title)
                            ));
                        } else {
                            out.push_str(&format!(
                                "  - {} — unresolved reference\n",
                                md_inline(dependency_id)
                            ));
                        }
                    }
                }
                out.push_str(&format!(
                    "- Files:\n{}\n- Acceptance checks:\n{}\n- Verification intent: {}\n\nExact commands (verbatim, authored order):\n{}\n",
                    bullets(&task.files),
                    bullets(&task.acceptance_checks),
                    md_inline(&task.verification.intent),
                    fenced_commands(&task.verification.commands)
                ));
            }
        }
        out
    }
    fn list(v: &[String]) -> String {
        if v.is_empty() {
            "none".into()
        } else {
            v.iter()
                .map(|s| md_inline(s))
                .collect::<Vec<_>>()
                .join(", ")
        }
    }
    fn bullets(v: &[String]) -> String {
        if v.is_empty() {
            "  - none".into()
        } else {
            v.iter()
                .map(|s| format_list_item(s, 2))
                .collect::<Vec<_>>()
                .join("\n")
        }
    }
    fn bullets_indent(v: &[String], spaces: usize) -> String {
        let prefix = " ".repeat(spaces);
        if v.is_empty() {
            format!("{prefix}- none")
        } else {
            v.iter()
                .map(|s| format_list_item(s, spaces))
                .collect::<Vec<_>>()
                .join("\n")
        }
    }
    fn format_list_item(s: &str, spaces: usize) -> String {
        let prefix = " ".repeat(spaces);
        let continuation = " ".repeat(spaces + 4);
        let escaped = md_paragraph(s);
        let mut lines = escaped.lines();
        let first = lines.next().unwrap_or("");
        let mut out = format!("{prefix}- {first}");
        for line in lines {
            out.push('\n');
            out.push_str(&continuation);
            out.push_str(line);
        }
        out
    }
    fn md_inline(s: &str) -> String {
        md_escape(&s.replace(['\r', '\n'], " "))
    }
    fn md_paragraph(s: &str) -> String {
        s.replace("\r\n", "\n")
            .replace('\r', "\n")
            .split('\n')
            .map(md_escape_paragraph_line)
            .collect::<Vec<_>>()
            .join("  \n")
    }
    fn md_escape_paragraph_line(s: &str) -> String {
        escape_line_start_marker(&md_escape(s))
    }
    fn escape_line_start_marker(s: &str) -> String {
        if s.starts_with("# ") || s.starts_with("- ") || s.starts_with("+ ") {
            format!("\\{s}")
        } else if let Some(dot) = ordered_list_marker_dot(s) {
            format!("{}\\{}", &s[..dot], &s[dot..])
        } else {
            s.to_string()
        }
    }
    fn ordered_list_marker_dot(s: &str) -> Option<usize> {
        let bytes = s.as_bytes();
        let mut index = 0;
        while index < bytes.len() && bytes[index].is_ascii_digit() {
            index += 1;
        }
        if index > 0 && bytes.get(index) == Some(&b'.') && bytes.get(index + 1) == Some(&b' ') {
            Some(index)
        } else {
            None
        }
    }
    fn md_escape(s: &str) -> String {
        let mut out = String::new();
        for ch in s.chars() {
            match ch {
                '&' => out.push_str("&amp;"),
                '<' => out.push_str("&lt;"),
                '>' => out.push_str("&gt;"),
                '\\' | '`' | '*' | '_' | '{' | '}' | '[' | ']' | '(' | ')' | '!' | '|' => {
                    out.push('\\');
                    out.push(ch);
                }
                _ => out.push(ch),
            }
        }
        out
    }
    fn fenced_commands(commands: &[String]) -> String {
        if commands.is_empty() {
            "none".into()
        } else {
            commands
                .iter()
                .enumerate()
                .map(|(index, command)| {
                    let fence = command_fence(command);
                    format!("Command {}:\n\n{fence}sh\n{command}\n{fence}", index + 1)
                })
                .collect::<Vec<_>>()
                .join("\n\n")
        }
    }
    fn command_fence(command: &str) -> String {
        let mut max_run = 0;
        let mut current_run = 0;
        for ch in command.chars() {
            if ch == '`' {
                current_run += 1;
                max_run = max_run.max(current_run);
            } else {
                current_run = 0;
            }
        }
        "`".repeat((max_run + 1).max(3))
    }

    pub fn check(dir: &Path) -> Vec<CheckFinding> {
        let mut findings = check_deck(dir);
        let plan = match load(dir) {
            Ok(p) => p,
            Err(e) => {
                findings.push(err("plan.json", "plan-schema", format!("{e:#}")));
                return findings;
            }
        };
        if plan.schema_version != 2 {
            findings.push(err("plan.json", "plan-schema", "schema_version must be 2"));
        }
        if plan.title.trim().is_empty() || plan.objective.trim().is_empty() {
            findings.push(err(
                "plan.json",
                "plan-schema",
                "title and objective must not be empty",
            ));
        }
        if plan.outcomes.is_empty() || plan.workstreams.is_empty() {
            findings.push(err(
                "plan.json",
                "plan-actionability",
                "a plan needs at least one outcome and one workstream",
            ));
        }
        let mut ids = BTreeSet::new();
        let mut outcome_ids = BTreeSet::new();
        let mut task_ids = BTreeSet::new();
        macro_rules! id {
            ($id:expr) => {
                if $id.trim().is_empty() {
                    findings.push(err("plan.json", "plan-id", "ids must not be empty"));
                } else if !valid_plan_id(&$id) {
                    findings.push(err(
                        "plan.json",
                        "plan-id",
                        format!("id {} must use lowercase kebab-case", $id),
                    ));
                } else if !ids.insert($id.clone()) {
                    findings.push(err("plan.json", "plan-id", format!("duplicate id {}", $id)));
                }
            };
        }
        for o in &plan.outcomes {
            id!(o.id);
            outcome_ids.insert(o.id.clone());
            if o.description.trim().is_empty() {
                findings.push(err(
                    "plan.json",
                    "plan-outcome",
                    format!("outcome {} needs a description", o.id),
                ));
            }
            if o.proof.is_empty() {
                findings.push(warn(
                    "plan.json",
                    "plan-coverage",
                    format!("outcome {} has no proof", o.id),
                ));
            }
        }
        for c in &plan.constraints {
            id!(c.id);
            if c.description.trim().is_empty() {
                findings.push(err(
                    "plan.json",
                    "plan-constraint",
                    format!("constraint {} needs a description", c.id),
                ));
            }
        }
        for d in &plan.decisions {
            id!(d.id);
            if d.title.trim().is_empty() || d.rationale.trim().is_empty() {
                findings.push(err(
                    "plan.json",
                    "plan-decision",
                    format!("decision {} needs status and rationale", d.id),
                ));
            }
        }
        for r in &plan.risks {
            id!(r.id);
            if r.mitigation.trim().is_empty() {
                findings.push(err(
                    "plan.json",
                    "plan-risk",
                    format!("risk {} needs mitigation", r.id),
                ));
            }
        }
        for ws in &plan.workstreams {
            id!(ws.id);
            if ws.title.trim().is_empty() || ws.tasks.is_empty() {
                findings.push(err(
                    "plan.json",
                    "plan-workstream",
                    format!("workstream {} needs a title and at least one task", ws.id),
                ));
            }
            if ws.owner.as_deref().is_none_or(str::is_empty) {
                findings.push(warn(
                    "plan.json",
                    "plan-ownership",
                    format!("workstream {} has no owner", ws.id),
                ));
            }
            for t in &ws.tasks {
                id!(t.id);
                task_ids.insert(t.id.clone());
                if t.title.trim().is_empty()
                    || t.files.is_empty()
                    || t.acceptance_checks.is_empty()
                    || t.verification.intent.trim().is_empty()
                    || t.verification.commands.is_empty()
                    || t.verification
                        .commands
                        .iter()
                        .any(|command| command.trim().is_empty())
                {
                    findings.push(err("plan.json", "plan-task", format!("task {} needs status, files, acceptance_checks, verification.intent, and verification.commands", t.id)));
                }
                if t.owner.as_deref().is_none_or(str::is_empty) {
                    findings.push(warn(
                        "plan.json",
                        "plan-ownership",
                        format!("task {} has no owner", t.id),
                    ));
                }
                for outcome in &t.outcomes {
                    if !outcome_ids.contains(outcome) {
                        findings.push(err(
                            "plan.json",
                            "plan-reference",
                            format!("task {} targets unknown outcome {}", t.id, outcome),
                        ));
                    }
                }
            }
        }
        for ws in &plan.workstreams {
            for t in &ws.tasks {
                for dep in &t.dependencies {
                    if !task_ids.contains(dep) {
                        findings.push(err(
                            "plan.json",
                            "plan-reference",
                            format!("task {} depends on unknown task {}", t.id, dep),
                        ));
                    }
                }
            }
        }
        for c in cycles(&plan) {
            findings.push(err(
                "plan.json",
                "plan-cycle",
                format!("task dependency cycle: {}", c.join(" -> ")),
            ));
        }
        check_slide_refs(dir, &ids, &mut findings);
        for id in ids {
            if !slide_refs(dir).contains(&id) {
                findings.push(warn(
                    "slides",
                    "plan-coverage",
                    format!("plan id {id} is not referenced by any data-plan-id"),
                ));
            }
        }
        findings
    }
    fn valid_plan_id(id: &str) -> bool {
        static PLAN_ID_RE: LazyLock<Regex> =
            LazyLock::new(|| Regex::new(r"^[a-z0-9]+(?:-[a-z0-9]+)*$").unwrap());
        PLAN_ID_RE.is_match(id)
    }
    fn err(path: &str, kind: &str, message: impl Into<String>) -> CheckFinding {
        CheckFinding {
            path: path.into(),
            severity: FindingSeverity::Error,
            kind: kind.into(),
            message: message.into(),
        }
    }
    fn warn(path: &str, kind: &str, message: impl Into<String>) -> CheckFinding {
        CheckFinding {
            path: path.into(),
            severity: FindingSeverity::Warning,
            kind: kind.into(),
            message: message.into(),
        }
    }
    fn slide_refs(dir: &Path) -> BTreeSet<String> {
        let Ok(slides) = fs::read_dir(dir.join("slides")) else {
            return BTreeSet::new();
        };
        let re = Regex::new(r#"data-plan-id\s*=\s*[\"']([^\"']+)[\"']"#).unwrap();
        slides
            .filter_map(Result::ok)
            .filter_map(|e| fs::read_to_string(e.path()).ok())
            .flat_map(|s| {
                re.captures_iter(&s)
                    .map(|c| c[1].to_string())
                    .collect::<Vec<_>>()
            })
            .collect()
    }
    fn check_slide_refs(dir: &Path, ids: &BTreeSet<String>, findings: &mut Vec<CheckFinding>) {
        for id in slide_refs(dir) {
            if !ids.contains(&id) {
                findings.push(err(
                    "slides",
                    "plan-reference",
                    format!("data-plan-id references unknown id {id}"),
                ));
            }
        }
    }
    fn cycles(plan: &Plan) -> Vec<Vec<String>> {
        let deps: BTreeMap<_, _> = plan
            .workstreams
            .iter()
            .flat_map(|w| &w.tasks)
            .map(|t| {
                (
                    t.id.as_str(),
                    t.dependencies
                        .iter()
                        .map(String::as_str)
                        .collect::<Vec<_>>(),
                )
            })
            .collect();
        let mut out = vec![];
        for start in deps.keys() {
            visit(start, start, &deps, &mut vec![], &mut out);
        }
        out.sort();
        out.dedup();
        out
    }
    fn visit<'a>(
        start: &'a str,
        node: &'a str,
        deps: &BTreeMap<&'a str, Vec<&'a str>>,
        stack: &mut Vec<String>,
        out: &mut Vec<Vec<String>>,
    ) {
        stack.push(node.into());
        if let Some(nexts) = deps.get(node) {
            for &n in nexts {
                if n == start {
                    let mut c = stack.clone();
                    c.push(start.into());
                    out.push(c);
                } else if !stack.iter().any(|s| s == n) {
                    visit(start, n, deps, stack, out);
                }
            }
        }
        stack.pop();
    }

    #[cfg(test)]
    mod template_tests {
        use super::*;

        #[test]
        fn title_template_escapes_dynamic_markup() {
            let hostile = r#"<img src=x onerror="alert(1)"><script>alert(2)</script>"#;
            let rendered = render_template(
                &PlanTitleSlideTemplate { title: hostile },
                "slides/01-title.html",
                "templates/plan/slides/title.html",
            )
            .unwrap();

            assert!(rendered.contains("img src=x onerror="));
            assert!(rendered.contains("script"));
            assert!(rendered.contains("alert(2)"));
            assert!(!rendered.contains("<img"));
            assert!(!rendered.contains("<script>"));
            assert!(!rendered.contains("onerror=\"alert(1)\""));
        }
    }
}

pub const RUNTIME_MARKER: &str = "sideshow-runtime-v1";
const STAGE_CSS: &str = include_str!("runtime/stage.css");
const PLAN_CSS: &str = include_str!("components/plan.css");
const RUNTIME_JS: &str = include_str!("runtime/runtime.js");
const SIGNAL_CSS: &str = include_str!("themes/signal.css");
const LEDGER_CSS: &str = include_str!("themes/ledger.css");
const TERMINAL_CSS: &str = include_str!("themes/terminal.css");
const POSTER_CSS: &str = include_str!("themes/poster.css");
static CSS_URL_ASSET_RE: LazyLock<Regex> = LazyLock::new(|| {
    // Accepted grammar is intentionally narrow: case-insensitive CSS url(...)
    // with an assets/... path that is either unquoted with no CSS whitespace, or
    // quoted with any non-quote characters. Query/fragment suffixes are stripped
    // by asset_ref_without_suffix for filesystem resolution.
    Regex::new(r#"(?i)url\(\s*(?:\"(?P<dq>assets/[^\"]+)\"|'(?P<sq>assets/[^']+)'|(?P<bare>assets/[^)'\"\s]+))\s*\)"#).unwrap()
});
static CSS_COMMENT_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?s)/\*.*?\*/"#).unwrap());
static LOCAL_URL_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"url\(\s*['\"]?\s*#([A-Za-z_][A-Za-z0-9_.:-]*)\s*['\"]?\s*\)"#).unwrap()
});

pub fn find_tool(
    binary: &str,
    env_var: &str,
    purpose: &str,
    install_hint: &str,
) -> anyhow::Result<PathBuf> {
    let (config, config_path) = user_config()?;
    find_tool_with(
        &config,
        &config_path,
        binary,
        env_var,
        purpose,
        install_hint,
    )
}

fn find_tool_with(
    config: &UserConfig,
    config_path: &Path,
    binary: &str,
    env_var: &str,
    purpose: &str,
    install_hint: &str,
) -> anyhow::Result<PathBuf> {
    if let Some(value) = std::env::var_os(env_var) {
        let path = PathBuf::from(value);
        if !path.is_file() {
            bail!(
                "{env_var} points to {}, but it is not a file",
                path.display()
            );
        }
        return Ok(path);
    }

    if let Some(path) = config.tools.path_for(binary) {
        if !path.is_file() {
            bail!(
                "{} [tools] {binary} points to {}, but it is not a file",
                config_path.display(),
                path.display()
            );
        }
        return Ok(path.clone());
    }

    which::which(binary).with_context(|| {
        format!(
            "{binary} not found: {purpose}; install it ({install_hint}), set {env_var} to its path, or add {binary} to [tools] in {}",
            config_path.display()
        )
    })
}

#[derive(Debug, Default, Deserialize, PartialEq)]
pub struct UserConfig {
    #[serde(default)]
    pub tools: ToolsConfig,
    #[serde(default)]
    pub srht: SrhtConfig,
}

#[derive(Debug, Default, Deserialize, PartialEq)]
pub struct SrhtConfig {
    #[serde(default, rename = "token-cmd")]
    pub token_cmd: Option<Vec<String>>,
}

#[derive(Debug, Default, Deserialize, PartialEq)]
pub struct ToolsConfig {
    #[serde(default)]
    pub tailwindcss: Option<PathBuf>,
    #[serde(default)]
    pub ffmpeg: Option<PathBuf>,
    #[serde(default)]
    pub vhs: Option<PathBuf>,
    #[serde(default)]
    pub aws: Option<PathBuf>,
}

impl ToolsConfig {
    fn path_for(&self, binary: &str) -> Option<&PathBuf> {
        match binary {
            "tailwindcss" => self.tailwindcss.as_ref(),
            "ffmpeg" => self.ffmpeg.as_ref(),
            "vhs" => self.vhs.as_ref(),
            "aws" => self.aws.as_ref(),
            _ => None,
        }
    }
}

fn user_config() -> anyhow::Result<(UserConfig, PathBuf)> {
    let path = config_path();
    if !path.is_file() {
        return Ok((UserConfig::default(), path));
    }
    let raw = fs::read_to_string(&path)
        .with_context(|| format!("failed to read config file {}", path.display()))?;
    let config = parse_user_config(&raw, &path)?;
    Ok((config, path))
}

pub fn srht_config() -> anyhow::Result<(SrhtConfig, PathBuf)> {
    let (config, path) = user_config()?;
    Ok((config.srht, path))
}

fn parse_user_config(raw: &str, path: &Path) -> anyhow::Result<UserConfig> {
    toml::from_str(raw).with_context(|| format!("failed to parse config file {}", path.display()))
}

#[cfg(test)]
mod config_tests {
    use super::*;

    #[test]
    fn parses_srht_token_cmd() {
        let config = parse_user_config(
            "[srht]\ntoken-cmd = [\"pass\", \"show\", \"srht\"]\n",
            Path::new("config.toml"),
        )
        .unwrap();
        assert_eq!(
            config.srht.token_cmd,
            Some(vec!["pass".into(), "show".into(), "srht".into()])
        );
    }
}

fn config_path() -> PathBuf {
    if let Some(path) = std::env::var_os("SIDESHOW_CONFIG") {
        return PathBuf::from(path);
    }
    if let Some(home) = std::env::var_os("XDG_CONFIG_HOME") {
        return PathBuf::from(home).join("sideshow/config.toml");
    }
    default_config_path()
}

fn default_config_path() -> PathBuf {
    std::env::var_os("HOME")
        .map(|home| PathBuf::from(home).join(".config/sideshow/config.toml"))
        .unwrap_or_else(|| PathBuf::from("~/.config/sideshow/config.toml"))
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub struct ThemeMetadata {
    pub name: &'static str,
    pub description: &'static str,
    pub mood: &'static str,
    pub formality: &'static str,
    pub density_fit: &'static str,
    pub best_for: &'static str,
    pub avoid_for: &'static str,
}

pub const THEMES: &[ThemeMetadata] = &[
    ThemeMetadata {
        name: "signal",
        description: "Dark, crisp executive technology theme with a cool cyan accent.",
        mood: "focused",
        formality: "neutral",
        density_fit: "balanced",
        best_for: "strategy updates, product reviews, and modern technical narratives",
        avoid_for: "very dense reading decks or warm editorial talks",
    },
    ThemeMetadata {
        name: "ledger",
        description: "Warm paper theme with serif-led hierarchy and fine editorial rules.",
        mood: "measured",
        formality: "formal",
        density_fit: "dense",
        best_for: "board updates, research summaries, financial reviews, and reading-heavy briefs",
        avoid_for: "high-energy keynotes or code-heavy live demos",
    },
    ThemeMetadata {
        name: "terminal",
        description: "Dark engineering theme with monospace accents and phosphor-green emphasis.",
        mood: "technical",
        formality: "neutral",
        density_fit: "balanced",
        best_for: "architecture walkthroughs, incident reviews, infrastructure plans, and developer talks",
        avoid_for: "formal board materials or image-led inspirational decks",
    },
    ThemeMetadata {
        name: "poster",
        description: "High-contrast keynote theme with oversized type and a safety-orange accent.",
        mood: "assertive",
        formality: "casual",
        density_fit: "sparse",
        best_for: "speaker-led keynotes, launches, rally talks, and memorable section breaks",
        avoid_for: "dense reports, long prose, or subtle analytical comparisons",
    },
];

pub fn theme_metadata() -> &'static [ThemeMetadata] {
    THEMES
}

fn builtin_theme_css(theme: &str) -> Option<&'static str> {
    match theme {
        "signal" => Some(SIGNAL_CSS),
        "ledger" => Some(LEDGER_CSS),
        "terminal" => Some(TERMINAL_CSS),
        "poster" => Some(POSTER_CSS),
        _ => None,
    }
}

#[derive(Debug, Deserialize, PartialEq)]
pub struct DeckToml {
    pub deck: DeckMeta,
    #[serde(default)]
    pub build: BuildConfig,
    #[serde(default)]
    pub images: ImagesConfig,
    #[serde(default)]
    pub fonts: Vec<FontFaceConfig>,
}
#[derive(Debug, Deserialize, PartialEq)]
pub struct DeckMeta {
    pub title: String,
    #[serde(default = "default_theme")]
    pub theme: String,
    pub slides: Option<Vec<String>>,
}
#[derive(Debug, Deserialize, PartialEq)]
pub struct BuildConfig {
    #[serde(default = "default_inline")]
    pub inline_assets: bool,
}

#[derive(Debug, Deserialize, PartialEq, Clone, Copy)]
pub struct ImagesConfig {
    #[serde(default = "default_optimize_images")]
    pub optimize: bool,
    #[serde(default = "default_quality")]
    pub quality: f32,
    #[serde(default = "default_max_dim")]
    pub max_dim: u32,
}

impl Default for ImagesConfig {
    fn default() -> Self {
        Self {
            optimize: true,
            quality: default_quality(),
            max_dim: default_max_dim(),
        }
    }
}

impl Default for BuildConfig {
    fn default() -> Self {
        Self {
            inline_assets: true,
        }
    }
}
fn default_theme() -> String {
    "signal".into()
}
fn default_inline() -> bool {
    true
}
fn default_optimize_images() -> bool {
    true
}
fn default_quality() -> f32 {
    80.0
}
fn default_max_dim() -> u32 {
    3840
}

pub fn parse_deck_toml(s: &str) -> anyhow::Result<DeckToml> {
    Ok(toml::from_str(s)?)
}

pub fn new_deck(dir: &Path, theme: &str) -> anyhow::Result<()> {
    let theme_css = builtin_theme_css(theme).ok_or_else(|| {
        anyhow::anyhow!(
            "unknown built-in theme '{theme}' (available: {})",
            THEMES.iter().map(|t| t.name).collect::<Vec<_>>().join(", ")
        )
    })?;
    fs::create_dir_all(dir.join("slides"))?;
    fs::create_dir_all(dir.join("assets"))?;
    fs::create_dir_all(dir)?;
    fs::write(
        dir.join("deck.toml"),
        format!(
            "[deck]\ntitle = \"{} Demo\"\ntheme = \"{}\"\n\n[build]\ninline_assets = true\n\n# Declare non-system TrueType faces explicitly; keep sources under assets/.\n# [[fonts]]\n# source = \"assets/example-regular.ttf\"\n# family = \"Example Sans\"\n# style = \"normal\"\n# weight = 400\n",
            title_case(theme),
            theme
        ),
    )?;
    fs::write(dir.join("theme.css"), theme_css)?;
    fs::write(
        dir.join("slides/01-title.html"),
        format!(
            "<div class=\"slide-center\">\n  <p class=\"kicker\">sideshow</p>\n  <h1 class=\"text-7xl font-semibold tracking-tight\">{} Demo</h1>\n  <p class=\"mt-8 text-3xl text-muted\" data-step>One self-contained HTML deck.</p>\n</div>\n<template data-notes>Welcome the audience and frame the deck.</template>\n",
            title_case(theme)
        ),
    )?;
    fs::write(
        dir.join("slides/02-content.md"),
        "# Build loop\n\n- Write HTML or markdown slide fragments\n- Run `sideshow build .`\n- Share one offline HTML file\n\n<div class=\"stat\" data-step>1920×1080</div>\n",
    )?;
    fs::write(dir.join("assets/.gitkeep"), "")?;
    Ok(())
}

fn title_case(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

pub fn slide_order(dir: &Path, deck: &DeckToml) -> anyhow::Result<Vec<PathBuf>> {
    let canonical_root = dir
        .canonicalize()
        .with_context(|| format!("failed to resolve deck root {}", dir.display()))?;
    let mut paths = if let Some(slides) = &deck.deck.slides {
        slides.iter().map(|slide| dir.join(slide)).collect()
    } else {
        let mut paths = fs::read_dir(dir.join("slides"))?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                matches!(
                    path.extension().and_then(|extension| extension.to_str()),
                    Some("html" | "md")
                )
            })
            .collect::<Vec<_>>();
        paths.sort();
        paths
    };
    for path in &mut paths {
        let requested = path.clone();
        let canonical = requested
            .canonicalize()
            .with_context(|| format!("failed to resolve slide source {}", requested.display()))?;
        let metadata = canonical
            .metadata()
            .with_context(|| format!("failed to inspect slide source {}", canonical.display()))?;
        if !metadata.is_file() || !canonical.starts_with(&canonical_root) {
            bail!(
                "slide source must be a regular file inside deck root {}: {}",
                canonical_root.display(),
                requested.display()
            );
        }
        *path = canonical;
    }
    Ok(paths)
}

pub fn validate_fragment(path: &Path, html: &str) -> anyhow::Result<()> {
    if let Some(tag) = forbidden_tags(html)?.into_iter().next() {
        bail!(
            "{} violates fragment contract: forbidden <{}> tag",
            path.display(),
            tag
        );
    }
    Ok(())
}

fn forbidden_tags(html: &str) -> anyhow::Result<Vec<String>> {
    let mut tags = Vec::new();
    let result = lol_html::rewrite_str(
        html,
        RewriteStrSettings {
            element_content_handlers: vec![element!("html, head, script", |el| {
                tags.push(el.tag_name().to_ascii_lowercase());
                Ok(())
            })],
            ..RewriteStrSettings::default()
        },
    );
    result.map_err(|e| anyhow::anyhow!(e.to_string()))?;
    Ok(tags)
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct CheckFinding {
    pub path: String,
    pub severity: FindingSeverity,
    pub kind: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum FindingSeverity {
    Error,
    Warning,
}

fn asset_refs(input: &str) -> anyhow::Result<Vec<String>> {
    let mut refs = Vec::new();
    lol_html::rewrite_str(
        input,
        RewriteStrSettings {
            element_content_handlers: vec![element!("*[src], *[href], *[srcset]", |el| {
                for name in ["src", "href"] {
                    if let Some(value) = el.get_attribute(name)
                        && let Some(value) = asset_ref_without_suffix(&value)
                    {
                        refs.push(value);
                    }
                }
                if let Some(value) = el.get_attribute("srcset") {
                    refs.extend(
                        srcset_urls(&value).filter_map(|url| asset_ref_without_suffix(&url)),
                    );
                }
                Ok(())
            })],
            ..RewriteStrSettings::default()
        },
    )
    .map_err(|e| anyhow::anyhow!(e.to_string()))?;
    refs.extend(
        CSS_URL_ASSET_RE
            .captures_iter(input)
            .filter_map(|c| css_asset_capture_path(&c).and_then(asset_ref_without_suffix)),
    );
    Ok(refs)
}

fn safe_inline_svg_text(deck_dir: &Path, input: &str) -> String {
    let mut refs = Vec::new();
    if lol_html::rewrite_str(
        input,
        RewriteStrSettings {
            element_content_handlers: vec![element!("img[src]", |el| {
                if let Some(source) = el
                    .get_attribute("src")
                    .and_then(|value| asset_ref_without_suffix(&value))
                    .filter(|value| value.to_ascii_lowercase().ends_with(".svg"))
                {
                    refs.push(source);
                }
                Ok(())
            })],
            ..RewriteStrSettings::default()
        },
    )
    .is_err()
    {
        return String::new();
    }
    refs.into_iter()
        .filter_map(|source| {
            let path = validate_asset_path(deck_dir, &source).ok()?;
            let svg = fs::read_to_string(path).ok()?;
            validate_static_svg(&svg).ok()?;
            Some(svg)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn css_asset_capture_path<'a>(captures: &'a Captures<'a>) -> Option<&'a str> {
    captures
        .name("dq")
        .or_else(|| captures.name("sq"))
        .or_else(|| captures.name("bare"))
        .map(|m| m.as_str())
}

fn asset_ref_without_suffix(value: &str) -> Option<String> {
    if !value.starts_with("assets/") {
        return None;
    }
    let end = value.find(['?', '#']).unwrap_or(value.len());
    Some(normalize_asset_ref(&value[..end]).unwrap_or_else(|_| value[..end].to_string()))
}

fn normalize_asset_ref(value: &str) -> anyhow::Result<String> {
    if value.starts_with('/') || value.contains(':') || !value.starts_with("assets/") {
        bail!("asset reference must be relative and start with assets/: {value}");
    }
    let mut parts = Vec::new();
    for part in value.split('/') {
        match part {
            "" | "." => {}
            ".." => bail!("asset reference may not contain parent traversal: {value}"),
            p => parts.push(p),
        }
    }
    if parts.first() != Some(&"assets") || parts.len() < 2 {
        bail!("asset reference must name a file under assets/: {value}");
    }
    Ok(parts.join("/"))
}

fn validate_asset_path(deck_dir: &Path, rel: &str) -> anyhow::Result<PathBuf> {
    let rel = normalize_asset_ref(rel)?;
    let assets = fs::canonicalize(deck_dir.join("assets"))?;
    let path = deck_dir.join(&rel);
    let canon =
        fs::canonicalize(&path).with_context(|| format!("missing asset reference: {rel}"))?;
    if !canon.starts_with(&assets) {
        bail!("asset reference escapes assets directory: {rel}");
    }
    Ok(canon)
}

fn srcset_urls(input: &str) -> impl Iterator<Item = String> + '_ {
    split_srcset(input)
        .into_iter()
        .filter_map(|candidate| candidate.split_whitespace().next().map(str::to_string))
}

fn split_srcset(input: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut in_data = false;
    for (i, ch) in input.char_indices() {
        if input[i..].starts_with("data:") {
            in_data = true;
        }
        if in_data && ch.is_whitespace() {
            in_data = false;
        }
        if ch == ',' && !in_data {
            out.push(&input[start..i]);
            start = i + 1;
        }
    }
    out.push(&input[start..]);
    out
}

fn color_literals(input: &str) -> anyhow::Result<Vec<String>> {
    let color = Regex::new(
        r#"(?ix)
        \#(?:[0-9a-f]{8}|[0-9a-f]{6}|[0-9a-f]{4}|[0-9a-f]{3})\b
        |\b(?:rgb|rgba|hsl|hsla|hwb|lab|lch|oklab|oklch|color|color-mix)\([^)]*\)
        "#,
    )?;
    let style_block = Regex::new(r#"(?is)<style\b[^>]*>(.*?)</style>"#)?;
    let mut literals = BTreeSet::new();
    for captures in style_block.captures_iter(input) {
        if let Some(scope) = captures.get(1) {
            collect_colors_from_scope(&color, &strip_css_comments(scope.as_str()), &mut literals);
        }
    }
    lol_html::rewrite_str(
        input,
        RewriteStrSettings {
            element_content_handlers: vec![element!("*", |el| {
                for attr in el.attributes() {
                    let name = attr.name().to_ascii_lowercase();
                    if name == "style"
                        || name == "class"
                        || matches!(
                            name.as_str(),
                            "fill"
                                | "stroke"
                                | "stop-color"
                                | "flood-color"
                                | "lighting-color"
                                | "color"
                        )
                    {
                        collect_colors_from_scope(
                            &color,
                            &strip_css_comments(&attr.value()),
                            &mut literals,
                        );
                    }
                }
                Ok(())
            })],
            ..RewriteStrSettings::default()
        },
    )
    .map_err(|e| anyhow::anyhow!(e.to_string()))?;
    Ok(literals.into_iter().collect())
}

fn collect_colors_from_scope(color: &Regex, scope: &str, literals: &mut BTreeSet<String>) {
    literals.extend(color.find_iter(scope).map(|m| m.as_str().to_string()));
}

fn strip_css_comments(input: &str) -> String {
    CSS_COMMENT_RE.replace_all(input, "").into_owned()
}

fn orphaned_assets(dir: &Path, refs: &BTreeSet<String>) -> Vec<String> {
    fn visit(root: &Path, dir: &Path, refs: &BTreeSet<String>, out: &mut Vec<String>) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with('.'))
            {
                continue;
            }
            if path.is_dir() {
                visit(root, &path, refs, out);
            } else if path.is_file() {
                let rel = path
                    .strip_prefix(root)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .to_string();
                if !refs.contains(&rel) {
                    out.push(rel);
                }
            }
        }
    }

    let assets_dir = dir.join("assets");
    if !assets_dir.is_dir() {
        return Vec::new();
    }
    let mut out = Vec::new();
    visit(dir, &assets_dir, refs, &mut out);
    out.sort();
    out
}

pub fn check_deck(dir: &Path) -> Vec<CheckFinding> {
    let mut findings = Vec::new();
    let deck_path = dir.join("deck.toml");
    let deck_text = match fs::read_to_string(&deck_path) {
        Ok(s) => s,
        Err(e) => {
            findings.push(CheckFinding {
                path: deck_path.display().to_string(),
                severity: FindingSeverity::Error,
                kind: "deck_toml".into(),
                message: format!("cannot read deck.toml: {e}"),
            });
            return findings;
        }
    };
    let deck = match parse_deck_toml(&deck_text) {
        Ok(d) => d,
        Err(e) => {
            findings.push(CheckFinding {
                path: deck_path.display().to_string(),
                severity: FindingSeverity::Error,
                kind: "deck_toml".into(),
                message: format!("deck.toml does not parse: {e}"),
            });
            return findings;
        }
    };
    let slides_dir = dir.join("slides");
    let slides = match slide_order(dir, &deck) {
        Ok(s) => s,
        Err(e) => {
            if let Some(configured) = &deck.deck.slides {
                for slide in configured {
                    let path = dir.join(slide);
                    if !path.is_file() {
                        findings.push(CheckFinding {
                            path: path.display().to_string(),
                            severity: FindingSeverity::Error,
                            kind: "missing_slide".into(),
                            message: "referenced slide does not exist or is not a regular file"
                                .into(),
                        });
                    }
                }
            }
            findings.push(CheckFinding {
                path: slides_dir.display().to_string(),
                severity: FindingSeverity::Error,
                kind: "slides".into(),
                message: format!("cannot read slides: {e}"),
            });
            Vec::new()
        }
    };
    if slides.is_empty() {
        findings.push(CheckFinding {
            path: slides_dir.display().to_string(),
            severity: FindingSeverity::Error,
            kind: "empty_slides".into(),
            message: "slides directory contains no .html or .md slides".into(),
        });
    }
    let mut stems = std::collections::HashMap::<String, PathBuf>::new();
    let mut all_refs = BTreeSet::new();
    let mut ordinary_refs = BTreeSet::new();
    let mut rendered_content = String::new();
    for p in slides {
        let rel = p
            .strip_prefix(dir)
            .unwrap_or(&p)
            .to_string_lossy()
            .to_string();
        if !p.is_file() {
            findings.push(CheckFinding {
                path: rel,
                severity: FindingSeverity::Error,
                kind: "missing_slide".into(),
                message: "referenced slide does not exist".into(),
            });
            continue;
        }
        if let Some(stem) = p.file_stem().and_then(|s| s.to_str())
            && let Some(first) = stems.insert(stem.to_string(), p.clone())
        {
            findings.push(CheckFinding {
                path: rel.clone(),
                severity: FindingSeverity::Error,
                kind: "duplicate_slide_id".into(),
                message: format!(
                    "duplicate generated slide id s-{stem}; first seen at {}",
                    first.strip_prefix(dir).unwrap_or(&first).display()
                ),
            });
        }
        let raw = match fs::read_to_string(&p) {
            Ok(s) => s,
            Err(e) => {
                findings.push(CheckFinding {
                    path: rel,
                    severity: FindingSeverity::Error,
                    kind: "slide_read".into(),
                    message: format!("cannot read slide: {e}"),
                });
                continue;
            }
        };
        match forbidden_tags(&raw) {
            Ok(mut tags) => {
                tags.sort();
                tags.dedup();
                for tag in tags {
                    findings.push(CheckFinding {
                        path: rel.clone(),
                        severity: FindingSeverity::Error,
                        kind: "fragment_contract".into(),
                        message: format!("forbidden <{tag}> tag"),
                    });
                }
            }
            Err(e) => findings.push(CheckFinding {
                path: rel.clone(),
                severity: FindingSeverity::Error,
                kind: "fragment_parser".into(),
                message: format!("could not parse slide fragment: {e}"),
            }),
        }
        let rendered = if p.extension().and_then(|s| s.to_str()) == Some("md") {
            let mut plugins = Plugins::default();
            let adapter = highlight::Highlighter;
            plugins.render.codefence_syntax_highlighter = Some(&adapter);
            markdown_to_html_with_plugins(&raw, &Options::default(), &plugins)
        } else {
            raw.clone()
        };
        rendered_content.push_str(&rendered);
        rendered_content.push_str(&safe_inline_svg_text(dir, &rendered));
        if let Ok(literals) = color_literals(&rendered) {
            for literal in literals {
                findings.push(CheckFinding {
                    path: rel.clone(),
                    severity: FindingSeverity::Warning,
                    kind: "off_token_color".into(),
                    message: format!("hardcoded color {literal}; use theme tokens instead"),
                });
            }
        }
        match asset_refs(&rendered) {
            Ok(refs) => {
                for r in refs {
                    all_refs.insert(r.clone());
                    ordinary_refs.insert(r.clone());
                    match validate_asset_path(dir, &r) {
                        Err(e) => findings.push(CheckFinding {
                            path: rel.clone(),
                            severity: FindingSeverity::Error,
                            kind: "missing_asset".into(),
                            message: format!("invalid asset reference {r}: {e}"),
                        }),
                        Ok(asset) => {
                            if let Ok(size) = fs::metadata(asset).map(|m| m.len()) {
                                let projected = projected_data_uri_size(size, mime_for(&r));
                                if projected > 500 * 1024 {
                                    findings.push(CheckFinding {
                                        path: r.clone(),
                                        severity: FindingSeverity::Warning,
                                        kind: "asset_size_budget".into(),
                                        message: format!(
                                            "projected inlined asset size is {} bytes (> 500KB)",
                                            projected
                                        ),
                                    });
                                }
                            }
                        }
                    }
                }
            }
            Err(e) => findings.push(CheckFinding {
                path: rel.clone(),
                severity: FindingSeverity::Error,
                kind: "asset_parser".into(),
                message: format!("could not parse asset references: {e}"),
            }),
        }
    }
    let font_refs = fonts::source_refs(&deck.fonts);
    all_refs.extend(font_refs.iter().cloned());
    let mut embedded_fonts = Vec::new();
    if !deck.fonts.is_empty() {
        match fs::read_to_string(dir.join("theme.css"))
            .context("cannot read theme.css")
            .and_then(|theme| {
                fonts::glyph_corpus(
                    &deck.deck.title,
                    &rendered_content,
                    &format!("{STAGE_CSS}\n{PLAN_CSS}\n{theme}"),
                )
            })
            .and_then(|corpus| fonts::prepare_fonts(dir, &deck.fonts, &corpus))
        {
            Ok(prepared) => embedded_fonts = prepared,
            Err(e) => findings.push(CheckFinding {
                path: deck_path.display().to_string(),
                severity: FindingSeverity::Error,
                kind: "font".into(),
                message: e.to_string(),
            }),
        }
    }
    for face in &deck.fonts {
        let Ok(source) = normalize_asset_ref(&face.source) else {
            continue;
        };
        let Some(size) = validate_asset_path(dir, &source)
            .ok()
            .and_then(|path| fs::metadata(path).ok())
            .map(|metadata| metadata.len())
        else {
            continue;
        };
        if size > 500 * 1024 {
            findings.push(CheckFinding {
                path: source,
                severity: FindingSeverity::Warning,
                kind: "asset_size_budget".into(),
                message: format!("font source size is {size} bytes (> 500KB)"),
            });
        }
    }
    for font in &embedded_fonts {
        let generated = font.projected_inline_size();
        if generated > 500 * 1024 {
            findings.push(CheckFinding {
                path: font.source.clone(),
                severity: FindingSeverity::Warning,
                kind: "asset_size_budget".into(),
                message: format!("generated inlined TrueType font is {generated} bytes (> 500KB)"),
            });
        }
    }
    for orphan in orphaned_assets(dir, &all_refs) {
        findings.push(CheckFinding {
            path: orphan.clone(),
            severity: FindingSeverity::Warning,
            kind: "orphaned_asset".into(),
            message: "asset is not referenced by any slide fragment".into(),
        });
    }
    let total: u64 = ordinary_refs
        .iter()
        .filter_map(|r| {
            validate_asset_path(dir, r)
                .ok()
                .and_then(|p| fs::metadata(p).ok())
                .map(|m| projected_data_uri_size(m.len(), mime_for(r)))
        })
        .sum::<u64>()
        + embedded_fonts
            .iter()
            .map(fonts::EmbeddedFont::projected_inline_size)
            .sum::<u64>();
    if total > 10 * 1024 * 1024 {
        findings.push(CheckFinding {
            path: dir.display().to_string(),
            severity: FindingSeverity::Warning,
            kind: "deck_size_budget".into(),
            message: format!(
                "projected total inlined asset size is {} bytes (> 10MB)",
                total
            ),
        });
    }
    findings.extend(check_tapes(dir));
    findings
}

fn check_tapes(dir: &Path) -> Vec<CheckFinding> {
    let tapes_dir = dir.join("tapes");
    if !tapes_dir.is_dir() {
        return Vec::new();
    }
    let Ok(entries) = fs::read_dir(&tapes_dir) else {
        return Vec::new();
    };
    entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("tape"))
        .filter_map(|tape| tape_finding(dir, &tape))
        .collect()
}

fn tape_finding(dir: &Path, tape: &Path) -> Option<CheckFinding> {
    let stem = tape.file_stem()?.to_str()?;
    let rel_tape = format!("tapes/{stem}.tape");
    let rel_out = format!("assets/{stem}.webm");
    let out = dir.join(&rel_out);
    let stale = match (fs::metadata(tape), fs::metadata(&out)) {
        (Ok(_), Err(_)) => true,
        (Ok(tape_meta), Ok(out_meta)) => match (tape_meta.modified(), out_meta.modified()) {
            (Ok(tape_mtime), Ok(out_mtime)) => out_mtime < tape_mtime,
            _ => true,
        },
        _ => false,
    };
    stale.then(|| CheckFinding {
        path: rel_tape,
        severity: FindingSeverity::Warning,
        kind: "tape".into(),
        message: format!(
            "render {rel_out} with `sideshow tape render {}`",
            dir.display()
        ),
    })
}

fn projected_data_uri_size(bytes: u64, mime: &str) -> u64 {
    ("data:;base64,".len() + mime.len()) as u64 + bytes.div_ceil(3) * 4
}

pub fn rewrite_asset_refs(
    deck_dir: &Path,
    input: &str,
    images: ImagesConfig,
) -> anyhow::Result<String> {
    let mut state = RewriteState::default();
    rewrite_asset_refs_with_state(deck_dir, input, images, &mut state)
}

#[derive(Default)]
struct RewriteState {
    svg_instance: usize,
    authored_ids: BTreeSet<String>,
}

fn rewrite_asset_refs_with_state(
    deck_dir: &Path,
    input: &str,
    images: ImagesConfig,
    state: &mut RewriteState,
) -> anyhow::Result<String> {
    let replace_path = |rel: &str| -> anyhow::Result<String> {
        let normalized = normalize_asset_ref(rel)?;
        let mut bytes = fs::read(validate_asset_path(deck_dir, &normalized)?)?;
        let mut mime = mime_for(&normalized);
        if !normalized.ends_with(".svg") && images.optimize && is_raster(&normalized) {
            if is_animated(&bytes) {
                eprintln!(
                    "warning: {normalized} is animated; skipping optimization to preserve animation"
                );
            } else if let Ok(opt) = optimize_bytes(&bytes, images.quality, images.max_dim)
                && opt.len() < bytes.len()
            {
                bytes = opt;
                mime = "image/webp";
            }
        }
        Ok(format!(
            "data:{mime};base64,{}",
            base64::engine::general_purpose::STANDARD.encode(bytes)
        ))
    };
    let out = rewrite_html_asset_attrs(deck_dir, input, &replace_path, state)?;
    let mut err = None;
    let out = CSS_URL_ASSET_RE.replace_all(&out, |c: &Captures| {
        let path = css_asset_capture_path(c).unwrap_or("");
        match asset_ref_without_suffix(path)
            .map_or_else(|| replace_path(path), |p| replace_path(&p))
        {
            Ok(uri) => format!("url({uri})"),
            Err(e) => {
                err = Some(e);
                c[0].to_string()
            }
        }
    });
    if let Some(e) = err {
        return Err(e);
    }
    Ok(out.into_owned())
}

fn rewrite_html_asset_attrs<F>(
    deck_dir: &Path,
    input: &str,
    replace_path: &F,
    state: &mut RewriteState,
) -> anyhow::Result<String>
where
    F: Fn(&str) -> anyhow::Result<String>,
{
    let mut err = None;
    let out = lol_html::rewrite_str(
        input,
        RewriteStrSettings {
            element_content_handlers: vec![element!("*[src], *[href], *[srcset]", |el| {
                if el.tag_name().eq_ignore_ascii_case("img")
                    && let Some(src) = el.get_attribute("src")
                    && asset_ref_without_suffix(&src)
                        .is_some_and(|s| s.to_ascii_lowercase().ends_with(".svg"))
                {
                    state.svg_instance += 1;
                    match inline_svg_for_img(deck_dir, &src, state.svg_instance, el, state) {
                        Ok(Some(svg)) => {
                            el.replace(&svg, ContentType::Html);
                            return Ok(());
                        }
                        Ok(None) => {}
                        Err(e) => eprintln!("warning: SVG {} kept passive: {e}", src),
                    }
                }
                for name in ["src", "href"] {
                    if let Some(value) = el.get_attribute(name)
                        && let Some(path) = asset_ref_without_suffix(&value)
                    {
                        match replace_path(&path) {
                            Ok(uri) => el.set_attribute(name, &uri)?,
                            Err(e) => err = Some(e),
                        }
                    }
                }
                if let Some(value) = el.get_attribute("srcset") {
                    let rewritten = rewrite_srcset(&value, replace_path, &mut err);
                    el.set_attribute("srcset", &rewritten)?;
                }
                Ok(())
            })],
            ..RewriteStrSettings::default()
        },
    )
    .map_err(|e| anyhow::anyhow!(e.to_string()))?;
    if let Some(e) = err { Err(e) } else { Ok(out) }
}

fn inline_svg_for_img(
    deck_dir: &Path,
    rel: &str,
    instance: usize,
    img: &mut lol_html::html_content::Element,
    state: &RewriteState,
) -> anyhow::Result<Option<String>> {
    let rel = asset_ref_without_suffix(rel).context("invalid svg asset reference")?;
    let raw = fs::read_to_string(validate_asset_path(deck_dir, &rel)?)?;
    validate_static_svg(&raw)?;
    let prefix = svg_prefix(
        &raw,
        instance,
        &img.get_attribute("id"),
        &state.authored_ids,
    )?;
    let mut svg = sanitize_svg_markup(&raw, &prefix)?;
    svg = carry_img_attrs_to_svg(svg, img)?;
    Ok(Some(svg))
}

fn validate_static_svg(input: &str) -> anyhow::Result<()> {
    let doc = roxmltree::Document::parse(input).context("SVG must be well-formed XML")?;
    let root = doc.root_element();
    if root.tag_name().name() != "svg"
        || root
            .tag_name()
            .namespace()
            .unwrap_or("http://www.w3.org/2000/svg")
            != "http://www.w3.org/2000/svg"
    {
        bail!("SVG must have one <svg> root");
    }
    for n in doc.descendants().filter(|n| n.is_element()) {
        let name = n.tag_name().name().to_ascii_lowercase();
        let ns = n.tag_name().namespace().unwrap_or("");
        if !ns.is_empty()
            && ns != "http://www.w3.org/2000/svg"
            && ns != "http://www.w3.org/1999/xlink"
        {
            bail!("SVG contains foreign namespace");
        }
        if matches!(
            name.as_str(),
            "script"
                | "foreignobject"
                | "iframe"
                | "object"
                | "embed"
                | "audio"
                | "video"
                | "canvas"
                | "set"
                | "discard"
        ) {
            bail!("SVG contains active or embedded document element <{name}>");
        }
        if name.starts_with("animate") {
            bail!("SVG contains animation/mutation element <{name}>");
        }
        if name == "style" {
            bail!("SVG contains <style>; keeping passive image instead of sanitizing SVG CSS");
        }
        for a in n.attributes() {
            let an = a.name().to_ascii_lowercase();
            let v = a.value().trim();
            let vl = v.to_ascii_lowercase();
            if an == "style" {
                bail!(
                    "SVG contains source style attribute; keeping passive image instead of sanitizing SVG CSS"
                );
            }
            if an.starts_with("on") {
                bail!("SVG contains event handler attribute {an}");
            }
            if matches!(
                an.as_str(),
                "href" | "xlink:href" | "src" | "action" | "formaction"
            ) && !(v.is_empty() || v.starts_with('#'))
            {
                bail!("SVG contains external resource/navigation reference in {an}");
            }
            if vl.contains("url(") && !LOCAL_URL_RE.is_match(v) {
                bail!("SVG contains unsupported external url() reference");
            }
        }
    }
    Ok(())
}

fn svg_prefix(
    input: &str,
    instance: usize,
    img_id: &Option<String>,
    authored_ids: &BTreeSet<String>,
) -> anyhow::Result<String> {
    let doc = roxmltree::Document::parse(input)?;
    let mut existing = doc
        .descendants()
        .filter_map(|n| n.attribute("id"))
        .map(str::to_string)
        .collect::<BTreeSet<_>>();
    existing.extend(authored_ids.iter().cloned());
    if let Some(id) = img_id {
        existing.insert(id.clone());
    }
    for salt in 0..1000 {
        let prefix = if salt == 0 {
            format!("sideshow-svg-{instance}-")
        } else {
            format!("sideshow-svg-{instance}-{salt}-")
        };
        if existing.iter().all(|id| !id.starts_with(&prefix)) {
            return Ok(prefix);
        }
    }
    bail!("could not allocate collision-free SVG id prefix")
}

fn sanitize_svg_markup(input: &str, prefix: &str) -> anyhow::Result<String> {
    let doc = roxmltree::Document::parse(input).context("SVG must be well-formed XML")?;
    let ids = doc
        .descendants()
        .filter_map(|n| n.attribute("id"))
        .map(|id| (id.to_string(), format!("{prefix}{id}")))
        .collect::<BTreeMap<_, _>>();
    lol_html::rewrite_str(
        input,
        RewriteStrSettings {
            element_content_handlers: vec![element!("*", |el| {
                let attrs = el
                    .attributes()
                    .iter()
                    .map(|a| (a.name(), a.value()))
                    .collect::<Vec<_>>();
                for (name, value) in attrs {
                    let lower = name.to_ascii_lowercase();
                    let rewritten = if lower == "id" {
                        ids.get(&value).cloned()
                    } else if matches!(lower.as_str(), "href" | "xlink:href") {
                        rewrite_fragment_ref(&value, &ids)
                    } else if matches!(
                        lower.as_str(),
                        "aria-labelledby" | "aria-describedby" | "aria-controls" | "aria-owns"
                    ) {
                        Some(rewrite_idref_list(&value, &ids))
                    } else if lower == "style" || value.contains("url(") {
                        rewrite_local_url_refs(&value, &ids)
                    } else {
                        None
                    };
                    if let Some(new_value) = rewritten {
                        el.set_attribute(&name, &new_value)?;
                    }
                }
                Ok(())
            })],
            ..RewriteStrSettings::default()
        },
    )
    .map_err(|e| anyhow::anyhow!(e.to_string()))
}

fn rewrite_fragment_ref(value: &str, ids: &BTreeMap<String, String>) -> Option<String> {
    value
        .strip_prefix('#')
        .and_then(|id| ids.get(id))
        .map(|new| format!("#{new}"))
}

fn rewrite_idref_list(value: &str, ids: &BTreeMap<String, String>) -> String {
    value
        .split_whitespace()
        .map(|id| ids.get(id).map(String::as_str).unwrap_or(id))
        .collect::<Vec<_>>()
        .join(" ")
}

fn rewrite_local_url_refs(value: &str, ids: &BTreeMap<String, String>) -> Option<String> {
    let mut changed = false;
    let out = LOCAL_URL_RE.replace_all(value, |c: &Captures| {
        if let Some(new) = ids.get(&c[1]) {
            changed = true;
            format!("url(#{new})")
        } else {
            c[0].to_string()
        }
    });
    changed.then(|| out.into_owned())
}

fn carry_img_attrs_to_svg(
    svg: String,
    img: &mut lol_html::html_content::Element,
) -> anyhow::Result<String> {
    let doc = roxmltree::Document::parse(&svg).context("SVG must be well-formed XML")?;
    let root = doc.root_element();
    let root_attrs = root
        .attributes()
        .map(|a| (a.name().to_ascii_lowercase(), a.value().to_string()))
        .collect::<BTreeMap<_, _>>();
    let mut attrs = BTreeMap::<String, String>::new();
    for attr in img.attributes() {
        let name = attr.name();
        let lower = name.to_ascii_lowercase();
        if matches!(lower.as_str(), "src" | "alt") {
            continue;
        }
        if matches!(
            lower.as_str(),
            "class" | "id" | "style" | "width" | "height"
        ) || lower.starts_with("data-")
            || lower.starts_with("aria-")
        {
            attrs.insert(lower, attr.value());
        }
    }
    if let (Some(svg_id), Some(img_id)) = (root_attrs.get("id"), attrs.get("id"))
        && svg_id != img_id
    {
        bail!("SVG root id conflicts with img id; keeping passive image");
    }
    if let Some(alt) = img.get_attribute("alt") {
        if alt.is_empty() {
            attrs.insert("aria-hidden".into(), "true".into());
            attrs.insert("focusable".into(), "false".into());
        } else {
            attrs.insert("role".into(), "img".into());
            attrs.insert("aria-label".into(), alt);
        }
    }
    let mut done = false;
    lol_html::rewrite_str(
        &svg,
        RewriteStrSettings {
            element_content_handlers: vec![element!("svg", |el| {
                if done {
                    return Ok(());
                }
                done = true;
                for (name, value) in &attrs {
                    let value = if name == "class" {
                        match el.get_attribute("class") {
                            Some(existing) if !existing.is_empty() => format!("{existing} {value}"),
                            _ => value.clone(),
                        }
                    } else if name == "style" {
                        match el.get_attribute("style") {
                            Some(existing) if !existing.trim().is_empty() => {
                                format!("{}; {}", existing.trim_end_matches(';'), value)
                            }
                            _ => value.clone(),
                        }
                    } else {
                        value.clone()
                    };
                    if let Some(existing) = el
                        .attributes()
                        .iter()
                        .find(|a| a.name().eq_ignore_ascii_case(name))
                        .map(|a| a.name())
                    {
                        el.set_attribute(&existing, &value)?;
                    } else {
                        el.set_attribute(name, &value)?;
                    }
                }
                Ok(())
            })],
            ..RewriteStrSettings::default()
        },
    )
    .map_err(|e| anyhow::anyhow!(e.to_string()))
}

fn rewrite_srcset<F>(input: &str, replace_path: &F, err: &mut Option<anyhow::Error>) -> String
where
    F: Fn(&str) -> anyhow::Result<String>,
{
    split_srcset(input)
        .into_iter()
        .map(|candidate| {
            let leading = candidate.len() - candidate.trim_start().len();
            let trimmed = candidate.trim_start();
            let url_len = trimmed.find(char::is_whitespace).unwrap_or(trimmed.len());
            let (url, rest) = trimmed.split_at(url_len);
            if let Some(path) = asset_ref_without_suffix(url) {
                match replace_path(&path) {
                    Ok(uri) => format!("{}{}{}", &candidate[..leading], uri, rest),
                    Err(e) => {
                        *err = Some(e);
                        candidate.to_string()
                    }
                }
            } else {
                candidate.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(",")
}

fn html_ids(input: &str) -> anyhow::Result<BTreeSet<String>> {
    let mut ids = BTreeSet::new();
    lol_html::rewrite_str(
        input,
        RewriteStrSettings {
            element_content_handlers: vec![element!("*[id]", |el| {
                if let Some(id) = el.get_attribute("id") {
                    ids.insert(id);
                }
                Ok(())
            })],
            ..RewriteStrSettings::default()
        },
    )
    .map_err(|e| anyhow::anyhow!(e.to_string()))?;
    Ok(ids)
}

pub fn build_deck(dir: &Path) -> anyhow::Result<PathBuf> {
    build_deck_to(dir, &dir.join("dist"))
}

/// Builds a deck into an explicit output directory. Serve uses this to keep an unaccepted build
/// isolated until its input digest and review manifest have both been accepted.
pub fn build_deck_to(dir: &Path, out_dir: &Path) -> anyhow::Result<PathBuf> {
    let canonical_dir = dir
        .canonicalize()
        .with_context(|| format!("failed to resolve deck root {}", dir.display()))?;
    let deck = parse_deck_toml(&fs::read_to_string(dir.join("deck.toml"))?)?;
    let mut sections = String::new();
    let mut rewrite_state = RewriteState::default();
    let slides = slide_order(dir, &deck)?;
    let mut rendered_slides = Vec::new();
    for p in &slides {
        let rel = p
            .strip_prefix(&canonical_dir)
            .with_context(|| format!("slide source escaped deck root: {}", p.display()))?
            .to_string_lossy();
        let stem = p.file_stem().unwrap().to_string_lossy();
        let raw = fs::read_to_string(p)?;
        let html = if p.extension().and_then(|s| s.to_str()) == Some("md") {
            let mut plugins = Plugins::default();
            let adapter = highlight::Highlighter;
            plugins.render.codefence_syntax_highlighter = Some(&adapter);
            markdown_to_html_with_plugins(&raw, &Options::default(), &plugins)
        } else {
            validate_fragment(p, &raw)?;
            raw
        };
        rewrite_state.authored_ids.extend(html_ids(&html)?);
        rendered_slides.push((
            rel.to_string(),
            stem.to_string(),
            p.extension().and_then(|s| s.to_str()) == Some("md"),
            html,
        ));
    }
    let mut slide_sections = Vec::new();
    for (rel, stem, is_md, html) in rendered_slides {
        let html = rewrite_asset_refs_with_state(dir, &html, deck.images, &mut rewrite_state)?;
        let class = if is_md { "slide slide-md" } else { "slide" };
        sections.push_str(&format!(
            "<section class=\"{class}\" id=\"s-{stem}\" data-src=\"{rel}\">\n{html}\n</section>\n"
        ));
        slide_sections.push((rel, stem, class, html));
    }
    let mut css = compile_css(dir)?;
    if !deck.fonts.is_empty() {
        let corpus = fonts::glyph_corpus(&deck.deck.title, &sections, &css)?;
        let font_css = fonts::prepare_fonts(dir, &deck.fonts, &corpus)?
            .iter()
            .map(fonts::EmbeddedFont::css)
            .collect::<String>();
        css = format!("{font_css}{css}");
    }
    fs::create_dir_all(out_dir)?;
    let out = out_dir.join(format!("{}.html", slug(&deck.deck.title)));
    let slide_views = slide_sections
        .iter()
        .map(|(source, stem, class, html)| SlideSectionView {
            class,
            stem,
            source,
            html: TrustedSlideHtml(html),
        })
        .collect::<Vec<_>>();
    let body = render_template(
        &DeckTemplate {
            title: &deck.deck.title,
            css: TrustedCompilerOutput(&css),
            runtime_marker: RUNTIME_MARKER,
            slides: &slide_views,
            runtime_js: TrustedCompilerOutput(RUNTIME_JS),
        },
        &out.display().to_string(),
        "templates/deck.html",
    )?;
    atomic_write(&out, body.as_bytes())?;
    Ok(out)
}

fn atomic_write(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    let tmp = path.with_file_name(format!(
        ".{}.{}.tmp",
        path.file_name().and_then(|s| s.to_str()).unwrap_or("out"),
        std::process::id()
    ));
    let result = (|| -> anyhow::Result<()> {
        fs::write(&tmp, bytes)?;
        fs::rename(&tmp, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

pub fn deck_dist_path(dir: &Path) -> anyhow::Result<PathBuf> {
    let deck = parse_deck_toml(&fs::read_to_string(dir.join("deck.toml"))?)?;
    Ok(dir
        .join("dist")
        .join(format!("{}.html", slug(&deck.deck.title))))
}

fn compile_css(dir: &Path) -> anyhow::Result<String> {
    let tw = find_tool(
        "tailwindcss",
        "SIDESHOW_TAILWINDCSS",
        "sideshow build compiles deck CSS with Tailwind",
        "https://tailwindcss.com/blog/standalone-cli",
    )?;
    let slides_dir = fs::canonicalize(dir.join("slides"))?;
    let slides_source = css_string(&slides_dir.to_string_lossy());
    let tmp = tempfile::Builder::new().prefix("sideshow-").tempdir()?;
    let input = tmp.path().join("entry.css");
    let output = tmp.path().join("out.css");
    fs::write(
        &input,
        format!(
            "@import \"tailwindcss\" source(none);\n@source {};\n{}\n{}\n{}\n",
            slides_source,
            STAGE_CSS,
            PLAN_CSS,
            fs::read_to_string(dir.join("theme.css"))?
        ),
    )?;
    let status = Command::new(tw)
        .args(["-i"])
        .arg(&input)
        .args(["-o"])
        .arg(&output)
        .args(["--minify"])
        .status()?;
    if !status.success() {
        bail!("tailwindcss failed while compiling deck CSS");
    }
    Ok(strip_leading_css_banner_comments(&fs::read_to_string(output)?).to_string())
}

fn strip_leading_css_banner_comments(mut css: &str) -> &str {
    let trimmed = css.trim_start();
    let Some(after_open) = trimmed.strip_prefix("/*!") else {
        return css;
    };
    let Some(end) = after_open.find("*/") else {
        return css;
    };
    let banner = &after_open[..end].to_ascii_lowercase();
    if banner.contains("tailwindcss") {
        css = &after_open[end + 2..];
    }
    css
}

fn css_string(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}
fn slug(s: &str) -> String {
    let mut out = s
        .to_ascii_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect::<String>();
    while out.contains("--") {
        out = out.replace("--", "-");
    }
    out.trim_matches('-').to_string()
}
fn mime_for(p: &str) -> &'static str {
    match Path::new(p)
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
    {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        "webp" => "image/webp",
        "webm" => "video/webm",
        "mp4" => "video/mp4",
        "css" => "text/css",
        _ => "application/octet-stream",
    }
}

fn is_raster(p: &str) -> bool {
    matches!(
        Path::new(p)
            .extension()
            .and_then(|s| s.to_str())
            .map(|s| s.to_ascii_lowercase())
            .as_deref(),
        Some("png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "tiff")
    )
}

#[derive(Debug, Serialize)]
pub struct ImageInfo {
    pub path: String,
    pub width: u32,
    pub height: u32,
    pub format: String,
    pub file_size: u64,
    pub projected_inline_size: u64,
}

pub fn image_info(path: &Path) -> anyhow::Result<ImageInfo> {
    let bytes = fs::read(path)?;
    let reader = image::ImageReader::new(std::io::Cursor::new(&bytes)).with_guessed_format()?;
    let fmt = reader.format();
    let img = reader.decode()?;
    let mime = mime_for(path.to_string_lossy().as_ref());
    Ok(ImageInfo {
        path: path.display().to_string(),
        width: img.width(),
        height: img.height(),
        format: fmt
            .map(|f| format!("{f:?}"))
            .unwrap_or_else(|| "unknown".into()),
        file_size: bytes.len() as u64,
        projected_inline_size: projected_data_uri_size(bytes.len() as u64, mime),
    })
}

pub fn resize_image(
    path: &Path,
    width: u32,
    height: Option<u32>,
    out: Option<&Path>,
) -> anyhow::Result<PathBuf> {
    let img = image::open(path)?;
    let h = height.unwrap_or_else(|| {
        ((img.height() as f64) * (width as f64 / img.width() as f64))
            .round()
            .max(1.0) as u32
    });
    let resized = img.resize_exact(width, h, image::imageops::FilterType::Lanczos3);
    let dest = out.unwrap_or(path);
    resized.save(dest)?;
    Ok(dest.to_path_buf())
}

pub fn crop_image(path: &Path, rect: &str, out: Option<&Path>) -> anyhow::Result<PathBuf> {
    let (w, h, x, y, center) = parse_rect(rect)?;
    let img = image::open(path)?;
    let x = if center {
        (img.width().saturating_sub(w)) / 2
    } else {
        x
    };
    let y = if center {
        (img.height().saturating_sub(h)) / 2
    } else {
        y
    };
    if x + w > img.width() || y + h > img.height() {
        bail!("crop rectangle exceeds image bounds");
    }
    let cropped = img.crop_imm(x, y, w, h);
    let dest = out.unwrap_or(path);
    cropped.save(dest)?;
    Ok(dest.to_path_buf())
}

fn parse_rect(s: &str) -> anyhow::Result<(u32, u32, u32, u32, bool)> {
    let re = Regex::new(r"^(\d+)x(\d+)(?:\+(\d+)\+(\d+))?$")?;
    let c = re.captures(s).context("rect must be WxH+X+Y or WxH")?;
    Ok((
        c[1].parse()?,
        c[2].parse()?,
        c.get(3)
            .map(|m| m.as_str().parse())
            .transpose()?
            .unwrap_or(0),
        c.get(4)
            .map(|m| m.as_str().parse())
            .transpose()?
            .unwrap_or(0),
        c.get(3).is_none(),
    ))
}

pub fn is_animated(bytes: &[u8]) -> bool {
    is_animated_gif(bytes) || is_animated_webp(bytes) || is_animated_apng(bytes)
}

fn is_animated_gif(bytes: &[u8]) -> bool {
    if bytes.len() < 13 || !matches!(&bytes[..6], b"GIF87a" | b"GIF89a") {
        return false;
    }
    let mut pos = 13;
    if bytes[10] & 0x80 != 0 {
        pos += 3 * (1usize << ((bytes[10] & 0x07) + 1));
    }
    let mut images = 0;
    while pos < bytes.len() {
        match bytes[pos] {
            0x2c => {
                images += 1;
                if images > 1 {
                    return true;
                }
                if pos + 10 > bytes.len() {
                    return false;
                }
                let packed = bytes[pos + 9];
                pos += 10;
                if packed & 0x80 != 0 {
                    pos += 3 * (1usize << ((packed & 0x07) + 1));
                }
                if pos >= bytes.len() {
                    return false;
                }
                pos += 1;
                if let Some(next) = skip_gif_sub_blocks(bytes, pos) {
                    pos = next;
                } else {
                    return false;
                }
            }
            0x21 => {
                if pos + 2 > bytes.len() {
                    return false;
                }
                if bytes[pos + 1] == 0xff
                    && pos + 14 <= bytes.len()
                    && &bytes[pos + 3..pos + 14] == b"NETSCAPE2.0"
                {
                    return true;
                }
                if let Some(next) = skip_gif_sub_blocks(bytes, pos + 2) {
                    pos = next;
                } else {
                    return false;
                }
            }
            0x3b => return false,
            _ => return false,
        }
    }
    false
}

fn skip_gif_sub_blocks(bytes: &[u8], mut pos: usize) -> Option<usize> {
    while pos < bytes.len() {
        let len = bytes[pos] as usize;
        pos += 1;
        if len == 0 {
            return Some(pos);
        }
        pos = pos.checked_add(len)?;
    }
    None
}

fn is_animated_webp(bytes: &[u8]) -> bool {
    bytes.len() >= 21
        && &bytes[..4] == b"RIFF"
        && &bytes[8..12] == b"WEBP"
        && (&bytes[12..16] == b"VP8X" && bytes[20] & 0x02 != 0
            || bytes.windows(4).any(|w| w == b"ANIM"))
}

fn is_animated_apng(bytes: &[u8]) -> bool {
    if bytes.len() < 8 || &bytes[..8] != b"\x89PNG\r\n\x1a\n" {
        return false;
    }
    let mut pos = 8;
    while pos + 8 <= bytes.len() {
        let len = u32::from_be_bytes(bytes[pos..pos + 4].try_into().unwrap()) as usize;
        let kind = &bytes[pos + 4..pos + 8];
        if kind == b"acTL" {
            return true;
        }
        if kind == b"IDAT" {
            return false;
        }
        pos = match pos.checked_add(12).and_then(|p| p.checked_add(len)) {
            Some(next) => next,
            None => return false,
        };
    }
    false
}

pub fn optimize_bytes(bytes: &[u8], quality: f32, max_dim: u32) -> anyhow::Result<Vec<u8>> {
    if is_animated(bytes) {
        bail!(
            "animated image optimization would flatten animation; skipping to preserve animation"
        );
    }
    let mut img = image::load_from_memory(bytes)?;
    let longest = img.width().max(img.height());
    if longest > max_dim {
        let scale = max_dim as f64 / longest as f64;
        img = img.resize(
            ((img.width() as f64 * scale).round() as u32).max(1),
            ((img.height() as f64 * scale).round() as u32).max(1),
            image::imageops::FilterType::Lanczos3,
        );
    }
    let enc = webp::Encoder::from_image(&img)
        .map_err(|e| anyhow::anyhow!("webp encode setup failed: {e}"))?;
    Ok(enc.encode(quality).to_vec())
}

pub fn optimize_image(
    path: &Path,
    quality: f32,
    max_dim: u32,
    in_place: bool,
) -> anyhow::Result<(PathBuf, u64, Option<u64>)> {
    if path
        .extension()
        .and_then(|s| s.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("svg"))
    {
        bail!(
            "skip SVG: SVG is already inlined as markup/data; prefer inline SVG markup for diagrams"
        );
    }
    let old = fs::read(path)?;
    if is_animated(&old) {
        bail!(
            "{} is animated; skipping optimization to preserve animation",
            path.display()
        );
    }
    let new = optimize_bytes(&old, quality, max_dim)?;
    let out = path.with_extension("webp");
    if new.len() < old.len() {
        fs::write(&out, &new)?;
        if in_place {
            fs::remove_file(path)?;
        }
        Ok((out, old.len() as u64, Some(new.len() as u64)))
    } else {
        Ok((out, old.len() as u64, None))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_tool_uses_env_override_path() {
        let t = tempfile::tempdir().unwrap();
        let tool = t.path().join("custom-tool");
        fs::write(&tool, "#!/bin/sh\n").unwrap();
        let env_var = format!("SIDESHOW_TEST_TOOL_{}_OK", std::process::id());
        unsafe {
            std::env::set_var(&env_var, &tool);
        }

        let found = find_tool("ffmpeg", &env_var, "test needs a tool", "test").unwrap();

        unsafe {
            std::env::remove_var(&env_var);
        }
        assert_eq!(found, tool);
    }

    #[test]
    fn find_tool_errors_when_env_override_is_not_file() {
        let t = tempfile::tempdir().unwrap();
        let missing = t.path().join("missing-tool");
        let env_var = format!("SIDESHOW_TEST_TOOL_{}_MISSING", std::process::id());
        unsafe {
            std::env::set_var(&env_var, &missing);
        }

        let err = find_tool("ffmpeg", &env_var, "test needs a tool", "test")
            .unwrap_err()
            .to_string();

        unsafe {
            std::env::remove_var(&env_var);
        }
        assert!(err.contains(&format!("{env_var} points to")), "{err}");
        assert!(err.contains("but it is not a file"), "{err}");
    }

    #[test]
    fn strips_leading_css_banner_comments() {
        assert_eq!(
            strip_leading_css_banner_comments(
                "/*! tailwindcss v4.1.11 | MIT License | https://tailwindcss.com */\n/*! other */.slide{display:block}"
            ),
            "\n/*! other */.slide{display:block}"
        );
        assert_eq!(
            strip_leading_css_banner_comments("/*! author license */.slide{}"),
            "/*! author license */.slide{}"
        );
        assert_eq!(
            strip_leading_css_banner_comments("/* regular comment */.slide{}"),
            "/* regular comment */.slide{}"
        );
    }

    #[test]
    fn parses_user_config_tools() {
        let config = parse_user_config(
            "[tools]\ntailwindcss = '/tw'\nffmpeg = '/ffmpeg'\nvhs = '/vhs'\n",
            Path::new("config.toml"),
        )
        .unwrap();
        assert_eq!(config.tools.tailwindcss, Some(PathBuf::from("/tw")));
        assert_eq!(config.tools.ffmpeg, Some(PathBuf::from("/ffmpeg")));
        assert_eq!(config.tools.vhs, Some(PathBuf::from("/vhs")));
    }

    #[test]
    fn parses_user_config_defaults_and_unknown_keys() {
        let empty = parse_user_config("", Path::new("config.toml")).unwrap();
        assert_eq!(empty, UserConfig::default());

        let unknown = parse_user_config(
            "unknown = true\n\n[other]\nvalue = 1\n\n[tools]\nffmpeg = '/bin/ffmpeg'\nextra = 'ok'\n",
            Path::new("config.toml"),
        )
        .unwrap();
        assert_eq!(unknown.tools.ffmpeg, Some(PathBuf::from("/bin/ffmpeg")));
        assert_eq!(unknown.tools.tailwindcss, None);
    }

    #[test]
    fn invalid_user_config_error_mentions_path() {
        let path = Path::new("/tmp/sideshow-config.toml");
        let err = parse_user_config("[tools\n", path).unwrap_err().to_string();
        assert!(err.contains(&path.display().to_string()), "{err}");
    }

    #[test]
    fn find_tool_uses_config_path_before_path() {
        let t = tempfile::tempdir().unwrap();
        let tool = t.path().join("ffmpeg");
        fs::write(&tool, "#!/bin/sh\n").unwrap();
        let config = UserConfig {
            tools: ToolsConfig {
                ffmpeg: Some(tool.clone()),
                ..ToolsConfig::default()
            },
            ..UserConfig::default()
        };

        let found = find_tool_with(
            &config,
            Path::new("config.toml"),
            "ffmpeg",
            "SIDESHOW_TEST_TOOL_UNUSED",
            "test needs a tool",
            "test",
        )
        .unwrap();

        assert_eq!(found, tool);
    }

    #[test]
    fn parses_deck() {
        let d = parse_deck_toml("[deck]\ntitle='T'\nslides=['slides/b.md']\n").unwrap();
        assert_eq!(d.deck.title, "T");
        assert!(d.build.inline_assets);
        assert!(d.fonts.is_empty());
    }

    #[test]
    fn parses_multiple_font_faces_without_changing_defaults() {
        let d = parse_deck_toml(
            r#"
                [deck]
                title = "T"

                [[fonts]]
                source = "assets/regular.ttf"
                family = "Example Sans"
                style = "normal"
                weight = 400

                [[fonts]]
                source = "assets/bold.ttf"
                family = "Example Sans"
                style = "italic"
                weight = 700
            "#,
        )
        .unwrap();

        assert_eq!(d.fonts.len(), 2);
        assert_eq!(d.fonts[0].family, "Example Sans");
        assert_eq!(d.fonts[0].style, FontStyle::Normal);
        assert_eq!(d.fonts[0].weight, 400);
        assert_eq!(d.fonts[1].style, FontStyle::Italic);
        assert!(d.build.inline_assets);
        assert_eq!(d.images, ImagesConfig::default());
    }
    #[test]
    fn rejects_forbidden() {
        assert!(
            validate_fragment(Path::new("x.html"), "<div></div><script>")
                .unwrap_err()
                .to_string()
                .contains("script")
        );
    }
    #[test]
    fn orders_slides() {
        let t = tempfile::tempdir().unwrap();
        fs::create_dir_all(t.path().join("slides")).unwrap();
        fs::write(t.path().join("slides/b.md"), "").unwrap();
        fs::write(t.path().join("slides/a.html"), "").unwrap();
        let d = parse_deck_toml("[deck]\ntitle='T'\n").unwrap();
        let names = slide_order(t.path(), &d)
            .unwrap()
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().to_string())
            .collect::<Vec<_>>();
        assert_eq!(names, vec!["a.html", "b.md"]);
    }

    #[test]
    fn configured_slides_must_resolve_to_regular_files_inside_deck() {
        let parent = tempfile::tempdir().unwrap();
        let deck = parent.path().join("deck");
        fs::create_dir_all(deck.join("slides")).unwrap();
        fs::write(parent.path().join("outside.html"), "outside").unwrap();

        let traversal = parse_deck_toml("[deck]\ntitle='T'\nslides=['../outside.html']\n").unwrap();
        let error = slide_order(&deck, &traversal).unwrap_err().to_string();
        assert!(error.contains("inside deck root"), "{error}");

        let directory = parse_deck_toml("[deck]\ntitle='T'\nslides=['slides']\n").unwrap();
        let error = slide_order(&deck, &directory).unwrap_err().to_string();
        assert!(error.contains("regular file"), "{error}");
    }

    #[cfg(unix)]
    #[test]
    fn configured_slides_reject_symlinks_that_escape_deck() {
        use std::os::unix::fs::symlink;

        let parent = tempfile::tempdir().unwrap();
        let deck = parent.path().join("deck");
        fs::create_dir_all(deck.join("slides")).unwrap();
        let outside = parent.path().join("outside.html");
        fs::write(&outside, "outside").unwrap();
        symlink(&outside, deck.join("slides/linked.html")).unwrap();
        let configured =
            parse_deck_toml("[deck]\ntitle='T'\nslides=['slides/linked.html']\n").unwrap();

        let error = slide_order(&deck, &configured).unwrap_err().to_string();
        assert!(error.contains("inside deck root"), "{error}");
    }
    #[test]
    fn rewrites_assets() {
        let t = tempfile::tempdir().unwrap();
        fs::create_dir_all(t.path().join("assets")).unwrap();
        fs::write(t.path().join("assets/a.txt"), "hi").unwrap();
        let out = rewrite_asset_refs(
            t.path(),
            "<img src=\"assets/a.txt\"><div style=\"background:url(assets/a.txt)\">",
            ImagesConfig {
                optimize: false,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(!out.contains("assets/a.txt"));
        assert!(
            out.matches("data:application/octet-stream;base64,aGk=")
                .count()
                == 2
        );
    }

    #[test]
    fn rewrites_css_url_whitespace_quotes_suffixes_and_srcset_suffixes() {
        let t = tempfile::tempdir().unwrap();
        fs::create_dir_all(t.path().join("assets")).unwrap();
        fs::write(t.path().join("assets/a.png"), "hi").unwrap();
        fs::write(t.path().join("assets/my image.png"), "hi").unwrap();
        let out = rewrite_asset_refs(
            t.path(),
            "<div style='background:url( assets/a.png )'></div><div style='background:url(\"assets/a.png\" )'></div><div style='background:url(assets/a.png?x=1#f)'></div><div style='background:URL(assets/a.png)'></div><div style='background:url(\"assets/my image.png\")'></div><img srcset='assets/a.png?x=1 1x, assets/a.png#f 2x'>",
            ImagesConfig { optimize: false, ..Default::default() },
        ).unwrap();
        assert!(!out.contains("assets/a.png"), "{out}");
        assert!(!out.contains("assets/my image.png"), "{out}");
        assert_eq!(
            out.matches("data:image/png;base64,aGk=").count(),
            7,
            "{out}"
        );
        assert_eq!(
            asset_refs("<div style='background:URL( assets/a.png?x=1#f )'></div><div style='background:url(\"assets/my image.png\")'></div>").unwrap(),
            vec!["assets/a.png", "assets/my image.png"]
        );
    }

    #[test]
    fn inline_svg_img_replaces_with_markup_and_carries_attrs() {
        let t = tempfile::tempdir().unwrap();
        fs::create_dir_all(t.path().join("assets")).unwrap();
        fs::write(
            t.path().join("assets/icon.svg"),
            "<svg viewBox='0 0 1 1'><path/></svg>",
        )
        .unwrap();
        let out = rewrite_asset_refs(
            t.path(),
            "<img src='assets/icon.svg' class='logo' width='10' height='11' alt='Logo'>",
            ImagesConfig::default(),
        )
        .unwrap();
        assert!(out.contains("<svg"), "{out}");
        assert!(out.contains("class=\"logo\""), "{out}");
        assert!(out.contains("width=\"10\""), "{out}");
        assert!(out.contains("height=\"11\""), "{out}");
        assert!(out.contains("role=\"img\""), "{out}");
        assert!(out.contains("aria-label=\"Logo\""), "{out}");
        assert!(!out.contains("data:image/svg+xml"), "{out}");
    }

    #[test]
    fn unsafe_svg_falls_back_to_passive_data_uri() {
        let t = tempfile::tempdir().unwrap();
        fs::create_dir_all(t.path().join("assets")).unwrap();
        fs::write(
            t.path().join("assets/icon.svg"),
            "<svg onload='evil()'><script>evil()</script><defs><linearGradient id='g'/></defs><rect fill='url(#g)' href='https://example.com/x'/><use href='#g' xlink:href='http://example.com/y'/></svg>",
        )
        .unwrap();
        let out = rewrite_asset_refs(
            t.path(),
            "<img src='assets/icon.svg'>",
            ImagesConfig::default(),
        )
        .unwrap();
        assert!(out.contains("data:image/svg+xml;base64,"), "{out}");
        assert!(!out.contains("<svg"), "{out}");
    }

    #[test]
    fn inline_svg_instances_get_distinct_id_prefixes() {
        let t = tempfile::tempdir().unwrap();
        fs::create_dir_all(t.path().join("assets")).unwrap();
        fs::write(
            t.path().join("assets/icon.svg"),
            "<svg><g id='a'/><use href='#a'/></svg>",
        )
        .unwrap();
        let out = rewrite_asset_refs(
            t.path(),
            "<img src='assets/icon.svg'><img src='assets/icon.svg'>",
            ImagesConfig::default(),
        )
        .unwrap();
        assert!(out.contains("id=\"sideshow-svg-1-a\""), "{out}");
        assert!(out.contains("id=\"sideshow-svg-2-a\""), "{out}");
        assert!(out.contains("href=\"#sideshow-svg-1-a\""), "{out}");
        assert!(out.contains("href=\"#sideshow-svg-2-a\""), "{out}");
    }

    #[test]
    fn inline_svg_rewrites_url_quotes_whitespace_and_aria_idrefs() {
        let t = tempfile::tempdir().unwrap();
        fs::create_dir_all(t.path().join("assets")).unwrap();
        fs::write(
            t.path().join("assets/icon.svg"),
            r##"<svg xmlns='http://www.w3.org/2000/svg' aria-labelledby='title desc'><title id='title'>T</title><desc id='desc'>D</desc><defs><linearGradient id='g'/><filter id='f'/></defs><rect fill="url('#g')" stroke='url( "#g" )' filter='url( #f )'/></svg>"##,
        ).unwrap();
        let out = rewrite_asset_refs(
            t.path(),
            "<img src='assets/icon.svg'>",
            ImagesConfig::default(),
        )
        .unwrap();
        assert!(
            out.contains("aria-labelledby=\"sideshow-svg-1-title sideshow-svg-1-desc\""),
            "{out}"
        );
        assert!(out.contains("url(#sideshow-svg-1-g)"), "{out}");
        assert!(out.contains("url(#sideshow-svg-1-f)"), "{out}");
    }

    #[test]
    fn inline_svg_rejects_style_id_selectors_and_animation() {
        let t = tempfile::tempdir().unwrap();
        fs::create_dir_all(t.path().join("assets")).unwrap();
        fs::write(t.path().join("assets/style.svg"), "<svg xmlns='http://www.w3.org/2000/svg'><style>#g { fill: red } .x{color:#fff}</style><g id='g'/></svg>").unwrap();
        fs::write(t.path().join("assets/anim.svg"), "<svg xmlns='http://www.w3.org/2000/svg'><rect><animate attributeName='fill' to='url(#g)'/></rect></svg>").unwrap();
        let out = rewrite_asset_refs(
            t.path(),
            "<img src='assets/style.svg'><img src='assets/anim.svg'>",
            ImagesConfig::default(),
        )
        .unwrap();
        assert_eq!(
            out.matches("data:image/svg+xml;base64,").count(),
            2,
            "{out}"
        );
    }

    #[test]
    fn inline_svg_rejects_style_url_bypasses_and_compound_id_selectors() {
        let t = tempfile::tempdir().unwrap();
        fs::create_dir_all(t.path().join("assets")).unwrap();
        fs::write(t.path().join("assets/https.svg"), r#"<svg xmlns='http://www.w3.org/2000/svg'><rect style='fill: url( "https://evil.test/x" )'/></svg>"#).unwrap();
        fs::write(t.path().join("assets/style-url.svg"), "<svg xmlns='http://www.w3.org/2000/svg'><style>.x{fill:url(#gradient)}</style><linearGradient id='gradient'/></svg>").unwrap();
        fs::write(t.path().join("assets/selector.svg"), "<svg xmlns='http://www.w3.org/2000/svg'><style>g#gradient.x{fill:red}</style><g id='gradient'/></svg>").unwrap();
        let out = rewrite_asset_refs(t.path(), "<img src='assets/https.svg'><img src='assets/style-url.svg'><img src='assets/selector.svg'>", ImagesConfig::default()).unwrap();
        assert_eq!(
            out.matches("data:image/svg+xml;base64,").count(),
            3,
            "{out}"
        );
    }

    #[test]
    fn inline_svg_conservative_fallback_for_escaped_css_in_source_styles() {
        let t = tempfile::tempdir().unwrap();
        fs::create_dir_all(t.path().join("assets")).unwrap();
        fs::write(t.path().join("assets/block.svg"), r#"<svg xmlns='http://www.w3.org/2000/svg'><style>.x{fill:u\72l(#g);@im\70ort "https://evil.test/x.css"}</style></svg>"#).unwrap();
        fs::write(t.path().join("assets/attr.svg"), r#"<svg xmlns='http://www.w3.org/2000/svg'><rect style='fill:u\72l(#g); color:red'/></svg>"#).unwrap();
        let out = rewrite_asset_refs(
            t.path(),
            "<img src='assets/block.svg'><img src='assets/attr.svg' style='color: red'>",
            ImagesConfig::default(),
        )
        .unwrap();
        assert_eq!(
            out.matches("data:image/svg+xml;base64,").count(),
            2,
            "{out}"
        );
    }

    #[test]
    fn inline_svg_prefix_avoids_authored_reserved_id_collision() {
        let t = tempfile::tempdir().unwrap();
        fs::create_dir_all(t.path().join("assets")).unwrap();
        fs::write(t.path().join("assets/icon.svg"), "<svg xmlns='http://www.w3.org/2000/svg'><g id='sideshow-svg-1-g'/><g id='g'/><use href='#g'/></svg>").unwrap();
        let out = rewrite_asset_refs(
            t.path(),
            "<img src='assets/icon.svg'>",
            ImagesConfig::default(),
        )
        .unwrap();
        assert!(out.contains("id=\"sideshow-svg-1-1-g\""), "{out}");
        assert!(out.contains("href=\"#sideshow-svg-1-1-g\""), "{out}");
    }

    #[test]
    fn inline_svg_merges_root_class_style_and_img_attrs_take_precedence() {
        let t = tempfile::tempdir().unwrap();
        fs::create_dir_all(t.path().join("assets")).unwrap();
        fs::write(
            t.path().join("assets/icon.svg"),
            "<svg xmlns='http://www.w3.org/2000/svg' class='svg-class' width='1' height='2'></svg>",
        )
        .unwrap();
        let out = rewrite_asset_refs(t.path(), "<img src='assets/icon.svg' class='img-class' style='color: red' width='10' height='20' data-step='1' aria-describedby='x' alt=''>", ImagesConfig::default()).unwrap();
        assert!(out.contains("class=\"svg-class img-class\""), "{out}");
        assert!(out.contains("style=\"color: red\""), "{out}");
        assert!(out.contains("width=\"10\""), "{out}");
        assert!(out.contains("height=\"20\""), "{out}");
        assert!(out.contains("data-step=\"1\""), "{out}");
        assert!(out.contains("aria-hidden=\"true\""), "{out}");
    }

    #[test]
    fn inline_svg_root_id_conflict_falls_back() {
        let t = tempfile::tempdir().unwrap();
        fs::create_dir_all(t.path().join("assets")).unwrap();
        fs::write(
            t.path().join("assets/icon.svg"),
            "<svg xmlns='http://www.w3.org/2000/svg' id='root'><use href='#root'/></svg>",
        )
        .unwrap();
        let out = rewrite_asset_refs(
            t.path(),
            "<img src='assets/icon.svg' id='img-id'>",
            ImagesConfig::default(),
        )
        .unwrap();
        assert!(out.contains("data:image/svg+xml;base64,"), "{out}");
    }

    #[test]
    fn inline_svg_rejects_foreign_object_and_external_css_url() {
        let t = tempfile::tempdir().unwrap();
        fs::create_dir_all(t.path().join("assets")).unwrap();
        for (name, svg) in [
            (
                "foreign.svg",
                "<svg xmlns='http://www.w3.org/2000/svg'><foreignObject/></svg>",
            ),
            (
                "css.svg",
                "<svg xmlns='http://www.w3.org/2000/svg'><style>@import url(https://e.test/a.css)</style></svg>",
            ),
        ] {
            fs::write(t.path().join("assets").join(name), svg).unwrap();
        }
        let out = rewrite_asset_refs(
            t.path(),
            "<img src='assets/foreign.svg'><img src='assets/css.svg'>",
            ImagesConfig::default(),
        )
        .unwrap();
        assert_eq!(
            out.matches("data:image/svg+xml;base64,").count(),
            2,
            "{out}"
        );
    }

    #[test]
    fn asset_refs_normalize_dot_and_strip_query_fragment() {
        let refs = asset_refs("<img src='assets/./a.png?cache=1#frag'><source srcset='assets/./b.png#x 1x, data:image/svg+xml,<svg></svg> 2x'>").unwrap();
        assert_eq!(refs, vec!["assets/a.png", "assets/b.png"]);
        assert!(normalize_asset_ref("assets/../secret.png").is_err());
        assert!(normalize_asset_ref("/assets/a.png").is_err());
    }

    #[test]
    fn malformed_svg_falls_back_to_data_uri() {
        let t = tempfile::tempdir().unwrap();
        fs::create_dir_all(t.path().join("assets")).unwrap();
        fs::write(t.path().join("assets/bad.svg"), "<svg><path <></svg>").unwrap();
        let out = rewrite_asset_refs(
            t.path(),
            "<img src='assets/bad.svg'>",
            ImagesConfig::default(),
        )
        .unwrap();
        assert!(out.contains("data:image/svg+xml;base64,"), "{out}");
    }

    #[test]
    fn non_img_svg_references_stay_data_uris() {
        let t = tempfile::tempdir().unwrap();
        fs::create_dir_all(t.path().join("assets")).unwrap();
        fs::write(t.path().join("assets/icon.svg"), "<svg></svg>").unwrap();
        let out = rewrite_asset_refs(
            t.path(),
            "<a href='assets/icon.svg'></a><source srcset='assets/icon.svg 1x'><div style='background:url(assets/icon.svg)'></div>",
            ImagesConfig::default(),
        )
        .unwrap();
        assert_eq!(
            out.matches("data:image/svg+xml;base64,").count(),
            3,
            "{out}"
        );
    }

    #[test]
    fn rewrites_video_assets_with_video_mime() {
        let t = tempfile::tempdir().unwrap();
        fs::create_dir_all(t.path().join("assets")).unwrap();
        fs::write(t.path().join("assets/demo.webm"), b"webm").unwrap();
        fs::write(t.path().join("assets/demo.mp4"), b"mp4").unwrap();
        let out = rewrite_asset_refs(
            t.path(),
            "<video src=\"assets/demo.webm\"></video><video src='assets/demo.mp4'></video>",
            ImagesConfig::default(),
        )
        .unwrap();
        assert!(out.contains("src=\"data:video/webm;base64,d2VibQ==\""));
        assert!(out.contains("src=\"data:video/mp4;base64,bXA0\""));
    }

    #[test]
    fn asset_refs_parse_html_attrs_with_parser() {
        let refs = asset_refs(
            "<IMG SRC=assets/a.png><a HrEf='assets/b.svg'></a><source SrcSet=\"assets/s.png 1x, data:image/png;base64,AA 2x, assets/l.png 800w\"><div style='background:url(assets/bg.png)'>",
        )
        .unwrap();

        assert_eq!(
            refs,
            vec![
                "assets/a.png",
                "assets/b.svg",
                "assets/s.png",
                "assets/l.png",
                "assets/bg.png"
            ]
        );
    }

    #[test]
    fn rewrites_unquoted_single_quoted_mixed_case_and_srcset_assets() {
        let t = tempfile::tempdir().unwrap();
        fs::create_dir_all(t.path().join("assets")).unwrap();
        for name in ["a.txt", "b.txt", "s.txt", "l.txt"] {
            fs::write(t.path().join("assets").join(name), name).unwrap();
        }

        let out = rewrite_asset_refs(
            t.path(),
            "<IMG SRC=assets/a.txt><a HrEf='assets/b.txt'></a><source SrcSet=\"assets/s.txt 1x, data:image/png;base64,AA 2x, assets/l.txt 800w\">",
            ImagesConfig::default(),
        )
        .unwrap();

        assert!(!out.contains("assets/a.txt"));
        assert!(!out.contains("assets/b.txt"));
        assert!(!out.contains("assets/s.txt"));
        assert!(!out.contains("assets/l.txt"));
        assert!(out.contains("data:image/png;base64,AA 2x"));
        assert_eq!(
            out.matches("data:application/octet-stream;base64,").count(),
            4
        );
    }

    #[test]
    fn data_urls_are_ignored_by_asset_collection_and_rewrite() {
        let input = "<img src='data:image/png;base64,AA'><source srcset='data:image/png;base64,AA 1x, assets/a.png 2x'>";
        assert_eq!(asset_refs(input).unwrap(), vec!["assets/a.png"]);
    }

    #[test]
    fn theme_metadata_lists_all_builtins() {
        let json = serde_json::to_string(theme_metadata()).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        let names = parsed
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(names, vec!["signal", "ledger", "terminal", "poster"]);
    }

    #[test]
    fn plan_component_css_is_compiler_owned_and_js_free() {
        assert!(PLAN_CSS.contains(".plan-shell"));
        assert!(PLAN_CSS.contains(".plan-status[data-state=\"blocked\"]"));
        assert!(PLAN_CSS.contains(".plan-diagram [data-node]"));
        assert!(!PLAN_CSS.to_ascii_lowercase().contains("javascript"));
        assert!(!RUNTIME_JS.contains("plan-"));
    }

    #[test]
    fn builtin_themes_define_plan_contract_tokens() {
        for (name, css) in [
            ("signal", SIGNAL_CSS),
            ("ledger", LEDGER_CSS),
            ("terminal", TERMINAL_CSS),
            ("poster", POSTER_CSS),
        ] {
            for token in ["--plan-good", "--plan-warn", "--plan-risk", "--plan-info"] {
                assert!(css.contains(token), "{name} missing {token}");
            }
        }
    }

    #[test]
    fn new_scaffolds_each_builtin_theme() {
        for theme in ["signal", "ledger", "terminal", "poster"] {
            let t = tempfile::tempdir().unwrap();
            new_deck(t.path(), theme).unwrap();
            let deck = fs::read_to_string(t.path().join("deck.toml")).unwrap();
            assert!(deck.contains(&format!("theme = \"{theme}\"")));
            assert!(
                fs::read_to_string(t.path().join("theme.css"))
                    .unwrap()
                    .contains(".kicker")
            );
        }
    }

    #[test]
    fn build_succeeds_for_each_builtin_theme_when_tailwind_exists() {
        if which::which("tailwindcss").is_err() {
            eprintln!("skipping build smoke test: tailwindcss not on PATH");
            return;
        }
        for theme in ["signal", "ledger", "terminal", "poster"] {
            let t = tempfile::tempdir().unwrap();
            new_deck(t.path(), theme).unwrap();
            let out = build_deck(t.path()).unwrap();
            assert!(out.is_file(), "{} did not build", theme);
        }
    }

    #[test]
    fn empty_font_config_leaves_system_font_build_output_byte_identical() {
        if which::which("tailwindcss").is_err() {
            eprintln!("skipping font compatibility build test: tailwindcss not on PATH");
            return;
        }
        let t = tempfile::tempdir().unwrap();
        minimal_deck(&t);
        fs::write(t.path().join("theme.css"), SIGNAL_CSS).unwrap();
        fs::write(t.path().join("slides/01.html"), "<h1>System font</h1>").unwrap();
        let default_output = fs::read(build_deck(t.path()).unwrap()).unwrap();

        fs::write(
            t.path().join("deck.toml"),
            "fonts = []\n[deck]\ntitle='T'\n",
        )
        .unwrap();
        let explicit_empty_output = fs::read(build_deck(t.path()).unwrap()).unwrap();

        assert_eq!(default_output, explicit_empty_output);
        assert!(
            !explicit_empty_output
                .windows(b"@font-face".len())
                .any(|window| window == b"@font-face")
        );
    }

    #[test]
    fn build_inlines_subsetted_true_type_font_without_source_url_or_mutation() {
        if which::which("tailwindcss").is_err() {
            eprintln!("skipping font build test: tailwindcss not on PATH");
            return;
        }
        let t = tempfile::tempdir().unwrap();
        minimal_deck(&t);
        fs::write(
            t.path().join("theme.css"),
            "body { font-family: 'Tiny Five'; }",
        )
        .unwrap();
        fs::write(
            t.path().join("slides/01.html"),
            "<h1>Café Ελληνικά e\u{301}</h1>",
        )
        .unwrap();
        let fixture = font_fixture();
        fs::write(t.path().join("assets/tiny5.ttf"), fixture).unwrap();
        fs::write(
            t.path().join("deck.toml"),
            r#"[deck]
title = "T"

[[fonts]]
source = "assets/tiny5.ttf"
family = "Tiny Five"
style = "normal"
weight = 400
"#,
        )
        .unwrap();

        let output = fs::read_to_string(build_deck(t.path()).unwrap()).unwrap();

        assert!(output.contains("@font-face{font-family:\"Tiny Five\""));
        assert!(output.contains("data:font/ttf;base64,"));
        assert!(output.contains("format(\"truetype\")"));
        assert!(!output.contains("assets/tiny5.ttf"));
        let encoded = output
            .split("data:font/ttf;base64,")
            .nth(1)
            .unwrap()
            .split(')')
            .next()
            .unwrap();
        let payload = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .unwrap();
        assert!(payload.starts_with(&[0x00, 0x01, 0x00, 0x00]));
        assert_eq!(
            fs::read(t.path().join("assets/tiny5.ttf")).unwrap(),
            fixture
        );
    }

    #[test]
    fn ordinary_ttf_asset_without_font_declarations_keeps_octet_stream_mime() {
        let t = tempfile::tempdir().unwrap();
        minimal_deck(&t);
        fs::write(t.path().join("assets/ordinary.ttf"), b"ordinary asset").unwrap();

        let output = rewrite_asset_refs(
            t.path(),
            "<a href='assets/ordinary.ttf'>download</a>",
            ImagesConfig::default(),
        )
        .unwrap();

        assert!(output.contains("data:application/octet-stream;base64,"));
        assert!(!output.contains("data:font/ttf"));
    }

    #[test]
    fn build_avoids_generated_svg_ids_colliding_with_later_authored_slide_ids() {
        if which::which("tailwindcss").is_err() {
            eprintln!("skipping build SVG id collision test: tailwindcss not on PATH");
            return;
        }
        let t = tempfile::tempdir().unwrap();
        minimal_deck(&t);
        fs::write(t.path().join("theme.css"), "").unwrap();
        fs::write(
            t.path().join("assets/icon.svg"),
            "<svg xmlns='http://www.w3.org/2000/svg'><g id='g'/><use href='#g'/></svg>",
        )
        .unwrap();
        fs::write(
            t.path().join("slides/01.html"),
            "<img src='assets/icon.svg'>",
        )
        .unwrap();
        fs::write(
            t.path().join("slides/02.html"),
            "<div id='sideshow-svg-1-g'></div>",
        )
        .unwrap();

        let out = fs::read_to_string(build_deck(t.path()).unwrap()).unwrap();

        assert!(out.contains("id=\"sideshow-svg-1-1-g\""), "{out}");
        assert!(out.contains("href=\"#sideshow-svg-1-1-g\""), "{out}");
        assert!(
            out.contains("id='sideshow-svg-1-g'") || out.contains("id=\"sideshow-svg-1-g\""),
            "{out}"
        );
    }

    fn minimal_deck(t: &tempfile::TempDir) {
        fs::create_dir_all(t.path().join("slides")).unwrap();
        fs::create_dir_all(t.path().join("assets")).unwrap();
        fs::write(t.path().join("deck.toml"), "[deck]\ntitle='T'\n").unwrap();
    }

    fn font_fixture() -> &'static [u8] {
        include_bytes!("../tests/fixtures/fonts/Tiny5-Regular.ttf")
    }

    #[test]
    fn check_reports_empty_slides() {
        let t = tempfile::tempdir().unwrap();
        minimal_deck(&t);
        assert!(
            check_deck(t.path())
                .iter()
                .any(|f| f.kind == "empty_slides")
        );
    }

    #[test]
    fn check_reports_bad_toml_and_missing_explicit_slide() {
        let t = tempfile::tempdir().unwrap();
        fs::write(t.path().join("deck.toml"), "not toml").unwrap();
        assert!(check_deck(t.path()).iter().any(|f| f.kind == "deck_toml"));
        fs::write(
            t.path().join("deck.toml"),
            "[deck]\ntitle='T'\nslides=['slides/nope.html']\n",
        )
        .unwrap();
        assert!(
            check_deck(t.path())
                .iter()
                .any(|f| f.kind == "missing_slide")
        );
    }

    #[test]
    fn check_reports_all_fragment_and_asset_findings() {
        let t = tempfile::tempdir().unwrap();
        minimal_deck(&t);
        fs::write(t.path().join("slides/01.html"), "<script></script><img src='assets/missing.png'><div style=\"background:url(assets/nope.png)\">").unwrap();
        let findings = check_deck(t.path());
        assert_eq!(
            findings
                .iter()
                .filter(|f| f.kind == "fragment_contract")
                .count(),
            1
        );
        assert_eq!(
            findings
                .iter()
                .filter(|f| f.kind == "missing_asset")
                .count(),
            2
        );
    }

    #[test]
    fn check_uses_rendered_markdown_asset_refs_and_orphans() {
        let t = tempfile::tempdir().unwrap();
        minimal_deck(&t);
        fs::write(t.path().join("assets/used.png"), b"used").unwrap();
        fs::write(t.path().join("assets/orphan.png"), b"orphan").unwrap();
        fs::write(
            t.path().join("slides/01.md"),
            "![alt](assets/used.png?x=1)\n",
        )
        .unwrap();
        let findings = check_deck(t.path());
        assert!(!findings.iter().any(|f| f.kind == "missing_asset"));
        assert!(
            findings
                .iter()
                .any(|f| f.kind == "orphaned_asset" && f.path == "assets/orphan.png")
        );
        assert!(
            !findings
                .iter()
                .any(|f| f.kind == "orphaned_asset" && f.path == "assets/used.png")
        );
    }

    #[test]
    fn check_reports_duplicate_stems() {
        let t = tempfile::tempdir().unwrap();
        minimal_deck(&t);
        fs::write(t.path().join("slides/01.html"), "one").unwrap();
        fs::write(t.path().join("slides/01.md"), "two").unwrap();
        assert!(
            check_deck(t.path())
                .iter()
                .any(|f| f.kind == "duplicate_slide_id")
        );
    }

    #[test]
    fn image_resize_crop_optimize_work() {
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("a.png");
        let img =
            image::RgbaImage::from_fn(64, 32, |x, y| image::Rgba([x as u8, y as u8, 128, 255]));
        img.save(&p).unwrap();
        resize_image(&p, 32, None, None).unwrap();
        let info = image_info(&p).unwrap();
        assert_eq!((info.width, info.height), (32, 16));
        crop_image(&p, "10x10", None).unwrap();
        let info = image_info(&p).unwrap();
        assert_eq!((info.width, info.height), (10, 10));
        let (_out, old, maybe_new) = optimize_image(&p, 80.0, 3840, false).unwrap();
        if let Some(new) = maybe_new {
            assert!(new < old);
        }
    }

    fn animated_gif_bytes() -> Vec<u8> {
        vec![
            b'G', b'I', b'F', b'8', b'9', b'a', 1, 0, 1, 0, 0x80, 0, 0, 0, 0, 0, 255, 255, 255,
            0x21, 0xff, 11, b'N', b'E', b'T', b'S', b'C', b'A', b'P', b'E', b'2', b'.', b'0', 3, 1,
            0, 0, 0, 0x21, 0xf9, 4, 0, 1, 0, 0, 0, 0x2c, 0, 0, 0, 0, 1, 0, 1, 0, 0, 2, 2, 0x44, 1,
            0, 0x21, 0xf9, 4, 0, 1, 0, 0, 0, 0x2c, 0, 0, 0, 0, 1, 0, 1, 0, 0, 2, 2, 0x44, 1, 0,
            0x3b,
        ]
    }

    fn static_gif_bytes() -> Vec<u8> {
        vec![
            b'G', b'I', b'F', b'8', b'9', b'a', 1, 0, 1, 0, 0x80, 0, 0, 0, 0, 0, 255, 255, 255,
            0x2c, 0, 0, 0, 0, 1, 0, 1, 0, 0, 2, 2, 0x44, 1, 0, 0x3b,
        ]
    }

    #[test]
    fn detects_animated_gif_webp_and_apng() {
        assert!(is_animated(&animated_gif_bytes()));
        assert!(!is_animated(&static_gif_bytes()));

        let mut webp = b"RIFF\x16\0\0\0WEBPVP8X\n\0\0\0".to_vec();
        webp.extend([0x02, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        assert!(is_animated(&webp));

        let mut apng = b"\x89PNG\r\n\x1a\n".to_vec();
        apng.extend([0, 0, 0, 13]);
        apng.extend(b"IHDR");
        apng.extend([0; 17]);
        apng.extend([0, 0, 0, 8]);
        apng.extend(b"acTL");
        apng.extend([0; 12]);
        assert!(is_animated(&apng));
    }

    #[test]
    fn animated_assets_inline_without_optimization() {
        let t = tempfile::tempdir().unwrap();
        minimal_deck(&t);
        let gif = animated_gif_bytes();
        fs::write(t.path().join("assets/a.gif"), &gif).unwrap();
        let out = rewrite_asset_refs(
            t.path(),
            "<img src='assets/a.gif'>",
            ImagesConfig::default(),
        )
        .unwrap();
        assert!(out.contains("data:image/gif;base64,"));
        let encoded = out
            .split("data:image/gif;base64,")
            .nth(1)
            .unwrap()
            .split('"')
            .next()
            .unwrap();
        let inlined = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .unwrap();
        assert_eq!(inlined, gif);
        assert!(optimize_bytes(&gif, 80.0, 3840).is_err());
    }

    #[test]
    fn static_images_still_optimize() {
        assert!(!is_animated(&static_gif_bytes()));
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("static.png");
        let img = image::RgbaImage::from_pixel(16, 16, image::Rgba([10, 20, 30, 255]));
        img.save(&p).unwrap();
        let bytes = fs::read(&p).unwrap();
        assert!(!is_animated(&bytes));
        assert!(
            optimize_bytes(&bytes, 80.0, 3840)
                .unwrap()
                .starts_with(b"RIFF")
        );

        let gif = t.path().join("static.gif");
        img.save(&gif).unwrap();
        let bytes = fs::read(&gif).unwrap();
        assert!(!is_animated(&bytes));
        assert!(
            optimize_bytes(&bytes, 80.0, 3840)
                .unwrap()
                .starts_with(b"RIFF")
        );
    }

    #[test]
    fn check_warns_for_asset_budget() {
        let t = tempfile::tempdir().unwrap();
        minimal_deck(&t);
        fs::write(
            t.path().join("slides/01.html"),
            "<img src='assets/big.bin'>",
        )
        .unwrap();
        fs::write(t.path().join("assets/big.bin"), vec![0u8; 400_000]).unwrap();
        assert!(
            check_deck(t.path())
                .iter()
                .any(|f| f.severity == FindingSeverity::Warning && f.kind == "asset_size_budget")
        );
    }

    #[test]
    fn check_accounts_for_font_source_and_generated_payload_without_orphaning() {
        let t = tempfile::tempdir().unwrap();
        minimal_deck(&t);
        fs::write(t.path().join("theme.css"), "").unwrap();
        fs::write(t.path().join("slides/01.html"), "Café e\u{301}").unwrap();
        let mut padded = font_fixture().to_vec();
        padded.resize(520 * 1024, 0);
        fs::write(t.path().join("assets/tiny5.ttf"), padded).unwrap();
        fs::write(
            t.path().join("deck.toml"),
            r#"[deck]
title = "T"

[[fonts]]
source = "assets/tiny5.ttf"
family = "Tiny Five"
style = "normal"
weight = 400
"#,
        )
        .unwrap();

        let findings = check_deck(t.path());

        assert!(findings.iter().any(|finding| {
            finding.kind == "asset_size_budget"
                && finding.path == "assets/tiny5.ttf"
                && finding.message.contains("font source size")
        }));
        assert!(!findings.iter().any(|finding| {
            finding.kind == "orphaned_asset" && finding.path == "assets/tiny5.ttf"
        }));
        assert!(!findings.iter().any(|finding| finding.kind == "font"));
    }

    #[test]
    fn check_reports_missing_font_as_a_font_error() {
        let t = tempfile::tempdir().unwrap();
        minimal_deck(&t);
        fs::write(t.path().join("theme.css"), "").unwrap();
        fs::write(t.path().join("slides/01.html"), "Hello").unwrap();
        fs::write(
            t.path().join("deck.toml"),
            r#"[deck]
title = "T"

[[fonts]]
source = "assets/missing.ttf"
family = "Missing"
style = "normal"
weight = 400
"#,
        )
        .unwrap();

        let findings = check_deck(t.path());
        assert!(findings.iter().any(|finding| {
            finding.kind == "font" && finding.message.contains("assets/missing.ttf")
        }));
    }

    #[test]
    fn check_warns_for_orphaned_assets_but_skips_referenced_and_dotfiles() {
        let t = tempfile::tempdir().unwrap();
        minimal_deck(&t);
        fs::create_dir_all(t.path().join("assets/nested")).unwrap();
        fs::write(t.path().join("assets/used.png"), b"used").unwrap();
        fs::write(t.path().join("assets/srcset.png"), b"srcset").unwrap();
        fs::write(t.path().join("assets/nested/css.png"), b"css").unwrap();
        fs::write(t.path().join("assets/orphan.png"), b"orphan").unwrap();
        fs::write(t.path().join("assets/.scratch.png"), b"scratch").unwrap();
        fs::write(t.path().join("assets/demo.tape"), b"tape").unwrap();
        fs::write(
            t.path().join("slides/01.html"),
            "<img src='assets/used.png' srcset='assets/srcset.png 2x'><div style=\"background:url(assets/nested/css.png)\"></div>",
        )
        .unwrap();

        let findings = check_deck(t.path());
        assert!(findings.iter().any(|f| {
            f.severity == FindingSeverity::Warning
                && f.kind == "orphaned_asset"
                && f.path == "assets/orphan.png"
        }));
        assert!(!findings.iter().any(|f| f.kind == "orphaned_asset"
            && (f.path == "assets/used.png"
                || f.path == "assets/srcset.png"
                || f.path == "assets/nested/css.png"
                // .tape files are now treated like normal assets when placed under assets/.
                || f.path == "assets/.scratch.png")));
        assert!(
            findings
                .iter()
                .any(|f| f.kind == "orphaned_asset" && f.path == "assets/demo.tape")
        );
    }

    #[test]
    fn check_warns_for_off_token_colors_in_fragment_css_surfaces() {
        let t = tempfile::tempdir().unwrap();
        minimal_deck(&t);
        fs::write(
            t.path().join("slides/01.html"),
            r##"
            <section style="color: #fff; border-color: rgb(1, 2, 3); background: var(--color-bg)">
              <style>.x { color: oklch(70% 0.2 140); background: var(--color-accent); }</style>
              <div class="text-[#ff0000] bg-[rgb(1,2,3)] border-[var(--color-panel)]"></div>
              <svg><path fill=#abcd stroke="lab(50% 0 0)" /></svg>
              <pre><code>.demo { color: #123456; }</code></pre>
            </section>
            "##,
        )
        .unwrap();

        let findings = check_deck(t.path());
        let off_token: Vec<_> = findings
            .iter()
            .filter(|f| f.kind == "off_token_color")
            .map(|f| f.message.as_str())
            .collect();
        assert!(off_token.iter().any(|m| m.contains("#fff")));
        assert!(off_token.iter().any(|m| m.contains("rgb(1, 2, 3)")));
        assert!(off_token.iter().any(|m| m.contains("oklch(70% 0.2 140)")));
        assert!(off_token.iter().any(|m| m.contains("#ff0000")));
        assert!(off_token.iter().any(|m| m.contains("rgb(1,2,3)")));
        assert!(off_token.iter().any(|m| m.contains("#abcd")));
        assert!(off_token.iter().any(|m| m.contains("lab(50% 0 0)")));
        assert!(!off_token.iter().any(|m| m.contains("var(--color")));
        assert!(!off_token.iter().any(|m| m.contains("#123456")));
    }

    #[test]
    fn check_warns_for_missing_or_stale_tape_output() {
        let t = tempfile::tempdir().unwrap();
        minimal_deck(&t);
        fs::create_dir_all(t.path().join("tapes")).unwrap();
        let tape = t.path().join("tapes/demo.tape");
        fs::write(&tape, "Output \"assets/demo.webm\"\n").unwrap();

        let findings = check_deck(t.path());
        assert!(findings.iter().any(|f| {
            f.severity == FindingSeverity::Warning
                && f.kind == "tape"
                && f.path == "tapes/demo.tape"
                && f.message.contains("sideshow tape render")
        }));

        std::thread::sleep(std::time::Duration::from_millis(20));
        fs::write(t.path().join("assets/demo.webm"), b"webm").unwrap();
        let findings = check_deck(t.path());
        assert!(!findings.iter().any(|f| f.kind == "tape"));
    }

    #[test]
    fn tools_config_parses_aws_path() {
        let config =
            parse_user_config("[tools]\naws = '/tmp/aws'\n", Path::new("config.toml")).unwrap();
        assert_eq!(config.tools.aws, Some(PathBuf::from("/tmp/aws")));
        assert_eq!(
            config.tools.path_for("aws"),
            Some(&PathBuf::from("/tmp/aws"))
        );
    }
}
