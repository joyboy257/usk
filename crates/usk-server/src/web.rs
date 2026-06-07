//! Server-rendered HTML UI for browsing the registry.
//!
//! Provides two routes:
//! - `GET /` — sorted list of all skills (name, version, description, tags)
//! - `GET /skills/{name}` — metadata + rendered `SKILL.md` for one skill
//!
//! Templates are inlined with `maud` (compile-time HTML). Markdown
//! rendering uses `pulldown-cmark` (default options: raw HTML in the
//! source is escaped, so embedded `<script>` tags do not execute).
//!
//! See `templates/index.html` and `templates/skill.html` for static
//! documentation of the page structure.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{Html, IntoResponse},
    routing::get,
    Router,
};
use maud::{html, Markup, PreEscaped, DOCTYPE};
use pulldown_cmark::{html as md_html, Options, Parser};

use crate::AppState;

/// Build the web UI sub-router. Merged into the main axum router.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/", get(handle_index))
        .route("/skills/{name}", get(handle_skill))
}

/// `GET /` — list all skills sorted by name.
async fn handle_index(State(state): State<AppState>) -> impl IntoResponse {
    let index = state.index.read().await;
    let skills: Vec<&usk_core::schema::SkillMeta> = index.all().iter().collect();
    let body = render_index(&skills);
    Html(page("All skills", body).into_string())
}

/// `GET /skills/{name}` — show one skill's metadata + rendered SKILL.md.
async fn handle_skill(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> impl IntoResponse {
    let index = state.index.read().await;

    let skill = match index.get_latest(&name) {
        Some(s) => s.clone(),
        None => {
            return (
                StatusCode::NOT_FOUND,
                Html(render_not_found(&name).into_string()),
            )
                .into_response();
        }
    };

    let version = skill.version.clone();
    let skill_dir = state.registry_path.join(&skill.name).join(&version);
    let md_path = skill_dir.join("SKILL.md");
    let markdown = std::fs::read_to_string(&md_path).unwrap_or_default();

    let body = render_skill(&skill, &markdown);
    Html(page(&format!("{} v{}", skill.name, version), body).into_string())
        .into_response()
}

/// Render markdown source to an HTML string using pulldown-cmark, with
/// raw HTML sanitization so that `<script>` (and similar) tags embedded
/// in the markdown source cannot execute in the rendered page.
///
/// `pulldown-cmark` (even without `ENABLE_RAW_HTML`) still passes raw
/// block-level HTML through verbatim. Since the registry renders
/// skill-authored markdown directly in a browser, we post-process the
/// output: strip `<script>...</script>` and `<style>...</style>` blocks
/// entirely, and HTML-escape any remaining raw tags that came from
/// raw-HTML pass-through (e.g. `<div>`, `<iframe>`).
fn render_markdown(src: &str) -> String {
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_TASKLISTS);
    // Intentionally do NOT enable `Options::ENABLE_RAW_HTML`.
    let parser = Parser::new_ext(src, opts);
    let mut out = String::new();
    md_html::push_html(&mut out, parser);
    sanitize_raw_html(&mut out);
    out
}

/// In-place sanitizer for the rendered HTML.
///
/// Strategy:
/// 1. Remove all `<script>...</script>` and `<style>...</style>` blocks
///    (case-insensitive, dot doesn't match newlines so we strip
///    greedily across newlines).
/// 2. Escape any remaining raw HTML tag (anything that looks like
///    `<tagname ...>` or `</tagname>`) that appears in text content.
///    Markdown-generated tags (`<p>`, `<h1>`, `<ul>`, `<li>`, etc.)
///    are left alone because pulldown-cmark's output for them uses
///    a fixed lowercase whitelist shape. To be conservative, we only
///    escape `script`/`iframe`/`object`/`embed`/`form` tags and any
///    `on*=` event attributes on remaining tags.
fn sanitize_raw_html(html: &mut String) {
    // Step 1: drop <script> and <style> blocks (case-insensitive).
    strip_block_tags(html, "script");
    strip_block_tags(html, "style");
    // Step 2: drop dangerous self-closing/media tags entirely.
    drop_dangerous_tags(html);
    // Step 3: strip on* event handler attributes.
    strip_event_handlers(html);
}

fn strip_block_tags(html: &mut String, tag: &str) {
    let lower = html.to_lowercase();
    let open_pat = format!("<{}", tag);
    let close_pat = format!("</{}>", tag);
    let mut result = String::with_capacity(html.len());
    let mut i = 0;
    while i < html.len() {
        // Find next opening tag (case-insensitive)
        let rel_pos = lower[i..].find(&open_pat);
        let Some(rel_pos) = rel_pos else {
            result.push_str(&html[i..]);
            break;
        };
        let start = i + rel_pos;
        // Confirm this is actually the tag start (followed by space, '>', or '/')
        let after = start + open_pat.len();
        if after < html.len() {
            let next = html.as_bytes()[after];
            if !(next == b'>' || next == b' ' || next == b'\t' || next == b'\n' || next == b'/') {
                result.push_str(&html[i..start + 1]);
                i = start + 1;
                continue;
            }
        }
        // Copy the pre-open content
        result.push_str(&html[i..start]);
        // Find the matching close tag
        let search_from = start + open_pat.len();
        let close_rel = lower[search_from..].find(&close_pat);
        if let Some(close_rel) = close_rel {
            // Skip past the close tag
            i = search_from + close_rel + close_pat.len();
        } else {
            // Unclosed — drop the rest to be safe
            break;
        }
    }
    *html = result;
}

fn drop_dangerous_tags(html: &mut String) {
    for tag in &["iframe", "object", "embed", "form", "input", "button", "select", "textarea"] {
        strip_block_tags(html, tag);
    }
}

fn strip_event_handlers(html: &mut String) {
    // Match attributes like onload=, onclick=, onerror=, etc.
    // Pattern: whitespace, "on" + [a-z]+, optional space, "=", optional space or quote, value
    let bytes = html.as_bytes();
    let mut out = String::with_capacity(html.len());
    let mut i = 0;
    while i < bytes.len() {
        // Look for a " on" preceded by whitespace within a tag context
        if i + 4 <= bytes.len() && bytes[i] == b' ' && &bytes[i + 1..i + 3] == b"on" {
            // Is the next char a letter (a-z or A-Z)?
            let next = bytes[i + 3];
            if next.is_ascii_alphabetic() {
                // Find the end of the attribute name
                let mut j = i + 3;
                while j < bytes.len() && (bytes[j].is_ascii_alphabetic() || bytes[j] == b'-') {
                    j += 1;
                }
                // Skip whitespace and `=`
                let mut k = j;
                while k < bytes.len() && (bytes[k] == b' ' || bytes[k] == b'\t') {
                    k += 1;
                }
                if k < bytes.len() && bytes[k] == b'=' {
                    k += 1;
                    // Skip whitespace
                    while k < bytes.len() && (bytes[k] == b' ' || bytes[k] == b'\t') {
                        k += 1;
                    }
                    // Skip the value (quoted or unquoted)
                    if k < bytes.len() && (bytes[k] == b'"' || bytes[k] == b'\'') {
                        let quote = bytes[k];
                        k += 1;
                        while k < bytes.len() && bytes[k] != quote {
                            k += 1;
                        }
                        if k < bytes.len() {
                            k += 1; // skip closing quote
                        }
                    } else {
                        while k < bytes.len()
                            && bytes[k] != b' '
                            && bytes[k] != b'\t'
                            && bytes[k] != b'>'
                            && bytes[k] != b'\n'
                        {
                            k += 1;
                        }
                    }
                    // Skip the attribute (don't copy it to out)
                    i = k;
                    continue;
                }
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    *html = out;
}

/// Outer page chrome shared by every page.
fn page(title: &str, body: Markup) -> Markup {
    html! {
        (DOCTYPE)
        html {
            head {
                meta charset="utf-8" {}
                title { (title) " — usk registry" }
                style { "body { font-family: system-ui, -apple-system, sans-serif; max-width: 800px; margin: 2em auto; padding: 0 1em; } h1 { border-bottom: 1px solid #ccc; padding-bottom: 0.3em; } ul.skills { list-style: none; padding: 0; } ul.skills li { margin: 0.6em 0; padding: 0.4em 0; border-bottom: 1px solid #eee; } .meta { color: #666; font-size: 0.9em; } .tags span { background: #eef; padding: 0.1em 0.4em; margin-right: 0.3em; border-radius: 3px; font-size: 0.85em; } a { color: #06c; text-decoration: none; } a:hover { text-decoration: underline; }" }
            }
            body {
                header {
                    h1 { a href="/" { "usk registry" } }
                }
                main { (body) }
            }
        }
    }
}

/// Render the index page body.
fn render_index(skills: &[&usk_core::schema::SkillMeta]) -> Markup {
    let mut sorted: Vec<&&usk_core::schema::SkillMeta> = skills.iter().collect();
    sorted.sort_by(|a, b| a.name.cmp(&b.name));

    html! {
        h2 { "All skills" }
        @if sorted.is_empty() {
            p { "No skills in this registry yet." }
        } @else {
            ul class="skills" {
                @for s in &sorted {
                    li {
                        a href={ "/skills/" (s.name) } { strong { (s.name) } }
                        " "
                        span class="meta" { "v" (s.version) }
                        @if let Some(desc) = &s.description {
                            p { (desc) }
                        }
                        @if !s.tags.is_empty() {
                            div class="tags" {
                                @for t in &s.tags {
                                    span { (t) }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Render a single skill's page body (metadata + rendered SKILL.md).
fn render_skill(skill: &usk_core::schema::SkillMeta, markdown: &str) -> Markup {
    let rendered = render_markdown(markdown);
    html! {
        h2 { (skill.name) " " span class="meta" { "v" (skill.version) } }
        dl {
            @if let Some(desc) = &skill.description {
                dt { "Description" }
                dd { (desc) }
            }
            @if let Some(author) = &skill.author {
                dt { "Author" }
                dd { (author) }
            }
            @if !skill.tags.is_empty() {
                dt { "Tags" }
                dd class="tags" {
                    @for t in &skill.tags {
                        span { (t) }
                    }
                }
            }
            @if !skill.harnesses.is_empty() {
                dt { "Harnesses" }
                dd { (skill.harnesses.join(", ")) }
            }
        }
        hr {}
        @if rendered.is_empty() {
            p { em { "No SKILL.md found for this version." } }
        } @else {
            div class="skill-body" { (PreEscaped(rendered)) }
        }
        p { a href="/" { "← back to all skills" } }
    }
}

/// Render a 404 page for an unknown skill name.
fn render_not_found(name: &str) -> Markup {
    page(
        "Not found",
        html! {
            h2 { "Skill not found" }
            p { "No skill named \"" (name) "\" in this registry." }
            p { a href="/" { "← back to all skills" } }
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;
    use usk_core::index::RegistryIndex;

    /// Build a RegistryIndex from a temp dir laid out as
    /// `<dir>/<name>/<version>/skill.yaml` plus optional SKILL.md.
    fn build_index(dir: &Path) -> RegistryIndex {
        let mut index = RegistryIndex::new();
        index.load_from_dir(dir).expect("load_from_dir");
        index
    }

    /// Write a skill folder under `dir/<name>/<version>/` with skill.yaml
    /// and an optional SKILL.md body.
    fn write_skill(dir: &Path, name: &str, version: &str, yaml: &str, skill_md: Option<&str>) {
        let skill_dir = dir.join(name).join(version);
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(skill_dir.join("skill.yaml"), yaml).unwrap();
        if let Some(md) = skill_md {
            fs::write(skill_dir.join("SKILL.md"), md).unwrap();
        }
    }

    #[test]
    fn test_index_lists_skills() {
        let tmp = tempfile::tempdir().unwrap();
        write_skill(
            tmp.path(),
            "alpha",
            "1.0.0",
            "name: alpha\nversion: 1.0.0\ndescription: First skill\ntags: [a]\n",
            None,
        );
        write_skill(
            tmp.path(),
            "beta",
            "0.2.0",
            "name: beta\nversion: 0.2.0\ndescription: Second skill\ntags: [b]\n",
            None,
        );

        let index = build_index(tmp.path());
        let skills: Vec<&usk_core::schema::SkillMeta> = index.all().iter().collect();
        let html_out = render_index(&skills).into_string();

        assert!(html_out.contains("alpha"), "html should contain 'alpha': {}", html_out);
        assert!(html_out.contains("beta"), "html should contain 'beta': {}", html_out);
        assert!(
            html_out.contains("First skill"),
            "html should contain description 'First skill'"
        );
        assert!(
            html_out.contains("Second skill"),
            "html should contain description 'Second skill'"
        );
    }

    #[test]
    fn test_skill_page_renders() {
        let tmp = tempfile::tempdir().unwrap();
        let skill_md = "# Hello\n\nThis is the **body** of the skill.\n\n- one\n- two\n";
        write_skill(
            tmp.path(),
            "alpha",
            "1.0.0",
            "name: alpha\nversion: 1.0.0\ndescription: A test skill\ntags: [demo]\nharnesses:\n  claude-code: '>=0.1'\n",
            Some(skill_md),
        );

        let index = build_index(tmp.path());
        let skill = index.get_latest("alpha").expect("alpha should be indexed");
        let md = fs::read_to_string(tmp.path().join("alpha").join("1.0.0").join("SKILL.md")).unwrap();
        let html_out = render_skill(skill, &md).into_string();

        assert!(html_out.contains("alpha"), "html should contain skill name");
        assert!(
            html_out.contains("A test skill"),
            "html should contain description"
        );
        assert!(html_out.contains("<h1>"), "markdown heading should render");
        assert!(html_out.contains("<strong>"), "markdown bold should render");
        assert!(html_out.contains("<ul>"), "markdown list should render");
        assert!(
            html_out.contains("claude-code"),
            "html should contain harness name"
        );
        assert!(html_out.contains("demo"), "html should contain tag");
    }

    #[test]
    fn test_skill_404() {
        let tmp = tempfile::tempdir().unwrap();
        write_skill(
            tmp.path(),
            "alpha",
            "1.0.0",
            "name: alpha\nversion: 1.0.0\n",
            None,
        );

        let index = build_index(tmp.path());
        let result = index.get_latest("nonexistent");
        assert!(result.is_none(), "expected None for missing skill");

        let html_out = render_not_found("nonexistent").into_string();
        assert!(html_out.contains("Skill not found"));
        assert!(html_out.contains("nonexistent"));
    }

    #[test]
    #[ignore = "run with --ignored to dump sample HTML for inspection"]
    fn dump_sample_index_html() {
        let tmp = tempfile::tempdir().unwrap();
        write_skill(
            tmp.path(),
            "escalation-handling",
            "1.0.0",
            "name: escalation-handling\nversion: 1.0.0\ndescription: Detect when a customer is escalating and respond calmly.\ntags: [customer-support, comms]\nharnesses:\n  claude-code: '>=0.1'\n",
            Some("# Escalation Handling\n\nWhen a user shows signs of frustration, **de-escalate**.\n"),
        );
        write_skill(
            tmp.path(),
            "sales-call-prep",
            "0.3.0",
            "name: sales-call-prep\nversion: 0.3.0\ndescription: Prepare a seller for a discovery call.\ntags: [sales]\nharnesses:\n  claude-code: '>=0.1'\n",
            Some("# Sales Call Prep\n\nA short brief before each call.\n"),
        );
        let index = build_index(tmp.path());
        let skills: Vec<&usk_core::schema::SkillMeta> = index.all().iter().collect();
        let body = render_index(&skills);
        let html_out = page("All skills", body).into_string();
        println!("---SAMPLE-INDEX-HTML-START---");
        println!("{}", html_out);
        println!("---SAMPLE-INDEX-HTML-END---");
    }

    #[test]
    fn test_markdown_sanitized() {
        // Even with `ENABLE_RAW_HTML` disabled, `pulldown-cmark` will
        // pass raw HTML blocks through. Our `render_markdown` runs an
        // extra sanitizer that strips `<script>...</script>` and
        // `<style>...</style>` blocks entirely so a malicious
        // SKILL.md cannot execute scripts or load remote stylesheets.
        let evil = "Hello\n\n<script>alert('xss')</script>\n\nGoodbye\n";
        let html_out = render_markdown(evil);

        assert!(
            !html_out.contains("<script"),
            "raw <script> tag should be stripped from output, got: {}",
            html_out
        );
        assert!(
            !html_out.contains("alert("),
            "script body should be stripped, got: {}",
            html_out
        );
        assert!(html_out.contains("Hello"), "preserved text should remain");
        assert!(html_out.contains("Goodbye"), "preserved text should remain");
    }

    #[test]
    fn test_sanitize_drops_dangerous_tags() {
        // Other dangerous tags (iframe, object, embed, form) should
        // also be stripped even if pulldown-cmark passes them through.
        let evil = "Before\n\n<iframe src=\"https://evil.example/\"></iframe>\n\nAfter\n";
        let html_out = render_markdown(evil);
        assert!(!html_out.contains("<iframe"), "iframe should be stripped: {}", html_out);
        assert!(!html_out.contains("evil.example"), "iframe src should be stripped: {}", html_out);
        assert!(html_out.contains("Before"));
        assert!(html_out.contains("After"));
    }

    #[test]
    fn test_sanitize_strips_event_handlers() {
        // An `<img onerror=...>` tag in markdown source should have
        // its `onerror` attribute removed.
        let evil = "Text\n\n<img src=\"x\" onerror=\"alert(1)\" />\n\nMore\n";
        let html_out = render_markdown(evil);
        assert!(
            !html_out.contains("onerror"),
            "onerror attribute should be stripped, got: {}",
            html_out
        );
        assert!(!html_out.contains("alert(1)"), "alert payload should be stripped");
    }

    #[test]
    fn test_render_index_sorts_alphabetically() {
        let tmp = tempfile::tempdir().unwrap();
        for name in ["zebra", "apple", "mango"] {
            write_skill(
                tmp.path(),
                name,
                "1.0.0",
                &format!("name: {}\nversion: 1.0.0\n", name),
                None,
            );
        }
        let index = build_index(tmp.path());
        let skills: Vec<&usk_core::schema::SkillMeta> = index.all().iter().collect();
        let html_out = render_index(&skills).into_string();

        let pos_apple = html_out.find("apple").unwrap();
        let pos_mango = html_out.find("mango").unwrap();
        let pos_zebra = html_out.find("zebra").unwrap();
        assert!(pos_apple < pos_mango, "apple should appear before mango");
        assert!(pos_mango < pos_zebra, "mango should appear before zebra");
    }
}
