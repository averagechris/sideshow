use anyhow::{Context, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

pub const REGISTRY_SCHEMA_VERSION: u32 = 1;
pub const DEFAULT_PACK_SCHEMA_VERSION: u32 = 1;
pub const BUNDLED_DEFAULT_PACK: &str = "sideshow-defaults";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Provenance {
    pub source: String,
    pub pack: String,
    pub pack_schema_version: u32,
}

impl Provenance {
    fn bundled() -> Self {
        Self {
            source: "bundled".to_owned(),
            pack: BUNDLED_DEFAULT_PACK.to_owned(),
            pack_schema_version: DEFAULT_PACK_SCHEMA_VERSION,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RegistryMetadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mood: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub formality: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub density_fit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub best_for: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avoid_for: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub intent: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub accepted_input: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub capabilities: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource_digest: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub files: Vec<ScaffoldFileMetadata>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub props: Vec<PropertySchema>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty", default)]
    pub presets: BTreeMap<String, BTreeMap<String, PropertyValue>>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub file_slots: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum PropertyValue {
    String(String),
    Boolean(bool),
    Integer(i64),
    Number(f64),
    StringList(Vec<String>),
}

impl Eq for PropertyValue {}

impl PropertyValue {
    pub fn as_render_string(&self) -> String {
        match self {
            Self::String(s) => s.clone(),
            Self::Boolean(v) => v.to_string(),
            Self::Integer(v) => v.to_string(),
            Self::Number(v) => v.to_string(),
            Self::StringList(v) => v.join(", "),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PropertySchema {
    pub name: String,
    #[serde(rename = "type")]
    pub ty: PropertyType,
    #[serde(default)]
    pub required: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<PropertyValue>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub values: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum PropertyType {
    String,
    Boolean,
    Integer,
    Number,
    Enum,
    StringList,
    File,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ScaffoldFileMetadata {
    pub path: String,
    pub resource: String,
    pub digest: String,
}

impl RegistryMetadata {
    fn theme(theme: ThemeMetadataOwned, resource_path: String, bytes: &[u8]) -> Self {
        Self {
            description: Some(theme.description),
            mood: Some(theme.mood),
            formality: Some(theme.formality),
            density_fit: Some(theme.density_fit),
            best_for: Some(theme.best_for),
            avoid_for: Some(theme.avoid_for),
            intent: Vec::new(),
            accepted_input: Vec::new(),
            capabilities: Vec::new(),
            resource_path: Some(resource_path),
            resource_digest: Some(sha256_hex(bytes)),
            files: Vec::new(),
            props: Vec::new(),
            presets: BTreeMap::new(),
            file_slots: Vec::new(),
        }
    }

    pub fn resource(
        intent: Vec<String>,
        accepted_input: Vec<String>,
        capabilities: Vec<String>,
        resource_path: Option<String>,
        bytes: Option<&[u8]>,
        files: Vec<ScaffoldFileMetadata>,
    ) -> Self {
        Self {
            description: None,
            mood: None,
            formality: None,
            density_fit: None,
            best_for: None,
            avoid_for: None,
            intent,
            accepted_input,
            capabilities,
            resource_path,
            resource_digest: bytes.map(sha256_hex),
            files,
            props: Vec::new(),
            presets: BTreeMap::new(),
            file_slots: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ThemeMetadataOwned {
    pub description: String,
    pub mood: String,
    pub formality: String,
    pub density_fit: String,
    pub best_for: String,
    pub avoid_for: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RegistryEntry {
    pub kind: String,
    pub name: String,
    pub metadata: RegistryMetadata,
    pub provenance: Provenance,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RegistryDocument {
    pub schema_version: u32,
    pub entries: Vec<RegistryEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RegistrySourcesDocument {
    pub schema_version: u32,
    pub sources: Vec<RegistrySource>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RegistrySource {
    pub provenance: Provenance,
    pub activated: bool,
    pub note: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DefaultPackManifest {
    schema_version: u32,
    pack: String,
    themes: Vec<ThemeManifest>,
    scaffolds: Vec<ScaffoldManifest>,
    components: Vec<ComponentManifest>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ThemeManifest {
    name: String,
    css: String,
    metadata: ThemeMetadataOwned,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ScaffoldManifest {
    name: String,
    intent: Vec<String>,
    accepted_input: Vec<String>,
    capabilities: Vec<String>,
    files: Vec<ScaffoldFileManifest>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ScaffoldFileManifest {
    path: String,
    resource: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ComponentManifest {
    name: String,
    css: String,
    #[serde(default)]
    templates: Vec<String>,
    intent: Vec<String>,
    accepted_input: Vec<String>,
    capabilities: Vec<String>,
    #[serde(default)]
    props: Vec<PropertySchema>,
    #[serde(default)]
    presets: BTreeMap<String, BTreeMap<String, PropertyValue>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThemeResource {
    pub name: String,
    pub css: &'static str,
    pub css_resource: String,
    pub metadata: ThemeMetadataOwned,
    pub provenance: Provenance,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScaffoldFile {
    pub path: String,
    pub bytes: Vec<u8>,
}

static DEFAULT_PACK_MANIFEST: &str = include_str!("bundled/default-pack.toml");

pub fn bundled_theme_css(name: &str) -> Option<&'static str> {
    match name {
        "signal" => Some(include_str!("themes/signal.css")),
        "ledger" => Some(include_str!("themes/ledger.css")),
        "terminal" => Some(include_str!("themes/terminal.css")),
        "poster" => Some(include_str!("themes/poster.css")),
        _ => None,
    }
}

fn bundled_resource_bytes(path: &str) -> Option<&'static [u8]> {
    match path {
        "themes/signal.css" => Some(include_bytes!("themes/signal.css")),
        "themes/ledger.css" => Some(include_bytes!("themes/ledger.css")),
        "themes/terminal.css" => Some(include_bytes!("themes/terminal.css")),
        "themes/poster.css" => Some(include_bytes!("themes/poster.css")),
        "components/plan.css" => Some(include_bytes!("components/plan.css")),
        "bundled/components/literal-card.html" => {
            Some(include_bytes!("bundled/components/literal-card.html"))
        }
        "bundled/components/plan-record-card.html" => {
            Some(include_bytes!("bundled/components/plan-record-card.html"))
        }
        "bundled/components/semantic-card.html" => {
            Some(include_bytes!("bundled/components/semantic-card.html"))
        }
        "bundled/components/semantic-list.html" => {
            Some(include_bytes!("bundled/components/semantic-list.html"))
        }
        "bundled/components/semantic-table.html" => {
            Some(include_bytes!("bundled/components/semantic-table.html"))
        }
        "bundled/components/semantic-lanes.html" => {
            Some(include_bytes!("bundled/components/semantic-lanes.html"))
        }
        "bundled/scaffolds/deck/deck.toml" => {
            Some(include_bytes!("bundled/scaffolds/deck/deck.toml"))
        }
        "bundled/scaffolds/deck/slides/01-title.html" => Some(include_bytes!(
            "bundled/scaffolds/deck/slides/01-title.html"
        )),
        "bundled/scaffolds/deck/slides/02-content.md" => Some(include_bytes!(
            "bundled/scaffolds/deck/slides/02-content.md"
        )),
        "bundled/scaffolds/deck/assets/.gitkeep" => Some(b""),
        _ => None,
    }
}

pub fn bundled_template(path: &str) -> Option<&'static str> {
    bundled_resource_bytes(path).and_then(|bytes| std::str::from_utf8(bytes).ok())
}

pub fn bundled_themes() -> anyhow::Result<Vec<ThemeResource>> {
    let manifest = parse_default_pack()?;
    manifest
        .themes
        .into_iter()
        .map(|theme| {
            if theme.css != format!("themes/{}.css", theme.name) {
                bail!("bundled theme '{}' css path is not canonical", theme.name);
            }
            let css = bundled_theme_css(&theme.name).with_context(|| {
                format!("bundled theme '{}' references missing css", theme.name)
            })?;
            let css_bytes = bundled_resource_bytes(&theme.css).with_context(|| {
                format!(
                    "bundled theme '{}' references missing css resource",
                    theme.name
                )
            })?;
            if css.as_bytes() != css_bytes {
                bail!(
                    "bundled theme '{}' css string/resource mismatch",
                    theme.name
                );
            }
            Ok(ThemeResource {
                name: theme.name,
                css,
                css_resource: theme.css,
                metadata: theme.metadata,
                provenance: Provenance::bundled(),
            })
        })
        .collect()
}

pub fn registry_document() -> anyhow::Result<RegistryDocument> {
    let mut entries = Vec::new();
    for theme in bundled_themes()? {
        entries.push(RegistryEntry {
            kind: "theme".to_owned(),
            name: theme.name.clone(),
            metadata: RegistryMetadata::theme(
                theme.metadata,
                theme.css_resource,
                theme.css.as_bytes(),
            ),
            provenance: theme.provenance,
        });
    }
    let manifest = parse_default_pack()?;
    for scaffold in manifest.scaffolds {
        let files = scaffold_file_metadata(&scaffold)?;
        entries.push(RegistryEntry {
            kind: "scaffold".to_owned(),
            name: scaffold.name,
            metadata: RegistryMetadata::resource(
                ordered(scaffold.intent),
                ordered(scaffold.accepted_input),
                ordered(scaffold.capabilities),
                None,
                None,
                files,
            ),
            provenance: Provenance::bundled(),
        });
    }
    for component in manifest.components {
        let bytes = bundled_resource_bytes(&component.css).with_context(|| {
            format!(
                "bundled component '{}' references missing css",
                component.name
            )
        })?;
        let mut files = vec![ScaffoldFileMetadata {
            path: component.css.clone(),
            resource: component.css.clone(),
            digest: sha256_hex(bytes),
        }];
        for template in &component.templates {
            let template_bytes = bundled_resource_bytes(template).with_context(|| {
                format!(
                    "bundled component '{}' references missing template",
                    component.name
                )
            })?;
            files.push(ScaffoldFileMetadata {
                path: template.clone(),
                resource: template.clone(),
                digest: sha256_hex(template_bytes),
            });
        }
        let mut metadata = RegistryMetadata::resource(
            ordered(component.intent),
            ordered(component.accepted_input),
            ordered(component.capabilities),
            Some(component.css),
            Some(bytes),
            files,
        );
        metadata.props = component.props;
        metadata.presets = component.presets;
        entries.push(RegistryEntry {
            kind: "component".to_owned(),
            name: component.name,
            metadata,
            provenance: Provenance::bundled(),
        });
    }
    let entries = validate_entries(&entries)?;
    Ok(RegistryDocument {
        schema_version: REGISTRY_SCHEMA_VERSION,
        entries,
    })
}

pub fn explain(kind: &str, name: &str) -> anyhow::Result<RegistryEntry> {
    registry_document()?
        .entries
        .into_iter()
        .find(|entry| entry.kind == kind && entry.name == name)
        .ok_or_else(|| anyhow::anyhow!("unknown registry entry '{kind}/{name}'"))
}

pub fn sources_document() -> RegistrySourcesDocument {
    RegistrySourcesDocument {
        schema_version: REGISTRY_SCHEMA_VERSION,
        sources: vec![RegistrySource {
            provenance: Provenance::bundled(),
            activated: true,
            note: "Phase 1 uses only bundled defaults; user-global and project pack layering are not active.".to_owned(),
        }],
    }
}

pub fn validate_entries(entries: &[RegistryEntry]) -> anyhow::Result<Vec<RegistryEntry>> {
    let mut seen = BTreeSet::new();
    let mut resolved = entries.to_vec();
    resolved.sort_by(|a, b| {
        a.kind
            .cmp(&b.kind)
            .then(a.name.cmp(&b.name))
            .then(a.provenance.source.cmp(&b.provenance.source))
            .then(a.provenance.pack.cmp(&b.provenance.pack))
    });
    for entry in &resolved {
        reject_reserved(&entry.kind, &entry.name)?;
        if entry.kind == "component" && entry.metadata.capabilities.iter().any(|c| c == "js") {
            bail!(
                "component registry entry '{}/{}' declares forbidden js capability",
                entry.kind,
                entry.name
            );
        }
        if !seen.insert((entry.kind.clone(), entry.name.clone())) {
            bail!("duplicate registry entry '{}/{}'", entry.kind, entry.name);
        }
    }
    Ok(resolved)
}

pub fn reject_reserved(kind: &str, name: &str) -> anyhow::Result<()> {
    let reserved: BTreeMap<&str, &[&str]> = BTreeMap::from([
        (
            "runtime",
            &["stage", "navigation", "audit", "review"] as &[_],
        ),
        ("stage", &["*"] as &[_]),
        ("navigation", &["*"] as &[_]),
        ("audit", &["*"] as &[_]),
        ("review", &["*"] as &[_]),
    ]);
    if reserved
        .get(kind)
        .is_some_and(|names| names.contains(&"*") || names.contains(&name))
    {
        bail!("'{kind}/{name}' is a fixed compiler runtime resource and cannot be configured");
    }
    Ok(())
}

fn parse_default_pack() -> anyhow::Result<DefaultPackManifest> {
    let manifest: DefaultPackManifest = toml::from_str(DEFAULT_PACK_MANIFEST)
        .context("bundled default pack manifest is invalid")?;
    if manifest.schema_version != DEFAULT_PACK_SCHEMA_VERSION
        || manifest.pack != BUNDLED_DEFAULT_PACK
    {
        bail!("bundled default pack manifest identity/version mismatch");
    }
    for scaffold in &manifest.scaffolds {
        if scaffold.files.is_empty() {
            bail!("bundled scaffold '{}' has no files", scaffold.name);
        }
        scaffold_file_metadata(scaffold)?;
    }
    for component in &manifest.components {
        let capabilities = ordered(component.capabilities.clone());
        if capabilities.iter().any(|capability| capability == "js") {
            bail!(
                "bundled component '{}' declares forbidden js capability",
                component.name
            );
        }
        if capabilities != component.capabilities
            || ordered(component.intent.clone()) != component.intent
            || ordered(component.accepted_input.clone()) != component.accepted_input
        {
            bail!(
                "bundled component '{}' metadata must be deterministic sorted unique",
                component.name
            );
        }
        let css = bundled_resource_bytes(&component.css).with_context(|| {
            format!(
                "bundled component '{}' references missing css",
                component.name
            )
        })?;
        for template in &component.templates {
            let template_bytes = bundled_resource_bytes(template).with_context(|| {
                format!(
                    "bundled component '{}' references missing template",
                    component.name
                )
            })?;
            if std::str::from_utf8(template_bytes)?.contains("<script") {
                bail!(
                    "bundled component '{}' template contains script",
                    component.name
                );
            }
        }
        if std::str::from_utf8(css)?
            .to_ascii_lowercase()
            .contains("javascript")
        {
            bail!(
                "bundled component '{}' css must be javascript-free",
                component.name
            );
        }
    }
    Ok(manifest)
}

fn scaffold_file_metadata(
    scaffold: &ScaffoldManifest,
) -> anyhow::Result<Vec<ScaffoldFileMetadata>> {
    let mut files = Vec::new();
    for file in &scaffold.files {
        let bytes = if file.resource == "theme-css" {
            b"" as &[u8]
        } else {
            bundled_resource_bytes(&file.resource).with_context(|| {
                format!(
                    "bundled scaffold '{}' references missing resource {}",
                    scaffold.name, file.resource
                )
            })?
        };
        files.push(ScaffoldFileMetadata {
            path: file.path.clone(),
            resource: file.resource.clone(),
            digest: sha256_hex(bytes),
        });
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(files)
}

pub fn scaffold_files(name: &str, theme: &str, title: &str) -> anyhow::Result<Vec<ScaffoldFile>> {
    let manifest = parse_default_pack()?;
    let scaffold = manifest
        .scaffolds
        .into_iter()
        .find(|scaffold| scaffold.name == name)
        .ok_or_else(|| anyhow::anyhow!("unknown bundled scaffold '{name}'"))?;
    let theme_css = bundled_theme_css(theme)
        .ok_or_else(|| anyhow::anyhow!("unknown bundled theme '{theme}'"))?;
    scaffold
        .files
        .into_iter()
        .map(|file| {
            let bytes = if file.resource == "theme-css" {
                theme_css.as_bytes().to_vec()
            } else {
                let template = bundled_resource_bytes(&file.resource).with_context(|| {
                    format!(
                        "bundled scaffold '{name}' references missing resource {}",
                        file.resource
                    )
                })?;
                render_scaffold_template(template, theme, title)?
            };
            Ok(ScaffoldFile {
                path: file.path,
                bytes,
            })
        })
        .collect()
}

fn render_scaffold_template(template: &[u8], theme: &str, title: &str) -> anyhow::Result<Vec<u8>> {
    let rendered = std::str::from_utf8(template)?
        .replace("{{theme}}", theme)
        .replace("{{title}}", title);
    Ok(rendered.into_bytes())
}

fn ordered(values: Vec<String>) -> Vec<String> {
    values
        .into_iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_registry_is_deterministic_and_has_provenance() {
        let doc = registry_document().unwrap();
        assert_eq!(doc.schema_version, 1);
        let keys = doc
            .entries
            .iter()
            .map(|e| format!("{}/{}", e.kind, e.name))
            .collect::<Vec<_>>();
        assert_eq!(
            keys,
            vec![
                "component/compare-options",
                "component/decision-record",
                "component/file-impact-outcomes",
                "component/literal-card",
                "component/plan-primitives",
                "component/plan-record-card",
                "component/risk-register",
                "component/show-dependencies",
                "component/verification-evidence",
                "component/workstream-lanes",
                "scaffold/deck",
                "theme/ledger",
                "theme/poster",
                "theme/signal",
                "theme/terminal"
            ]
        );
        assert!(doc.entries.iter().all(|e| e.provenance.source == "bundled"));
        let component = explain("component", "plan-primitives").unwrap();
        assert!(
            component
                .metadata
                .capabilities
                .contains(&"js-free".to_owned())
        );
        assert!(!component.metadata.capabilities.contains(&"js".to_owned()));
        assert_eq!(
            component.metadata.resource_path.as_deref(),
            Some("components/plan.css")
        );
    }

    #[test]
    fn unknown_duplicates_and_reserved_entries_fail_clearly() {
        assert!(
            explain("theme", "missing")
                .unwrap_err()
                .to_string()
                .contains("unknown registry entry")
        );
        assert!(
            reject_reserved("runtime", "review")
                .unwrap_err()
                .to_string()
                .contains("fixed compiler runtime")
        );
        let entry = RegistryEntry {
            kind: "theme".into(),
            name: "signal".into(),
            metadata: RegistryMetadata::resource(
                Vec::new(),
                Vec::new(),
                Vec::new(),
                None,
                None,
                Vec::new(),
            ),
            provenance: Provenance::bundled(),
        };
        assert!(
            validate_entries(&[entry.clone(), entry])
                .unwrap_err()
                .to_string()
                .contains("duplicate")
        );
    }
}
