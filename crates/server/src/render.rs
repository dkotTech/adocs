use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd, html};
use serde::Serialize;

use crate::content::DocMeta;

/// Above this size text is not inlined, only served through /raw.
const MAX_INLINE: usize = 2 * 1024 * 1024;

/// How the frontend should display a document.
#[derive(Serialize)]
#[serde(tag = "kind", content = "body", rename_all = "lowercase")]
pub enum Render {
    /// Markdown rendered to HTML.
    Html(String),
    /// Plain source text: code, json, yaml.
    Text(String),
    /// An HTML document: a sandboxed iframe pointing at /raw.
    Frame,
    /// PDF: an iframe without sandbox, otherwise Chrome refuses to start its viewer.
    Pdf,
    /// An image.
    Image,
    /// Everything else, download only.
    Binary,
}

/// Whether the content is needed to prepare the display. Images, PDFs and large files
/// are shown through /raw, so there is no reason to read them into memory.
pub fn needs_data(meta: &DocMeta) -> bool {
    meta.size as usize <= MAX_INLINE
        && (is_markdown(&meta.content_type, &meta.path) || is_text(&meta.content_type))
}

pub fn render(meta: &DocMeta, data: Option<&[u8]>) -> Render {
    let ct = meta.content_type.as_str();
    let text = data.and_then(|d| std::str::from_utf8(d).ok());

    if is_markdown(ct, &meta.path) {
        return match text {
            Some(md) => Render::Html(markdown_to_html(md)),
            None => Render::Binary,
        };
    }
    if ct.starts_with("application/pdf") {
        return Render::Pdf;
    }
    if ct.starts_with("text/html") {
        return Render::Frame;
    }
    if ct.starts_with("image/") {
        return Render::Image;
    }
    match text {
        Some(text) if is_text(ct) => Render::Text(text.to_string()),
        _ => Render::Binary,
    }
}

pub fn is_markdown(content_type: &str, path: &str) -> bool {
    content_type.starts_with("text/markdown")
        || path.ends_with(".md")
        || path.ends_with(".markdown")
}

pub fn is_text(content_type: &str) -> bool {
    const TEXT_TYPES: &[&str] = &[
        "application/json",
        "application/xml",
        "application/javascript",
        "application/x-yaml",
        "application/yaml",
        "application/toml",
        "application/x-sh",
        "application/sql",
        "application/octet-stream",
    ];
    content_type.starts_with("text/")
        || TEXT_TYPES.iter().any(|t| content_type.starts_with(t))
        || content_type.ends_with("+json")
        || content_type.ends_with("+xml")
}

pub fn markdown_to_html(md: &str) -> String {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_FOOTNOTES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);
    options.insert(Options::ENABLE_HEADING_ATTRIBUTES);
    let parser = Parser::new_ext(md, options);
    let mut out = String::with_capacity(md.len() * 3 / 2);
    html::push_html(&mut out, parser);
    out
}

/// Escapes text for HTML and XML, attribute values included.
pub fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// The first level-one heading in markdown.
pub fn markdown_title(md: &str) -> Option<String> {
    md.lines()
        .map(str::trim)
        .find_map(|line| line.strip_prefix("# "))
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
}

/// Characters kept from the first paragraph for a summary.
const SUMMARY_CHARS: usize = 200;
/// Shorter paragraphs are skipped: a lone "See also" or "Back to ..." does not describe a document.
const MIN_SUMMARY_CHARS: usize = 30;

/// The first descriptive paragraph after the level-one heading, or before any heading when there is
/// none, flattened to plain text and cut to `SUMMARY_CHARS` at a word boundary. Skipped: paragraphs
/// inside lists, short ones and those made mostly of link text (navigation). A paragraph after a lower
/// heading describes that section rather than the document, so the search stops there.
pub fn markdown_summary(md: &str) -> Option<String> {
    let mut text = String::new();
    let mut link_chars = 0;
    let mut in_paragraph = false;
    let (mut in_list, mut in_image, mut in_link) = (0, 0, 0);
    for event in Parser::new_ext(md, Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH) {
        match event {
            Event::Start(Tag::Heading { level, .. }) if level != HeadingLevel::H1 => return None,
            Event::Start(Tag::List(_)) => in_list += 1,
            Event::End(TagEnd::List(_)) => in_list -= 1,
            Event::Start(Tag::Paragraph) => in_paragraph = in_list == 0,
            Event::End(TagEnd::Paragraph) => {
                in_paragraph = false;
                let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
                let chars = flat.chars().count();
                if chars >= MIN_SUMMARY_CHARS && link_chars * 2 < chars {
                    return Some(shorten(&flat));
                }
                text.clear();
                link_chars = 0;
            }
            Event::Start(Tag::Image { .. }) => in_image += 1,
            Event::End(TagEnd::Image) => in_image -= 1,
            Event::Start(Tag::Link { .. }) => in_link += 1,
            Event::End(TagEnd::Link) => in_link -= 1,
            Event::Text(t) | Event::Code(t) if in_paragraph && in_image == 0 => {
                if in_link > 0 {
                    link_chars += t.chars().filter(|c| !c.is_whitespace()).count();
                }
                text.push_str(&t);
            }
            Event::SoftBreak | Event::HardBreak if in_paragraph => text.push(' '),
            _ => {}
        }
    }
    None
}

fn shorten(text: &str) -> String {
    if text.chars().count() <= SUMMARY_CHARS {
        return text.to_string();
    }
    let cut: String = text.chars().take(SUMMARY_CHARS).collect();
    // Back to the last space, unless that would drop most of the text.
    let end = cut
        .rfind(' ')
        .filter(|&i| i > cut.len() / 2)
        .unwrap_or(cut.len());
    format!("{}…", cut[..end].trim_end())
}

/// Extensions whose type we set ourselves. `mime_guess` treats `.ts` as MPEG-TS video
/// and knows no text type for some sources, which would make those files undisplayable.
const SOURCE_TYPES: &[(&str, &str)] = &[
    ("ts", "text/typescript"),
    ("mts", "text/typescript"),
    ("cts", "text/typescript"),
    ("js", "text/javascript"),
    ("mjs", "text/javascript"),
    ("cjs", "text/javascript"),
    ("go", "text/x-go"),
    ("rs", "text/x-rust"),
    ("sh", "text/x-shellscript"),
    ("bash", "text/x-shellscript"),
    ("yml", "text/yaml"),
    ("yaml", "text/yaml"),
    ("toml", "text/x-toml"),
    ("json", "application/json"),
];

/// Content type derived from the file extension.
pub fn detect_content_type(path: &str) -> String {
    if is_markdown("", path) {
        return "text/markdown".to_string();
    }
    let ext = path
        .rsplit('/')
        .next()
        .and_then(|name| name.rsplit_once('.'))
        .map(|(_, ext)| ext.to_ascii_lowercase());
    if let Some((_, ct)) = ext
        .as_deref()
        .and_then(|ext| SOURCE_TYPES.iter().find(|(e, _)| *e == ext))
    {
        return (*ct).to_string();
    }
    mime_guess::from_path(path)
        .first_raw()
        .unwrap_or("application/octet-stream")
        .to_string()
}
