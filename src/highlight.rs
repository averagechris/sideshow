use comrak::{adapters::SyntaxHighlighterAdapter, html::write_opening_tag};
use std::{collections::HashMap, io, io::Write};

pub struct Highlighter;

impl SyntaxHighlighterAdapter for Highlighter {
    fn write_highlighted(
        &self,
        output: &mut dyn Write,
        language: Option<&str>,
        code: &str,
    ) -> io::Result<()> {
        match language.and_then(lang) {
            Some(lang) => output.write_all(highlight_code(code, lang).as_bytes()),
            None => {
                let mut escaped = String::new();
                escape_into(&mut escaped, code);
                output.write_all(escaped.as_bytes())
            }
        }
    }

    fn write_pre_tag(
        &self,
        output: &mut dyn Write,
        attributes: HashMap<String, String>,
    ) -> io::Result<()> {
        write_opening_tag(output, "pre", attributes)
    }

    fn write_code_tag(
        &self,
        output: &mut dyn Write,
        attributes: HashMap<String, String>,
    ) -> io::Result<()> {
        write_opening_tag(output, "code", attributes)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Lang {
    Rust,
    Shell,
    Toml,
    Json,
    Python,
    Js,
}

fn lang(tag: &str) -> Option<Lang> {
    match tag.to_ascii_lowercase().as_str() {
        "rs" | "rust" => Some(Lang::Rust),
        "sh" | "bash" | "shell" => Some(Lang::Shell),
        "toml" => Some(Lang::Toml),
        "json" => Some(Lang::Json),
        "py" | "python" => Some(Lang::Python),
        "js" | "javascript" | "ts" | "typescript" => Some(Lang::Js),
        _ => None,
    }
}

fn highlight_code(code: &str, lang: Lang) -> String {
    let (line_comment, hash_comment, keywords): (&[&str], bool, &[&str]) = match lang {
        Lang::Rust => (
            &["//"],
            false,
            &[
                "as", "async", "await", "break", "const", "continue", "crate", "else", "enum",
                "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut",
                "pub", "ref", "return", "self", "Self", "static", "struct", "super", "trait",
                "type", "unsafe", "use", "where", "while",
            ],
        ),
        Lang::Shell => (
            &[],
            true,
            &[
                "case", "do", "done", "elif", "else", "esac", "fi", "for", "function", "if", "in",
                "then", "while", "export", "local",
            ],
        ),
        Lang::Toml => (&[], true, &["true", "false"]),
        Lang::Json => (&[], false, &["true", "false", "null"]),
        Lang::Python => (
            &[],
            true,
            &[
                "and", "as", "async", "await", "break", "class", "continue", "def", "elif", "else",
                "except", "False", "finally", "for", "from", "if", "import", "in", "is", "lambda",
                "None", "not", "or", "pass", "return", "True", "try", "while", "with", "yield",
            ],
        ),
        Lang::Js => (
            &["//"],
            false,
            &[
                "async", "await", "break", "case", "catch", "class", "const", "continue",
                "default", "else", "export", "extends", "finally", "for", "from", "function", "if",
                "import", "in", "let", "new", "return", "switch", "this", "throw", "try", "typeof",
                "var", "while", "null", "true", "false",
            ],
        ),
    };
    scan(
        code,
        line_comment,
        hash_comment,
        keywords,
        matches!(lang, Lang::Python | Lang::Rust | Lang::Js),
    )
}

fn scan(
    s: &str,
    line_comment: &[&str],
    hash_comment: bool,
    keywords: &[&str],
    functions: bool,
) -> String {
    let mut out = String::new();
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let rest = &s[i..];
        if line_comment.iter().any(|p| rest.starts_with(p))
            || (hash_comment && rest.starts_with('#'))
        {
            let end = rest.find('\n').map_or(s.len(), |n| i + n);
            span(&mut out, "tok-com", &s[i..end]);
            i = end;
        } else if rest.starts_with('"') || rest.starts_with('\'') || rest.starts_with('`') {
            let quote = bytes[i];
            let mut j = i + 1;
            let mut esc = false;
            while j < bytes.len() {
                let b = bytes[j];
                j += 1;
                if esc {
                    esc = false;
                } else if b == b'\\' {
                    esc = true;
                } else if b == quote {
                    break;
                }
            }
            span(&mut out, "tok-str", &s[i..j]);
            i = j;
        } else if bytes[i].is_ascii_digit() {
            let mut j = i + 1;
            while j < bytes.len()
                && (bytes[j].is_ascii_alphanumeric() || bytes[j] == b'.' || bytes[j] == b'_')
            {
                j += 1;
            }
            span(&mut out, "tok-num", &s[i..j]);
            i = j;
        } else if is_ident_start(bytes[i]) {
            let mut j = i + 1;
            while j < bytes.len() && is_ident(bytes[j]) {
                j += 1;
            }
            let word = &s[i..j];
            if keywords.contains(&word) {
                span(&mut out, "tok-kw", word);
            } else if functions && s[j..].trim_start().starts_with('(') {
                span(&mut out, "tok-fn", word);
            } else {
                escape_into(&mut out, word);
            }
            i = j;
        } else {
            escape_into(&mut out, &s[i..i + 1]);
            i += 1;
        }
    }
    out
}

fn is_ident_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_'
}
fn is_ident(b: u8) -> bool {
    is_ident_start(b) || b.is_ascii_digit()
}
fn span(out: &mut String, class: &str, text: &str) {
    out.push_str("<span class=\"");
    out.push_str(class);
    out.push_str("\">");
    escape_into(out, text);
    out.push_str("</span>");
}
fn escape_into(out: &mut String, text: &str) {
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rust_spans_and_escapes() {
        let h = highlight_code("fn main() { let s = \"</code>\"; // ok\n42 }", Lang::Rust);
        assert!(h.contains("tok-kw"));
        assert!(h.contains("tok-fn"));
        assert!(h.contains("tok-str"));
        assert!(h.contains("tok-com"));
        assert!(h.contains("tok-num"));
        assert!(h.contains("&lt;/code&gt;"));
    }
    #[test]
    fn shell_spans() {
        let h = highlight_code("if echo \"x\" # hi\nthen exit 1\nfi", Lang::Shell);
        assert!(h.contains("tok-kw"));
        assert!(h.contains("tok-str"));
        assert!(h.contains("tok-com"));
        assert!(h.contains("tok-num"));
    }
    #[test]
    fn toml_json_python_js_spans() {
        for (src, lang) in [
            ("enabled = true # c\nport = 8080", Lang::Toml),
            ("{\"n\": 1, \"ok\": true}", Lang::Json),
            ("def f():\n return '</code>' # c\n", Lang::Python),
            ("function f(){ return 1 // c\n}", Lang::Js),
        ] {
            let h = highlight_code(src, lang);
            assert!(h.contains("tok-"));
            assert!(!h.contains("</code>"));
        }
    }
    #[test]
    fn adapter_leaves_unknown_language_plain() {
        let mut out = Vec::new();
        Highlighter
            .write_highlighted(&mut out, Some("wat"), "let x = <1>\n")
            .unwrap();
        assert_eq!(String::from_utf8(out).unwrap(), "let x = &lt;1&gt;\n");
    }
    #[test]
    fn adapter_highlights_recognized_language() {
        let mut out = Vec::new();
        Highlighter
            .write_highlighted(&mut out, Some("rust"), "fn main() {}\n")
            .unwrap();
        let h = String::from_utf8(out).unwrap();
        assert!(h.contains("tok-kw"));
        assert!(h.contains("tok-fn"));
    }
}
