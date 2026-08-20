//! Preview итогового BBCode с поддержкой только разрешённых Bitrix24-тегов.

use egui::text::LayoutJob;
use egui::{Color32, FontId, Stroke, TextFormat, Ui};

use crate::settings::AppSettings;

#[derive(Clone, Copy)]
struct Fmt {
    size: f32,
    color: Option<Color32>,
    strong: bool,
    italics: bool,
    underline: bool,
    strike: bool,
    code: bool,
}

impl Fmt {
    fn base(size: f32) -> Self {
        Self {
            size,
            color: None,
            strong: false,
            italics: false,
            underline: false,
            strike: false,
            code: false,
        }
    }

    fn text_format(&self, ui: &Ui) -> TextFormat {
        let visuals = ui.visuals();
        let color = self.color.unwrap_or(if self.strong {
            visuals.strong_text_color()
        } else {
            visuals.text_color()
        });
        let font_id = if self.code {
            FontId::monospace(self.size * 0.92)
        } else {
            FontId::proportional(self.size)
        };
        TextFormat {
            font_id,
            color,
            italics: self.italics,
            underline: if self.underline { Stroke::new(1.0, color) } else { Stroke::NONE },
            strikethrough: if self.strike { Stroke::new(1.0, color) } else { Stroke::NONE },
            background: if self.code { visuals.extreme_bg_color } else { Color32::TRANSPARENT },
            ..Default::default()
        }
    }
}

fn parse_hex(hex: &str) -> Option<Color32> {
    let h = hex.strip_prefix('#')?;
    let expand = |s: &str| -> Option<u8> { u8::from_str_radix(s, 16).ok() };
    match h.len() {
        3 => {
            let r = expand(&h[0..1].repeat(2))?;
            let g = expand(&h[1..2].repeat(2))?;
            let b = expand(&h[2..3].repeat(2))?;
            Some(Color32::from_rgb(r, g, b))
        }
        6 => {
            let r = expand(&h[0..2])?;
            let g = expand(&h[2..4])?;
            let b = expand(&h[4..6])?;
            Some(Color32::from_rgb(r, g, b))
        }
        _ => None,
    }
}

pub fn show_preview(ui: &mut Ui, bbcode: &str, settings: &AppSettings) {
    if bbcode.is_empty() {
        ui.weak("Пусто. Начните вводить Markdown в редакторе слева.");
        return;
    }
    let mut job = LayoutJob::default();
    append_bbcode(ui, &mut job, bbcode, Fmt::base(settings.output_font_size));
    ui.label(job);
}

fn append_bbcode(ui: &Ui, job: &mut LayoutJob, source: &str, fmt: Fmt) {
    let mut remaining = source;
    while let Some(open) = remaining.find('[') {
        if open > 0 {
            job.append(&remaining[..open], 0.0, fmt.text_format(ui));
            remaining = &remaining[open..];
        }

        let Some(close) = remaining.find(']') else {
            job.append(remaining, 0.0, fmt.text_format(ui));
            return;
        };
        let tag = &remaining[1..close];
        let after_open = &remaining[close + 1..];
        if let Some((closing_tag, next_fmt)) = opening_tag(tag, fmt)
            && let Some(content_end) = find_closing_tag(after_open, closing_tag)
        {
            append_bbcode(ui, job, &after_open[..content_end], next_fmt);
            remaining = &after_open[content_end + closing_tag.len() + 3..];
            continue;
        }
        if let Some(icon_fmt) = icon_tag(tag, fmt) {
            job.append("◆", 0.0, icon_fmt.text_format(ui));
            remaining = after_open;
            continue;
        }
        job.append("[", 0.0, fmt.text_format(ui));
        remaining = &remaining[1..];
    }
    job.append(remaining, 0.0, fmt.text_format(ui));
}

fn opening_tag(tag: &str, fmt: Fmt) -> Option<(&'static str, Fmt)> {
    let lower = tag.to_ascii_lowercase();
    let mut next = fmt;
    match lower.as_str() {
        "b" => next.strong = true,
        "i" => next.italics = true,
        "u" => next.underline = true,
        "s" => next.strike = true,
        "url" | "user" => {
            next.color = Some(Color32::from_rgb(96, 156, 255));
            next.underline = true;
        }
        _ if lower.starts_with("url=") || lower.starts_with("user=") => {
            next.color = Some(Color32::from_rgb(96, 156, 255));
            next.underline = true;
        }
        _ if lower.starts_with("color=") => next.color = parse_hex(&tag[6..]).or(next.color),
        _ if lower.starts_with("size=") => next.size = tag[5..].trim().parse::<f32>().ok()?,
        _ => return None,
    }
    let closing_tag = match lower.split_once('=').map_or(lower.as_str(), |(name, _)| name) {
        "b" => "b",
        "i" => "i",
        "u" => "u",
        "s" => "s",
        "url" => "url",
        "user" => "user",
        "color" => "color",
        "size" => "size",
        _ => return None,
    };
    Some((closing_tag, next))
}

fn find_closing_tag(source: &str, tag: &str) -> Option<usize> {
    let mut offset = 0;
    let mut depth = 1;
    while let Some(start) = source[offset..].find('[') {
        let start = offset + start;
        let end = source[start..].find(']')? + start;
        let candidate = source[start + 1..end].to_ascii_lowercase();
        if candidate == format!("/{tag}") {
            depth -= 1;
            if depth == 0 {
                return Some(start);
            }
        } else if candidate == tag || candidate.starts_with(&format!("{tag}=")) {
            depth += 1;
        }
        offset = end + 1;
    }
    None
}

fn icon_tag(tag: &str, fmt: Fmt) -> Option<Fmt> {
    tag.to_ascii_lowercase().starts_with("icon=").then(|| Fmt {
        color: Some(Color32::from_rgb(150, 150, 220)),
        ..fmt
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_nested_identical_tags() {
        assert_eq!(find_closing_tag("outer [b]inner[/b] tail[/b]", "b"), Some(23));
    }

    #[test]
    fn accepts_only_bitrix_preview_tags() {
        assert!(opening_tag("color=#ff0000", Fmt::base(16.0)).is_some());
        assert!(opening_tag("url=https://example.com", Fmt::base(16.0)).is_some());
        assert!(opening_tag("img=https://example.com/image.png", Fmt::base(16.0)).is_none());
    }

}
