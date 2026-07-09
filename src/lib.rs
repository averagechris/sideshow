use anyhow::{Context, bail};
use base64::Engine;
use comrak::{Options, Plugins, markdown_to_html_with_plugins};
use lol_html::{RewriteStrSettings, element};
use regex::{Captures, Regex};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

mod highlight;

pub const RUNTIME_MARKER: &str = "sideshow-runtime-v1";
const STAGE_CSS: &str = include_str!("runtime/stage.css");
const RUNTIME_JS: &str = include_str!("runtime/runtime.js");
const SIGNAL_CSS: &str = include_str!("themes/signal.css");
const LEDGER_CSS: &str = include_str!("themes/ledger.css");
const TERMINAL_CSS: &str = include_str!("themes/terminal.css");
const POSTER_CSS: &str = include_str!("themes/poster.css");

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
            "[deck]\ntitle = \"{} Demo\"\ntheme = \"{}\"\n\n[build]\ninline_assets = true\n",
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
    if let Some(slides) = &deck.deck.slides {
        return Ok(slides.iter().map(|s| dir.join(s)).collect());
    }
    let mut paths = fs::read_dir(dir.join("slides"))?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| matches!(p.extension().and_then(|s| s.to_str()), Some("html" | "md")))
        .collect::<Vec<_>>();
    paths.sort();
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
    let css = Regex::new(r#"url\(["']?(?P<path>assets/[^)'\"]+)["']?\)"#)?;
    let mut refs = Vec::new();
    lol_html::rewrite_str(
        input,
        RewriteStrSettings {
            element_content_handlers: vec![element!("*[src], *[href], *[srcset]", |el| {
                for name in ["src", "href"] {
                    if let Some(value) = el.get_attribute(name)
                        && value.starts_with("assets/")
                    {
                        refs.push(value);
                    }
                }
                if let Some(value) = el.get_attribute("srcset") {
                    refs.extend(srcset_urls(&value).filter(|url| url.starts_with("assets/")));
                }
                Ok(())
            })],
            ..RewriteStrSettings::default()
        },
    )
    .map_err(|e| anyhow::anyhow!(e.to_string()))?;
    refs.extend(css.captures_iter(input).map(|c| c["path"].to_string()));
    Ok(refs)
}

fn srcset_urls(input: &str) -> impl Iterator<Item = String> + '_ {
    input
        .split(',')
        .filter_map(|candidate| candidate.split_whitespace().next().map(str::to_string))
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
        for tag in unique_forbidden_tags(&raw) {
            findings.push(CheckFinding {
                path: rel.clone(),
                severity: FindingSeverity::Error,
                kind: "fragment_contract".into(),
                message: format!("forbidden <{tag}> tag"),
            });
        }
        if let Ok(refs) = asset_refs(&raw) {
            for r in refs {
                let asset = dir.join(&r);
                if !asset.is_file() {
                    findings.push(CheckFinding {
                        path: rel.clone(),
                        severity: FindingSeverity::Error,
                        kind: "missing_asset".into(),
                        message: format!("asset reference not found: {r}"),
                    });
                } else if let Ok(size) = fs::metadata(&asset).map(|m| m.len()) {
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
    let mut refs = std::collections::BTreeSet::new();
    if let Ok(slides) = slide_order(dir, &deck) {
        for p in slides {
            if let Ok(raw) = fs::read_to_string(&p)
                && let Ok(rs) = asset_refs(&raw)
            {
                refs.extend(rs);
            }
        }
    }
    let total: u64 = refs
        .iter()
        .filter_map(|r| {
            fs::metadata(dir.join(r))
                .ok()
                .map(|m| projected_data_uri_size(m.len(), mime_for(r)))
        })
        .sum();
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

fn unique_forbidden_tags(html: &str) -> Vec<String> {
    let mut v = forbidden_tags(html).unwrap_or_default();
    v.sort();
    v.dedup();
    v
}
fn projected_data_uri_size(bytes: u64, mime: &str) -> u64 {
    ("data:;base64,".len() + mime.len()) as u64 + bytes.div_ceil(3) * 4
}

pub fn rewrite_asset_refs(
    deck_dir: &Path,
    input: &str,
    images: ImagesConfig,
) -> anyhow::Result<String> {
    let css = Regex::new(r#"url\(["']?(?P<path>assets/[^)'\"]+)["']?\)"#)?;
    let replace_path = |rel: &str| -> anyhow::Result<String> {
        let mut bytes = fs::read(deck_dir.join(rel))
            .with_context(|| format!("missing asset reference: {rel}"))?;
        let mut mime = mime_for(rel);
        if rel.ends_with(".svg") {
            eprintln!(
                "info: {rel} is SVG; data URI inlined, but inline SVG markup is usually smaller and more editable"
            );
        } else if images.optimize && is_raster(rel) {
            if is_animated(&bytes) {
                eprintln!(
                    "warning: {rel} is animated; skipping optimization to preserve animation"
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
    let out = rewrite_html_asset_attrs(input, &replace_path)?;
    let mut err = None;
    let out = css.replace_all(&out, |c: &Captures| match replace_path(&c["path"]) {
        Ok(uri) => format!("url({uri})"),
        Err(e) => {
            err = Some(e);
            c[0].to_string()
        }
    });
    if let Some(e) = err {
        return Err(e);
    }
    Ok(out.into_owned())
}

fn rewrite_html_asset_attrs<F>(input: &str, replace_path: &F) -> anyhow::Result<String>
where
    F: Fn(&str) -> anyhow::Result<String>,
{
    let mut err = None;
    let out = lol_html::rewrite_str(
        input,
        RewriteStrSettings {
            element_content_handlers: vec![element!("*[src], *[href], *[srcset]", |el| {
                for name in ["src", "href"] {
                    if let Some(value) = el.get_attribute(name)
                        && value.starts_with("assets/")
                    {
                        match replace_path(&value) {
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

fn rewrite_srcset<F>(input: &str, replace_path: &F, err: &mut Option<anyhow::Error>) -> String
where
    F: Fn(&str) -> anyhow::Result<String>,
{
    input
        .split(',')
        .map(|candidate| {
            let leading = candidate.len() - candidate.trim_start().len();
            let trimmed = candidate.trim_start();
            let url_len = trimmed.find(char::is_whitespace).unwrap_or(trimmed.len());
            let (url, rest) = trimmed.split_at(url_len);
            if url.starts_with("assets/") {
                match replace_path(url) {
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

pub fn build_deck(dir: &Path) -> anyhow::Result<PathBuf> {
    let deck = parse_deck_toml(&fs::read_to_string(dir.join("deck.toml"))?)?;
    let mut sections = String::new();
    for p in slide_order(dir, &deck)? {
        let rel = p.strip_prefix(dir).unwrap_or(&p).to_string_lossy();
        let stem = p.file_stem().unwrap().to_string_lossy();
        let raw = fs::read_to_string(&p)?;
        let html = if p.extension().and_then(|s| s.to_str()) == Some("md") {
            let mut plugins = Plugins::default();
            let adapter = highlight::Highlighter;
            plugins.render.codefence_syntax_highlighter = Some(&adapter);
            markdown_to_html_with_plugins(&raw, &Options::default(), &plugins)
        } else {
            validate_fragment(&p, &raw)?;
            raw
        };
        let html = rewrite_asset_refs(dir, &html, deck.images)?;
        let class = if p.extension().and_then(|s| s.to_str()) == Some("md") {
            "slide slide-md"
        } else {
            "slide"
        };
        sections.push_str(&format!(
            "<section class=\"{class}\" id=\"s-{stem}\" data-src=\"{rel}\">\n{html}\n</section>\n"
        ));
    }
    let css = compile_css(dir)?;
    let out_dir = dir.join("dist");
    fs::create_dir_all(&out_dir)?;
    let out = out_dir.join(format!("{}.html", slug(&deck.deck.title)));
    fs::write(
        &out,
        format!(
            "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n<title>{}</title>\n<style>\n{}\n</style>\n</head>\n<body data-runtime=\"{}\">\n<main id=\"stage\" aria-live=\"polite\">\n{}\n</main>\n<script>\n{}\n</script>\n</body>\n</html>\n",
            escape(&deck.deck.title),
            css,
            RUNTIME_MARKER,
            sections,
            RUNTIME_JS
        ),
    )?;
    Ok(out)
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
    let tmp = std::env::temp_dir().join(format!("sideshow-{}", std::process::id()));
    fs::create_dir_all(&tmp)?;
    let input = tmp.join("entry.css");
    let output = tmp.join("out.css");
    fs::write(
        &input,
        format!(
            "@import \"tailwindcss\" source(none);\n@source {};\n{}\n{}\n",
            slides_source,
            STAGE_CSS,
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
    Ok(fs::read_to_string(output)?)
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
fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
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

    fn minimal_deck(t: &tempfile::TempDir) {
        fs::create_dir_all(t.path().join("slides")).unwrap();
        fs::create_dir_all(t.path().join("assets")).unwrap();
        fs::write(t.path().join("deck.toml"), "[deck]\ntitle='T'\n").unwrap();
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
