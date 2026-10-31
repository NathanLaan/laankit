//! Markdown → standalone HTML for the Convert menu (HTML file / PDF via the
//! host's print-to-PDF). The Electron build used `marked`; comrak with the
//! GFM extensions renders the same constructs.

use std::path::Path;

fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

pub fn markdown_to_html(md: &str) -> String {
    let mut opts = comrak::Options::default();
    opts.extension.table = true;
    opts.extension.strikethrough = true;
    opts.extension.autolink = true;
    opts.extension.tasklist = true;
    opts.render.unsafe_ = true; // marked passes raw HTML through
    comrak::markdown_to_html(md, &opts)
}

/// The native preview's HTML: [`markdown_to_html`] plus comrak's math (`$…$` and `$$…$$` as
/// `<span data-math-style>`, ```` ```math ```` blocks), which the native build typesets. The
/// HTML export and the Tauri build keep [`markdown_to_html`], where math stays text as `marked`
/// leaves it.
pub fn preview_html(md: &str) -> String {
    let mut opts = comrak::Options::default();
    opts.extension.table = true;
    opts.extension.strikethrough = true;
    opts.extension.autolink = true;
    opts.extension.tasklist = true;
    opts.extension.math_dollars = true;
    opts.extension.math_code = true;
    opts.render.unsafe_ = true;
    comrak::markdown_to_html(md, &opts)
}

const STYLE: &str = "    h1, h2, h3, h4 { margin-top: 1.5em; }
    code { background: #f0f0f0; padding: 2px 6px; border-radius: 3px; font-size: 0.9em; }
    pre { background: #f0f0f0; padding: 16px; border-radius: 6px; overflow-x: auto; }
    pre code { background: none; padding: 0; }
    img { max-width: 100%; }
    blockquote { border-left: 3px solid #ccc; margin-left: 0; padding-left: 16px; color: #555; }
    table { border-collapse: collapse; width: 100%; }
    th, td { border: 1px solid #ddd; padding: 8px 12px; text-align: left; }
    th { background: #f5f5f5; }";

/// The HTML-export document (file:convertToHtml).
pub fn html_document(name: &str, body_md: &str) -> String {
    let title = escape_html(name);
    format!(
        "<!DOCTYPE html>
<html lang=\"en\">
<head>
  <meta charset=\"utf-8\">
  <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">
  <title>{title}</title>
  <style>
    body {{ font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif; max-width: 800px; margin: 40px auto; padding: 0 20px; line-height: 1.6; color: #1a1a1a; }}
{STYLE}
  </style>
</head>
<body>
  <h1>{title}</h1>
  {}
</body>
</html>",
        markdown_to_html(body_md)
    )
}

/// The print document for PDF export: `_attachments/` references become
/// absolute file:// URLs so images render.
pub fn pdf_document(name: &str, body_md: &str, attachments_dir: &Path) -> String {
    let re = regex::Regex::new(r#"(src|href)="\.?/?_attachments/([^"]+)""#).unwrap();
    let dir = attachments_dir.display().to_string();
    let body = re
        .replace_all(&markdown_to_html(body_md), |c: &regex::Captures| {
            format!("{}=\"file://{dir}/{}\"", &c[1], &c[2])
        })
        .into_owned();
    let title = escape_html(name);
    format!(
        "<!DOCTYPE html>
<html lang=\"en\">
<head>
  <meta charset=\"utf-8\">
  <title>{title}</title>
  <style>
    @page {{ size: letter; margin: 0.5in; }}
    body {{ font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif; margin: 0; padding: 0 24px; line-height: 1.6; color: #1a1a1a; }}
{STYLE}
  </style>
</head>
<body>
  <h1>{title}</h1>
  {body}
</body>
</html>"
    )
}

/// `name` → `name.ext` slug used for exported files.
pub fn export_slug(name: &str) -> String {
    crate::project::slugify(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gfm_constructs_render() {
        let html = markdown_to_html(
            "# T\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n~~x~~ - [ ] task\n\n<b>raw</b>",
        );
        assert!(html.contains("<table>"));
        assert!(html.contains("<del>x</del>"));
        assert!(html.contains("<b>raw</b>"));
    }

    #[test]
    fn pdf_rewrites_attachment_urls() {
        let html = pdf_document(
            "N",
            "![x](./_attachments/a.png)",
            Path::new("/p/_attachments"),
        );
        assert!(html.contains("src=\"file:///p/_attachments/a.png\""));
    }
}
