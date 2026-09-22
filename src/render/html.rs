//! Безопасное HTML-представление результата для форматированного буфера обмена.

use crate::model::{is_valid_size, normalize_hex_color};

/// Преобразует поддерживаемый BBCode в HTML-фрагмент.
///
/// HTML кладётся в буфер вместе с исходным BBCode как plain-text fallback:
/// приложения, которые понимают rich clipboard, получают форматирование, а
/// остальные — тот же результат, который пользователь видит во вкладке BBCode.
pub fn bbcode_to_html(source: &str) -> String {
    let mut html =
        String::from("<div style=\"font-family:Segoe UI,Arial,sans-serif;white-space:normal;\">");
    render_fragment(source, &mut html);
    html.push_str("</div>");
    html
}

#[derive(Debug)]
struct TagSpec {
    name: &'static str,
    opening: String,
    closing: String,
    code: bool,
    self_closing: bool,
}

fn render_fragment(source: &str, html: &mut String) {
    let mut position = 0;
    let mut line_start = true;
    let mut quote_open = false;

    while position < source.len() {
        if line_start && source[position..].starts_with(">>") {
            if !quote_open {
                html.push_str(
                    "<blockquote style=\"margin:0 0 0 1em;padding-left:.75em;border-left:3px solid #9aa4b2;\">",
                );
                quote_open = true;
            }
            position += 2;
            line_start = false;
            continue;
        }

        let character = source[position..]
            .chars()
            .next()
            .expect("position must point at a character");
        if character == '\n' {
            html.push_str("<br>\n");
            position += character.len_utf8();
            line_start = true;
            if quote_open && !source[position..].starts_with(">>") {
                html.push_str("</blockquote>");
                quote_open = false;
            }
            continue;
        }

        if character == '['
            && let Some((tag_end, raw_tag)) = parse_tag(source, position)
            && let Some(spec) = opening_tag(raw_tag)
        {
            if spec.self_closing {
                html.push_str(&spec.opening);
                position = tag_end;
                line_start = false;
                continue;
            }

            if let Some((content_end, closing_end)) = find_closing_tag(source, tag_end, spec.name) {
                let content = &source[tag_end..content_end];
                if spec.code {
                    html.push_str(&spec.opening);
                    escape_html(content, html);
                    html.push_str(&spec.closing);
                } else if spec.name == "url" && raw_tag.trim().eq_ignore_ascii_case("url") {
                    let plain_content = strip_bbcode(content);
                    if let Some(href) = safe_href(plain_content.trim()) {
                        html.push_str("<a href=\"");
                        escape_html(&href, html);
                        html.push_str("\">");
                        render_fragment(content, html);
                        html.push_str("</a>");
                    } else {
                        html.push_str(&spec.opening);
                        render_fragment(content, html);
                        html.push_str(&spec.closing);
                    }
                } else {
                    html.push_str(&spec.opening);
                    render_fragment(content, html);
                    html.push_str(&spec.closing);
                }
                position = closing_end;
                line_start = false;
                continue;
            }
        }

        let text_start = position;
        while position < source.len() {
            let remainder = &source[position..];
            if remainder.starts_with('\n') || remainder.starts_with('[') {
                break;
            }
            if line_start && remainder.starts_with(">>") {
                break;
            }
            let next = remainder
                .chars()
                .next()
                .expect("position must point at a character");
            position += next.len_utf8();
            line_start = false;
        }
        if position == text_start {
            escape_html(&source[position..position + character.len_utf8()], html);
            position += character.len_utf8();
            line_start = false;
        } else {
            escape_html(&source[text_start..position], html);
        }
    }

    if quote_open {
        html.push_str("</blockquote>");
    }
}

fn parse_tag(source: &str, start: usize) -> Option<(usize, &str)> {
    let end = source[start..].find(']')? + start;
    Some((end + 1, &source[start + 1..end]))
}

fn opening_tag(raw_tag: &str) -> Option<TagSpec> {
    let tag = raw_tag.trim();
    if tag.is_empty() || tag.starts_with('/') {
        return None;
    }
    let lower = tag.to_ascii_lowercase();

    match lower.as_str() {
        "b" => Some(element("b", "<strong>", "</strong>")),
        "strong" => Some(element("strong", "<strong>", "</strong>")),
        "i" => Some(element("i", "<em>", "</em>")),
        "em" => Some(element("em", "<em>", "</em>")),
        "u" => Some(element(
            "u",
            "<span style=\"text-decoration:underline;\">",
            "</span>",
        )),
        "s" | "del" | "strike" => Some(element(
            "s",
            "<span style=\"text-decoration:line-through;\">",
            "</span>",
        )),
        "url" => Some(element(
            "url",
            "<span style=\"color:#609cff;text-decoration:underline;\">",
            "</span>",
        )),
        "code" => Some(TagSpec {
            name: "code",
            opening: "<pre style=\"font-family:Consolas,monospace;\"><code>".to_string(),
            closing: "</code></pre>".to_string(),
            code: true,
            self_closing: false,
        }),
        "user" => Some(element(
            "user",
            "<span style=\"color:#609cff;\">",
            "</span>",
        )),
        _ if lower.starts_with("url=") => {
            let href = tag[4..].trim();
            let (opening, closing) = match safe_href(href) {
                Some(href) => {
                    let mut opening = String::from("<a href=\"");
                    escape_html(&href, &mut opening);
                    opening.push_str("\">");
                    (opening, "</a>".to_string())
                }
                None => (
                    "<span style=\"color:#609cff;text-decoration:underline;\">".to_string(),
                    "</span>".to_string(),
                ),
            };
            Some(element_with_html("url", opening, closing))
        }
        _ if lower.starts_with("user=") => Some(element(
            "user",
            "<span style=\"color:#609cff;\">",
            "</span>",
        )),
        _ if lower.starts_with("color=") => {
            let color = normalize_hex_color(tag[6..].trim())?;
            Some(element_with_html(
                "color",
                format!("<span style=\"color:{color};\">"),
                "</span>".to_string(),
            ))
        }
        _ if lower.starts_with("size=") => {
            let size = tag[5..].trim().parse::<u8>().ok()?;
            is_valid_size(size).then(|| {
                element_with_html(
                    "size",
                    format!("<span style=\"font-size:{size}px;\">"),
                    "</span>".to_string(),
                )
            })
        }
        _ if lower.starts_with("icon=") => Some(TagSpec {
            name: "icon",
            opening: "<span style=\"color:#9696dc;\">◆</span>".to_string(),
            closing: String::new(),
            code: false,
            self_closing: true,
        }),
        _ => None,
    }
}

fn element(name: &'static str, opening: &str, closing: &str) -> TagSpec {
    element_with_html(name, opening.to_string(), closing.to_string())
}

fn element_with_html(name: &'static str, opening: String, closing: String) -> TagSpec {
    TagSpec {
        name,
        opening,
        closing,
        code: false,
        self_closing: false,
    }
}

fn find_closing_tag(source: &str, start: usize, name: &str) -> Option<(usize, usize)> {
    let opening = format!("{name}=");
    let closing = format!("/{name}");
    let mut position = start;
    let mut depth = 1;
    while let Some(relative) = source[position..].find('[') {
        let tag_start = position + relative;
        let (tag_end, raw_tag) = parse_tag(source, tag_start)?;
        let tag = raw_tag.trim().to_ascii_lowercase();
        if tag == closing {
            depth -= 1;
            if depth == 0 {
                return Some((tag_start, tag_end));
            }
        } else if tag == name || tag.starts_with(&opening) {
            depth += 1;
        }
        position = tag_end;
    }
    None
}

fn safe_href(value: &str) -> Option<String> {
    let value = value.trim();
    let lower = value.to_ascii_lowercase();
    (lower.starts_with("https://") || lower.starts_with("http://") || lower.starts_with("mailto:"))
        .then(|| value.to_string())
}

fn strip_bbcode(source: &str) -> String {
    let mut text = String::new();
    let mut position = 0;
    while position < source.len() {
        if source[position..].starts_with('[')
            && let Some((tag_end, _)) = parse_tag(source, position)
        {
            position = tag_end;
            continue;
        }
        let character = source[position..]
            .chars()
            .next()
            .expect("position must point at a character");
        text.push(character);
        position += character.len_utf8();
    }
    text
}

fn escape_html(source: &str, html: &mut String) {
    for character in source.chars() {
        match character {
            '&' => html.push_str("&amp;"),
            '<' => html.push_str("&lt;"),
            '>' => html.push_str("&gt;"),
            '"' => html.push_str("&quot;"),
            '\'' => html.push_str("&#39;"),
            _ => html.push(character),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_formatting_and_escapes_text() {
        let html =
            bbcode_to_html("[b]Жирный[/b] [url=https://example.com?a=1&b=2]ссылка[/url]\n<script>");

        assert!(html.contains("<strong>Жирный</strong>"));
        assert!(html.contains("href=\"https://example.com?a=1&amp;b=2\""));
        assert!(html.contains("&lt;script&gt;"));
        assert!(html.contains("<br>"));
    }

    #[test]
    fn preserves_code_as_literal_text_and_rejects_unsafe_links() {
        let html = bbcode_to_html("[url=javascript:alert(1)]x[/url]\n[code]<b>x</b>[/code]");

        assert!(!html.contains("javascript:"));
        assert!(html.contains("&lt;b&gt;x&lt;/b&gt;"));
        assert!(html.contains("<pre"));
    }

    #[test]
    fn renders_quote_prefix_as_blockquote() {
        let html = bbcode_to_html(">>[b]цитата[/b]\n>>продолжение");

        assert!(html.contains("<blockquote"));
        assert!(html.contains("<strong>цитата</strong>"));
        assert_eq!(html.matches("<blockquote").count(), 1);
    }
}
