use anyhow::{Context, bail};
use base64::Engine;
use lol_html::{RewriteStrSettings, text};
use regex::Regex;
use serde::Deserialize;
use skera::{DEFAULT_DROP_TABLES, Plan, SubsetFlags, subset_font};
use std::{
    collections::BTreeSet,
    fmt, fs,
    io::Read,
    path::{Path, PathBuf},
    sync::LazyLock,
};
use write_fonts::{
    read::{FontRef, TableProvider, collections::IntSet, tables::glyf::Glyph},
    types::{GlyphId, NameId, Tag},
};

use crate::{normalize_asset_ref, validate_asset_path};

static CSS_CONTENT_START_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\bcontent\s*:\s*").unwrap());
static CSS_STRING_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"\"(?P<dq>(?:\\.|[^\"\\])*)\"|'(?P<sq>(?:\\.|[^'\\])*)'"#).unwrap()
});
static TAILWIND_CONTENT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?:before|after):content-\[(?:\"(?P<dq>[^\"]*)\"|'(?P<sq>[^']*)')\]"#).unwrap()
});
static HTML_ENTITIES: LazyLock<Vec<(&'static str, &'static str)>> = LazyLock::new(|| {
    let mut entries = entities::ENTITIES
        .iter()
        .map(|entity| (entity.entity, entity.characters))
        .collect::<Vec<_>>();
    entries.sort_by_key(|(name, _)| std::cmp::Reverse(name.len()));
    entries
});

const REQUIRED_TABLES: &[(&str, &[u8; 4])] = &[
    ("cmap", b"cmap"),
    ("glyf", b"glyf"),
    ("head", b"head"),
    ("hhea", b"hhea"),
    ("loca", b"loca"),
    ("hmtx", b"hmtx"),
    ("maxp", b"maxp"),
    ("name", b"name"),
    ("OS/2", b"OS/2"),
    ("post", b"post"),
];
const UNSUPPORTED_POSITIONING_TABLES: &[(&str, &[u8; 4])] = &[
    ("kern", b"kern"),
    ("kerx", b"kerx"),
    ("morx", b"morx"),
    ("mort", b"mort"),
];
const MAX_FONT_FACES: usize = 16;
const MAX_FONT_SOURCE_BYTES: u64 = 8 * 1024 * 1024;
const MAX_TOTAL_FONT_SOURCE_BYTES: u64 = 32 * 1024 * 1024;
const MAX_SUBSET_FONT_BYTES: usize = 8 * 1024 * 1024;
type LicenseRecord = (u16, u16, u16, u16, Vec<u8>);

#[derive(Debug, Deserialize, PartialEq, Eq, Clone)]
pub struct FontFaceConfig {
    pub source: String,
    pub family: String,
    pub style: FontStyle,
    pub weight: u16,
}

#[derive(Debug, Deserialize, PartialEq, Eq, PartialOrd, Ord, Clone, Copy)]
#[serde(rename_all = "lowercase")]
pub enum FontStyle {
    Normal,
    Italic,
    Oblique,
}

impl fmt::Display for FontStyle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Normal => "normal",
            Self::Italic => "italic",
            Self::Oblique => "oblique",
        })
    }
}

#[derive(Debug)]
pub(crate) struct EmbeddedFont {
    pub(crate) source: String,
    pub(crate) ttf: Vec<u8>,
    family: String,
    style: FontStyle,
    weight: u16,
}

impl EmbeddedFont {
    pub(crate) fn projected_inline_size(&self) -> u64 {
        crate::projected_data_uri_size(self.ttf.len() as u64, "font/ttf")
    }

    pub(crate) fn css(&self) -> String {
        format!(
            "@font-face{{font-family:{};font-style:{};font-weight:{};font-display:swap;src:url(data:font/ttf;base64,{}) format(\"truetype\")}}\n",
            css_string(&self.family),
            self.style,
            self.weight,
            base64::engine::general_purpose::STANDARD.encode(&self.ttf),
        )
    }
}

pub(crate) fn source_refs(faces: &[FontFaceConfig]) -> BTreeSet<String> {
    faces
        .iter()
        .filter_map(|face| normalize_asset_ref(&face.source).ok())
        .collect()
}

pub(crate) fn glyph_corpus(
    title: &str,
    rendered_html: &str,
    theme_css: &str,
) -> anyhow::Result<BTreeSet<u32>> {
    let mut corpus = title.chars().map(u32::from).collect::<BTreeSet<_>>();
    let mut text_node = String::new();
    lol_html::rewrite_str(
        rendered_html,
        RewriteStrSettings {
            element_content_handlers: vec![text!("*", |chunk| {
                text_node.push_str(chunk.as_str());
                if chunk.last_in_text_node() {
                    extend_html_text(&mut corpus, &text_node);
                    text_node.clear();
                }
                Ok(())
            })],
            ..RewriteStrSettings::default()
        },
    )
    .map_err(|e| anyhow::anyhow!(e.to_string()))?;

    // Literal generated content is rendered text too. Dynamic attr(), counter(),
    // custom-property, and runtime-generated strings are intentionally not guessed.
    for declaration in css_content_values(theme_css) {
        for captures in CSS_STRING_RE.captures_iter(declaration) {
            extend_css_capture(&mut corpus, &captures);
        }
    }
    for captures in TAILWIND_CONTENT_RE.captures_iter(rendered_html) {
        extend_css_capture(&mut corpus, &captures);
    }
    // Browser-generated list markers and CSS case transforms are common in
    // themes/utilities. This small closure is conservative without attempting
    // to reproduce the cascade for each element.
    corpus.extend("•0123456789. İıςΣǱǲǳ\u{307}".chars().map(u32::from));
    let authored = corpus
        .iter()
        .filter_map(|cp| char::from_u32(*cp))
        .collect::<Vec<_>>();
    for ch in authored {
        corpus.extend(ch.to_lowercase().map(u32::from));
        corpus.extend(ch.to_uppercase().map(u32::from));
    }
    Ok(corpus)
}

fn css_content_values(css: &str) -> Vec<&str> {
    CSS_CONTENT_START_RE
        .find_iter(css)
        .map(|start| {
            let value = &css[start.end()..];
            let mut quote = None;
            let mut escaped = false;
            let mut end = value.len();
            for (offset, character) in value.char_indices() {
                if escaped {
                    escaped = false;
                    continue;
                }
                if character == '\\' {
                    escaped = true;
                    continue;
                }
                if let Some(open) = quote {
                    if character == open {
                        quote = None;
                    }
                } else if matches!(character, '\'' | '"') {
                    quote = Some(character);
                } else if matches!(character, ';' | '}') {
                    end = offset;
                    break;
                }
            }
            &value[..end]
        })
        .collect()
}

fn extend_css_capture(corpus: &mut BTreeSet<u32>, captures: &regex::Captures<'_>) {
    let value = captures
        .name("dq")
        .or_else(|| captures.name("sq"))
        .map(|m| m.as_str())
        .unwrap_or_default();
    corpus.extend(unescape_css_string(value).chars().map(u32::from));
}

fn extend_html_text(corpus: &mut BTreeSet<u32>, text: &str) {
    corpus.extend(text.chars().map(u32::from));
    for (offset, _) in text.match_indices('&') {
        let input = &text[offset..];
        if let Some(decoded) = decode_numeric_reference(input) {
            corpus.insert(u32::from(decoded));
            continue;
        }
        if let Some((_, characters)) = HTML_ENTITIES
            .iter()
            .find(|(entity, _)| input.starts_with(entity))
        {
            corpus.extend(characters.chars().map(u32::from));
        }
    }
}

fn decode_numeric_reference(input: &str) -> Option<char> {
    let numeric = input.strip_prefix("&#")?;
    let (radix, digits) = if let Some(hex) = numeric
        .strip_prefix('x')
        .or_else(|| numeric.strip_prefix('X'))
    {
        (16, hex)
    } else {
        (10, numeric)
    };
    let length = digits
        .bytes()
        .take_while(|byte| match radix {
            16 => byte.is_ascii_hexdigit(),
            _ => byte.is_ascii_digit(),
        })
        .count();
    if length == 0 {
        return None;
    }
    let value = u32::from_str_radix(&digits[..length], radix).ok()?;
    char::from_u32(html_numeric_replacement(value)).or(Some('\u{fffd}'))
}

fn html_numeric_replacement(value: u32) -> u32 {
    match value {
        0 | 0xD800..=0xDFFF | 0x110000.. => 0xFFFD,
        0x80 => 0x20AC,
        0x82 => 0x201A,
        0x83 => 0x0192,
        0x84 => 0x201E,
        0x85 => 0x2026,
        0x86 => 0x2020,
        0x87 => 0x2021,
        0x88 => 0x02C6,
        0x89 => 0x2030,
        0x8A => 0x0160,
        0x8B => 0x2039,
        0x8C => 0x0152,
        0x8E => 0x017D,
        0x91 => 0x2018,
        0x92 => 0x2019,
        0x93 => 0x201C,
        0x94 => 0x201D,
        0x95 => 0x2022,
        0x96 => 0x2013,
        0x97 => 0x2014,
        0x98 => 0x02DC,
        0x99 => 0x2122,
        0x9A => 0x0161,
        0x9B => 0x203A,
        0x9C => 0x0153,
        0x9E => 0x017E,
        0x9F => 0x0178,
        _ => value,
    }
}

pub(crate) fn prepare_fonts(
    deck_dir: &Path,
    faces: &[FontFaceConfig],
    corpus: &BTreeSet<u32>,
) -> anyhow::Result<Vec<EmbeddedFont>> {
    validate_declarations(faces)?;
    preflight_sources(deck_dir, faces)?;
    let mut embedded = Vec::new();
    for face in faces {
        if let Some(font) = prepare_font(deck_dir, face, corpus)? {
            embedded.push(font);
        }
    }
    Ok(embedded)
}

fn validate_declarations(faces: &[FontFaceConfig]) -> anyhow::Result<()> {
    if faces.len() > MAX_FONT_FACES {
        bail!(
            "too many font faces: {} declared, maximum is {MAX_FONT_FACES}",
            faces.len()
        );
    }
    let mut metadata = BTreeSet::new();
    for face in faces {
        if face.family.trim().is_empty() {
            bail!("font family must not be empty");
        }
        if face
            .family
            .chars()
            .any(|c| c.is_control() || matches!(c, '<' | '>'))
        {
            bail!(
                "font family {:?} contains unsafe CSS/HTML characters",
                face.family
            );
        }
        if !(1..=1000).contains(&face.weight) {
            bail!(
                "font weight {} is outside the CSS range 1..=1000",
                face.weight
            );
        }
        let key = (face.family.clone(), face.style, face.weight);
        if !metadata.insert(key) {
            bail!(
                "duplicate font face for family {:?}, style {}, weight {}",
                face.family,
                face.style,
                face.weight
            );
        }
        let normalized = normalize_asset_ref(&face.source)
            .with_context(|| format!("invalid font source {}", face.source))?;
        if !normalized.to_ascii_lowercase().ends_with(".ttf") {
            bail!(
                "unsupported font source {normalized}: only individual TrueType .ttf fonts with glyf outlines are supported"
            );
        }
    }
    Ok(())
}

fn preflight_sources(deck_dir: &Path, faces: &[FontFaceConfig]) -> anyhow::Result<()> {
    let mut sources = BTreeSet::new();
    let mut total = 0u64;
    for face in faces {
        let source = normalize_asset_ref(&face.source)?;
        if !sources.insert(source.clone()) {
            continue;
        }
        let path = validate_asset_path(deck_dir, &source)
            .with_context(|| format!("font source {source}"))?;
        let size = fs::metadata(&path)
            .with_context(|| format!("cannot inspect font source {source}"))?
            .len();
        if size > MAX_FONT_SOURCE_BYTES {
            bail!(
                "font source {source} is {size} bytes; maximum supported source size is {MAX_FONT_SOURCE_BYTES} bytes"
            );
        }
        total = total
            .checked_add(size)
            .ok_or_else(|| anyhow::anyhow!("total font source size overflow"))?;
        if total > MAX_TOTAL_FONT_SOURCE_BYTES {
            bail!(
                "total font source size is {total} bytes; maximum supported total is {MAX_TOTAL_FONT_SOURCE_BYTES} bytes"
            );
        }
    }
    Ok(())
}

fn read_bounded_font(path: &Path, source: &str) -> anyhow::Result<Vec<u8>> {
    let file = fs::File::open(path).with_context(|| format!("cannot read font source {source}"))?;
    let mut bytes = Vec::new();
    file.take(MAX_FONT_SOURCE_BYTES + 1)
        .read_to_end(&mut bytes)
        .with_context(|| format!("cannot read font source {source}"))?;
    if bytes.len() as u64 > MAX_FONT_SOURCE_BYTES {
        bail!(
            "font source {source} exceeds the maximum supported source size of {MAX_FONT_SOURCE_BYTES} bytes"
        );
    }
    Ok(bytes)
}

fn prepare_font(
    deck_dir: &Path,
    config: &FontFaceConfig,
    corpus: &BTreeSet<u32>,
) -> anyhow::Result<Option<EmbeddedFont>> {
    let source = normalize_asset_ref(&config.source)?;
    let path: PathBuf =
        validate_asset_path(deck_dir, &source).with_context(|| format!("font source {source}"))?;
    let bytes = read_bounded_font(&path, &source)?;
    if !bytes.starts_with(&[0x00, 0x01, 0x00, 0x00]) {
        bail!(
            "unsupported font source {source}: expected an individual TrueType .ttf font with glyf outlines (WOFF, WOFF2, OpenType/CFF, and collections are not supported)"
        );
    }

    let font = FontRef::new(&bytes)
        .map_err(|e| anyhow::anyhow!("malformed TrueType font source {source}: {e}"))?;
    validate_source_font(&source, &font)?;
    let source_license_records = licensing_records(&font)?;
    let best_cmap = font
        .cmap()?
        .best_subtable()
        .map(|(_, _, subtable)| subtable);
    if !corpus.iter().any(|cp| {
        best_cmap
            .as_ref()
            .and_then(|cmap| cmap.map_codepoint(*cp))
            .is_some()
    }) {
        return Ok(None);
    }

    let input_gids = IntSet::<GlyphId>::empty();
    let input_unicodes = corpus.iter().copied().collect::<IntSet<u32>>();
    let drop_tables = DEFAULT_DROP_TABLES.iter().copied().collect::<IntSet<Tag>>();
    let layout_scripts = IntSet::<Tag>::all();
    let layout_features = IntSet::<Tag>::all();
    // Keep and verify all copyright, trademark, author, license, and license URL
    // records, including legacy platform records.
    let name_ids = IntSet::<NameId>::all();
    let name_languages = IntSet::<u16>::all();
    let plan = Plan::new(
        &input_gids,
        &input_unicodes,
        &font,
        SubsetFlags::default(),
        &drop_tables,
        &layout_scripts,
        &layout_features,
        &name_ids,
        &name_languages,
    );
    let subset = subset_font(&font, &plan)
        .map_err(|e| anyhow::anyhow!("could not subset font source {source}: {e}"))?;
    if subset.len() > MAX_SUBSET_FONT_BYTES {
        bail!(
            "subset font for {source} is {} bytes; maximum supported subset size is {MAX_SUBSET_FONT_BYTES} bytes",
            subset.len()
        );
    }
    let subset_font = FontRef::new(&subset)
        .map_err(|_| anyhow::anyhow!("subsetter produced a malformed font for {source}"))?;
    validate_true_type_tables(&source, &subset_font)?;
    if licensing_records(&subset_font)? != source_license_records {
        bail!("subsetting {source} did not preserve all licensing name records");
    }

    Ok(Some(EmbeddedFont {
        source,
        ttf: subset,
        family: config.family.clone(),
        style: config.style,
        weight: config.weight,
    }))
}

fn validate_source_font(source: &str, font: &FontRef<'_>) -> anyhow::Result<()> {
    validate_true_type_tables(source, font)?;
    for (name, tag) in UNSUPPORTED_POSITIONING_TABLES {
        if font.table_data(Tag::new(tag)).is_some() {
            bail!(
                "font source {source} contains unsupported {name} shaping/positioning data, which cannot be subset safely"
            );
        }
    }
    let name = font
        .name()
        .map_err(|e| anyhow::anyhow!("font source {source} has a malformed name table: {e}"))?;
    if name.version() == 1 {
        bail!(
            "font source {source} uses name table format 1 language tags, which cannot be preserved by the supported subsetter"
        );
    }
    if name.name_record().iter().any(|record| {
        matches!(
            record.name_id().to_u16(),
            0 | 7 | 8 | 9 | 10 | 11 | 12 | 13 | 14
        ) && !record.is_unicode()
    }) {
        bail!(
            "font source {source} has legacy-platform licensing name records that cannot be preserved by the supported subsetter"
        );
    }
    let os2 = font
        .os2()
        .map_err(|e| anyhow::anyhow!("font source {source} has a malformed OS/2 table: {e}"))?;
    let flags = os2.fs_type();
    let permission = flags & 0x000F;
    let valid_permission = if os2.version() <= 2 {
        true
    } else {
        matches!(permission, 0 | 2 | 4 | 8)
    };
    if !valid_permission {
        bail!("font source {source} has malformed OS/2 embedding permissions");
    }
    let restricted = if os2.version() <= 2 {
        permission != 0 && permission & (4 | 8) == 0
    } else {
        permission == 2
    };
    if restricted {
        bail!("font source {source} has restricted OS/2 embedding permissions");
    }
    if os2.version() >= 2 && flags & 0x0100 != 0 {
        bail!("font source {source} forbids subsetting in its OS/2 embedding flags");
    }
    if os2.version() >= 2 && flags & 0x0200 != 0 {
        bail!("font source {source} forbids outline embedding in its OS/2 flags");
    }
    Ok(())
}

fn validate_true_type_tables(source: &str, font: &FontRef<'_>) -> anyhow::Result<()> {
    for (name, tag) in REQUIRED_TABLES {
        if font.table_data(Tag::new(tag)).is_none() {
            bail!("font source {source} is missing required TrueType table {name}");
        }
    }

    let malformed = |table: &str, error: write_fonts::read::ReadError| {
        anyhow::anyhow!("font source {source} has a malformed {table} table: {error}")
    };
    let head = font.head().map_err(|e| malformed("head", e))?;
    if !matches!(head.index_to_loc_format(), 0 | 1) {
        bail!("font source {source} has an invalid head indexToLocFormat");
    }
    let hhea = font.hhea().map_err(|e| malformed("hhea", e))?;
    let maxp = font.maxp().map_err(|e| malformed("maxp", e))?;
    let glyph_count = usize::from(maxp.num_glyphs());
    if glyph_count == 0 {
        bail!("font source {source} has no glyphs in its maxp table");
    }
    let cmap = font.cmap().map_err(|e| malformed("cmap", e))?;
    if cmap.best_subtable().is_none() {
        bail!("font source {source} has no usable cmap subtable");
    }
    font.name().map_err(|e| malformed("name", e))?;
    font.os2().map_err(|e| malformed("OS/2", e))?;
    font.post().map_err(|e| malformed("post", e))?;

    let glyf = font.glyf().map_err(|e| malformed("glyf", e))?;
    let loca = font.loca(None).map_err(|e| malformed("loca", e))?;
    if loca.len() != glyph_count {
        bail!(
            "font source {source} has {} loca entries for {glyph_count} glyphs",
            loca.len()
        );
    }
    if !loca.all_offsets_are_ascending() {
        bail!("font source {source} has non-ascending loca offsets");
    }
    let glyf_len = font
        .table_data(Tag::new(b"glyf"))
        .map(|data| data.len())
        .unwrap_or_default();
    if loca
        .get_raw(glyph_count)
        .is_none_or(|offset| offset as usize > glyf_len)
    {
        bail!("font source {source} has loca offsets outside its glyf table");
    }
    for gid in 0..glyph_count {
        let glyph = loca
            .get_glyf(GlyphId::new(gid as u32), &glyf)
            .map_err(|e| malformed("glyf/loca", e))?;
        if let Some(Glyph::Composite(composite)) = glyph {
            for component in composite.components() {
                if component.glyph.to_u32() as usize >= glyph_count {
                    bail!(
                        "font source {source} has a composite glyph referencing an out-of-range glyph"
                    );
                }
            }
        }
    }

    let metric_count = usize::from(hhea.number_of_h_metrics());
    if metric_count == 0 || metric_count > glyph_count {
        bail!(
            "font source {source} has invalid hhea numberOfHMetrics {metric_count} for {glyph_count} glyphs"
        );
    }
    let hmtx = font.hmtx().map_err(|e| malformed("hmtx", e))?;
    if hmtx.h_metrics().len() != metric_count
        || hmtx.left_side_bearings().len() != glyph_count - metric_count
    {
        bail!("font source {source} has inconsistent hhea/maxp/hmtx metrics");
    }
    for gid in 0..glyph_count {
        let gid = GlyphId::new(gid as u32);
        if hmtx.advance(gid).is_none() || hmtx.side_bearing(gid).is_none() {
            bail!("font source {source} has missing horizontal metrics for a glyph");
        }
    }
    Ok(())
}

fn licensing_records(font: &FontRef<'_>) -> anyhow::Result<Vec<LicenseRecord>> {
    let name = font.name()?;
    let string_data = name.string_data();
    let mut records = name
        .name_record()
        .iter()
        .filter(|record| {
            matches!(
                record.name_id().to_u16(),
                0 | 7 | 8 | 9 | 10 | 11 | 12 | 13 | 14
            )
        })
        .map(|record| {
            Ok((
                record.platform_id(),
                record.encoding_id(),
                record.name_id().to_u16(),
                record.language_id(),
                record.string(string_data)?.to_string().into_bytes(),
            ))
        })
        .collect::<Result<Vec<_>, write_fonts::read::ReadError>>()?;
    records.sort();
    Ok(records)
}

#[cfg(test)]
fn license_name_ids(font: &FontRef<'_>) -> anyhow::Result<BTreeSet<u16>> {
    Ok(licensing_records(font)?
        .into_iter()
        .map(|record| record.2)
        .collect())
}

fn css_string(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

fn unescape_css_string(value: &str) -> String {
    let mut out = String::new();
    let mut chars = value.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        let mut hex = String::new();
        while hex.len() < 6 && chars.peek().is_some_and(|c| c.is_ascii_hexdigit()) {
            hex.push(chars.next().unwrap());
        }
        if !hex.is_empty() {
            if chars.peek().is_some_and(|c| c.is_ascii_whitespace()) {
                chars.next();
            }
            if let Ok(value) = u32::from_str_radix(&hex, 16)
                && let Some(ch) = char::from_u32(value)
            {
                out.push(ch);
            }
        } else if let Some(escaped) = chars.next()
            && !matches!(escaped, '\n' | '\r')
        {
            out.push(escaped);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::Digest;

    const FIXTURE: &[u8] = include_bytes!("../tests/fixtures/fonts/Tiny5-Regular.ttf");

    fn fixture_deck() -> tempfile::TempDir {
        let deck = tempfile::tempdir().unwrap();
        fs::create_dir_all(deck.path().join("assets")).unwrap();
        fs::write(deck.path().join("assets/tiny5.ttf"), FIXTURE).unwrap();
        deck
    }

    fn fixture_face(source: &str, family: &str, style: FontStyle, weight: u16) -> FontFaceConfig {
        FontFaceConfig {
            source: source.into(),
            family: family.into(),
            style,
            weight,
        }
    }

    #[test]
    fn corpus_collects_unicode_combining_text_and_literal_css_content() {
        let corpus = glyph_corpus(
            "Café",
            "<p class=\"before:content-['★']\">naïve Ελληνικά e&#x301; &copy &#x80;</p>",
            r#".x::before { content: "\2192 ;☆" "B}✓◇"; }"#,
        )
        .unwrap();
        for ch in "CaféNAÏVEΕΛΛΗΝΙΚΆe\u{301}→✓★☆◇©€•İıςǲ\u{307}".chars()
        {
            assert!(corpus.contains(&u32::from(ch)), "missing {ch:?}");
        }
    }

    #[test]
    fn declarations_reject_unsafe_or_duplicate_metadata() {
        let face = FontFaceConfig {
            source: "assets/a.ttf".into(),
            family: "Example".into(),
            style: FontStyle::Normal,
            weight: 400,
        };
        assert!(validate_declarations(&[face.clone(), face]).is_err());
        let unsupported = FontFaceConfig {
            source: "assets/a.otf".into(),
            family: "Example".into(),
            style: FontStyle::Normal,
            weight: 400,
        };
        assert!(
            validate_declarations(&[unsupported])
                .unwrap_err()
                .to_string()
                .contains("only individual TrueType")
        );
    }

    #[test]
    fn subsets_deterministically_to_browser_true_type_without_mutating_source() {
        let deck = fixture_deck();
        let face = fixture_face("assets/tiny5.ttf", "Tiny Five", FontStyle::Normal, 400);
        let corpus = "Café Ελληνικά e\u{301}"
            .chars()
            .map(u32::from)
            .collect::<BTreeSet<_>>();

        let first = prepare_fonts(deck.path(), std::slice::from_ref(&face), &corpus).unwrap();
        let second = prepare_fonts(deck.path(), &[face], &corpus).unwrap();

        assert_eq!(first[0].ttf, second[0].ttf);
        assert!(first[0].ttf.starts_with(&[0x00, 0x01, 0x00, 0x00]));
        assert!(first[0].ttf.len() < FIXTURE.len());
        validate_true_type_tables("generated subset", &FontRef::new(&first[0].ttf).unwrap())
            .unwrap();
        assert_eq!(
            fs::read(deck.path().join("assets/tiny5.ttf")).unwrap(),
            FIXTURE
        );
        let source = FontRef::new(FIXTURE).unwrap();
        let license_ids = license_name_ids(&source).unwrap();
        assert!(license_ids.contains(&0));
        assert!(license_ids.contains(&13));
        assert!(license_ids.contains(&14));
    }

    #[test]
    fn emits_distinct_metadata_for_multiple_faces() {
        let deck = fixture_deck();
        let faces = [
            fixture_face("assets/tiny5.ttf", "Tiny Five", FontStyle::Normal, 400),
            fixture_face("assets/tiny5.ttf", "Tiny Five", FontStyle::Italic, 700),
        ];
        let corpus = "Hello".chars().map(u32::from).collect::<BTreeSet<_>>();
        let embedded = prepare_fonts(deck.path(), &faces, &corpus).unwrap();
        let css = embedded.iter().map(EmbeddedFont::css).collect::<String>();

        assert_eq!(css.matches("@font-face").count(), 2);
        assert!(css.contains("font-family:\"Tiny Five\""));
        assert!(css.contains("font-style:normal;font-weight:400"));
        assert!(css.contains("font-style:italic;font-weight:700"));
        assert_eq!(css.matches("data:font/ttf;base64,").count(), 2);
        assert_eq!(css.matches("format(\"truetype\")").count(), 2);
        assert!(!css.contains("assets/tiny5.ttf"));
    }

    #[test]
    fn rejects_missing_malformed_and_unsupported_font_sources() {
        let deck = fixture_deck();
        let corpus = BTreeSet::from([u32::from('A')]);
        let missing = fixture_face("assets/missing.ttf", "Missing", FontStyle::Normal, 400);
        assert!(
            prepare_fonts(deck.path(), &[missing], &corpus)
                .unwrap_err()
                .to_string()
                .contains("font source assets/missing.ttf")
        );

        fs::write(deck.path().join("assets/bad.ttf"), b"not a font").unwrap();
        let malformed = fixture_face("assets/bad.ttf", "Bad", FontStyle::Normal, 400);
        assert!(
            prepare_fonts(deck.path(), &[malformed], &corpus)
                .unwrap_err()
                .to_string()
                .contains("expected an individual TrueType")
        );

        fs::write(deck.path().join("assets/font.woff2"), b"wOF2").unwrap();
        let unsupported = fixture_face("assets/font.woff2", "Web", FontStyle::Normal, 400);
        assert!(
            prepare_fonts(deck.path(), &[unsupported], &corpus)
                .unwrap_err()
                .to_string()
                .contains("only individual TrueType")
        );

        let mut restricted = FIXTURE.to_vec();
        let face = FontRef::new(&restricted).unwrap();
        let os2 = face.table_data(Tag::new(b"OS/2")).unwrap();
        let os2_offset = os2.as_bytes().as_ptr() as usize - restricted.as_ptr() as usize;
        restricted[os2_offset + 8..os2_offset + 10].copy_from_slice(&2u16.to_be_bytes());
        fs::write(deck.path().join("assets/restricted.ttf"), restricted).unwrap();
        let restricted = fixture_face(
            "assets/restricted.ttf",
            "Restricted",
            FontStyle::Normal,
            400,
        );
        assert!(
            prepare_fonts(deck.path(), &[restricted], &corpus)
                .unwrap_err()
                .to_string()
                .contains("restricted OS/2 embedding")
        );
    }

    fn table_record_offset(bytes: &[u8], tag: &[u8; 4]) -> usize {
        let table_count = u16::from_be_bytes(bytes[4..6].try_into().unwrap()) as usize;
        (0..table_count)
            .map(|index| 12 + index * 16)
            .find(|offset| &bytes[*offset..*offset + 4] == tag)
            .unwrap()
    }

    fn rename_table(bytes: &[u8], from: &[u8; 4], to: &[u8; 4]) -> Vec<u8> {
        let mut changed = bytes.to_vec();
        let offset = table_record_offset(&changed, from);
        changed[offset..offset + 4].copy_from_slice(to);
        changed
    }

    fn truncate_table(bytes: &[u8], tag: &[u8; 4]) -> Vec<u8> {
        let mut changed = bytes.to_vec();
        let offset = table_record_offset(&changed, tag);
        changed[offset + 12..offset + 16].copy_from_slice(&1u32.to_be_bytes());
        changed
    }

    #[test]
    fn every_missing_or_truncated_mandatory_table_returns_an_error() {
        for (name, tag) in REQUIRED_TABLES {
            let missing = rename_table(FIXTURE, tag, b"ZZZZ");
            let font = FontRef::new(&missing).unwrap();
            let error = validate_source_font("missing.ttf", &font).unwrap_err();
            assert!(
                error
                    .to_string()
                    .contains("missing required TrueType table"),
                "missing {name} returned {error:#}"
            );

            let truncated = truncate_table(FIXTURE, tag);
            let font = FontRef::new(&truncated).unwrap();
            let error = validate_source_font("truncated.ttf", &font).unwrap_err();
            assert!(
                error.to_string().contains(name)
                    || error.to_string().contains("loca")
                    || error.to_string().contains("glyf")
                    || error.to_string().contains("metrics"),
                "truncated {name} returned {error:#}"
            );
        }
    }

    #[test]
    fn rejects_legacy_kerning_and_aat_tables_even_when_gpos_exists() {
        for tag in [b"kern", b"kerx", b"morx", b"mort"] {
            let changed = rename_table(FIXTURE, b"gasp", tag);
            let font = FontRef::new(&changed).unwrap();
            let error = validate_source_font("unsupported.ttf", &font).unwrap_err();
            assert!(
                error
                    .to_string()
                    .contains(std::str::from_utf8(tag).unwrap()),
                "{tag:?} returned {error:#}"
            );
        }
    }

    #[test]
    fn enforces_face_and_source_processing_limits_before_parsing() {
        let deck = fixture_deck();
        let corpus = BTreeSet::from([u32::from('A')]);
        let faces = (0..=MAX_FONT_FACES)
            .map(|index| {
                fixture_face(
                    "assets/tiny5.ttf",
                    &format!("Face {index}"),
                    FontStyle::Normal,
                    400,
                )
            })
            .collect::<Vec<_>>();
        assert!(
            prepare_fonts(deck.path(), &faces, &corpus)
                .unwrap_err()
                .to_string()
                .contains("too many font faces")
        );

        fs::write(
            deck.path().join("assets/oversized.ttf"),
            vec![0; MAX_FONT_SOURCE_BYTES as usize + 1],
        )
        .unwrap();
        let oversized = fixture_face("assets/oversized.ttf", "Oversized", FontStyle::Normal, 400);
        assert!(
            prepare_fonts(deck.path(), &[oversized], &corpus)
                .unwrap_err()
                .to_string()
                .contains("maximum supported source size")
        );

        let mut total_faces = Vec::new();
        for index in 0..5 {
            let source = format!("assets/total-{index}.ttf");
            fs::File::create(deck.path().join(&source))
                .unwrap()
                .set_len(MAX_FONT_SOURCE_BYTES)
                .unwrap();
            total_faces.push(fixture_face(
                &source,
                &format!("Total {index}"),
                FontStyle::Normal,
                400,
            ));
        }
        assert!(
            prepare_fonts(deck.path(), &total_faces, &corpus)
                .unwrap_err()
                .to_string()
                .contains("maximum supported total")
        );
    }

    #[test]
    fn fixture_matches_immutable_upstream_digest() {
        assert_eq!(
            format!("{:x}", sha2::Sha256::digest(FIXTURE)),
            "cb8168f80cfee2f47f6db59f2a7afbde31cdcdcdcf262e7a993e4d468a5bf4b0"
        );
    }

    #[test]
    fn skips_a_valid_face_with_no_glyphs_in_the_deck_corpus() {
        let deck = fixture_deck();
        let face = fixture_face("assets/tiny5.ttf", "Tiny Five", FontStyle::Normal, 400);
        let corpus = BTreeSet::from([u32::from('字')]);

        assert!(
            prepare_fonts(deck.path(), &[face], &corpus)
                .unwrap()
                .is_empty()
        );
    }
}
