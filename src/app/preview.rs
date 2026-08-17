//! GUI-preview документа с ручной подсветкой кода.

use egui::text::LayoutJob;
use egui::{Color32, FontId, Stroke, TextFormat, Ui};

use crate::highlight::{TokenKind, highlight_line};
use crate::model::{BlockNode, Document, InlineNode};
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

pub fn show_preview(ui: &mut Ui, doc: &Document, settings: &AppSettings) {
    let base_size = settings.editor_font_size;
    for (i, block) in doc.blocks.iter().enumerate() {
        if i > 0 {
            ui.add_space(8.0);
        }
        show_block(ui, block, settings, base_size, 0);
    }
    if doc.blocks.is_empty() {
        ui.weak("Пусто. Начните вводить Markdown в редакторе слева.");
    }
}

fn show_block(ui: &mut Ui, block: &BlockNode, settings: &AppSettings, base_size: f32, depth: usize) {
    match block {
        BlockNode::Paragraph(inlines) => {
            let mut job = LayoutJob::default();
            append_inlines(ui, &mut job, inlines, Fmt::base(base_size));
            ui.label(job);
        }
        BlockNode::Heading { level, content } => {
            let idx = (level.saturating_sub(1) as usize).min(5);
            let px = settings.heading_sizes[idx];
            let size = if px == 0 { base_size + 2.0 } else { px as f32 };
            let mut fmt = Fmt::base(size);
            fmt.strong = true;
            let mut job = LayoutJob::default();
            append_inlines(ui, &mut job, content, fmt);
            ui.label(job);
        }
        BlockNode::Quote(blocks) => {
            let accent = ui.visuals().selection.bg_fill;
            egui::Frame::group(ui.style())
                .stroke(Stroke::new(2.0, accent))
                .show(ui, |ui| {
                    for b in blocks {
                        show_block(ui, b, settings, base_size, depth);
                    }
                });
        }
        BlockNode::CodeBlock { language, code } => {
            show_code_block(ui, language.as_deref(), code, settings, base_size);
        }
        BlockNode::List { ordered, start, items } => {
            for (i, item) in items.iter().enumerate() {
                ui.horizontal_top(|ui| {
                    ui.add_space(depth as f32 * 18.0);
                    let marker = if *ordered {
                        format!("{}.", start + i as u64)
                    } else {
                        settings.bullet_marker.as_str().to_string()
                    };
                    ui.label(egui::RichText::new(marker).size(base_size).strong());
                    ui.vertical(|ui| {
                        for b in item {
                            show_block(ui, b, settings, base_size, depth + 1);
                        }
                    });
                });
            }
        }
        BlockNode::Table { rows } => {
            egui::Grid::new(ui.next_auto_id()).striped(true).show(ui, |ui| {
                for row in rows {
                    for cell in row {
                        let mut job = LayoutJob::default();
                        append_inlines(ui, &mut job, cell, Fmt::base(base_size));
                        ui.label(job);
                    }
                    ui.end_row();
                }
            });
        }
        BlockNode::Image { url, alt, size } => {
            let size_label = size.map(|s| s.as_str()).unwrap_or("размер по умолчанию");
            ui.horizontal(|ui| {
                ui.label("🖼");
                ui.hyperlink_to(
                    if alt.is_empty() { url.clone() } else { format!("{alt} — {url}") },
                    url.clone(),
                );
                ui.weak(format!("({size_label})"));
            });
        }
        BlockNode::HorizontalRule => {
            ui.separator();
        }
    }
}

fn show_code_block(ui: &mut Ui, language: Option<&str>, code: &str, settings: &AppSettings, base_size: f32) {
    let mono = FontId::monospace(base_size * 0.95);
    egui::Frame::group(ui.style())
        .fill(ui.visuals().extreme_bg_color)
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            if let Some(lang) = language {
                ui.weak(format!("код · {lang}"));
            }
            if settings.manual_highlight_preview {
                let lang = language.unwrap_or("");
                for line in code.lines() {
                    let mut job = LayoutJob::default();
                    for tok in highlight_line(lang, line) {
                        let color = token_color(ui, tok.kind);
                        job.append(
                            &tok.text,
                            0.0,
                            TextFormat { font_id: mono.clone(), color, ..Default::default() },
                        );
                    }
                    if line.is_empty() {
                        job.append(" ", 0.0, TextFormat { font_id: mono.clone(), ..Default::default() });
                    }
                    ui.label(job);
                }
            } else {
                ui.label(egui::RichText::new(code).font(mono.clone()));
            }
        });
}

fn token_color(ui: &Ui, kind: TokenKind) -> Color32 {
    let dark = ui.visuals().dark_mode;
    match kind {
        TokenKind::Keyword => {
            if dark { Color32::from_rgb(198, 120, 221) } else { Color32::from_rgb(150, 0, 150) }
        }
        TokenKind::String => {
            if dark { Color32::from_rgb(152, 195, 121) } else { Color32::from_rgb(0, 128, 0) }
        }
        TokenKind::Comment => {
            if dark { Color32::from_rgb(106, 115, 125) } else { Color32::from_rgb(128, 128, 128) }
        }
        TokenKind::Number => {
            if dark { Color32::from_rgb(209, 154, 102) } else { Color32::from_rgb(170, 85, 0) }
        }
        TokenKind::Plain => ui.visuals().text_color(),
    }
}

fn append_inlines(ui: &Ui, job: &mut LayoutJob, inlines: &[InlineNode], fmt: Fmt) {
    for node in inlines {
        match node {
            InlineNode::Text(t) => job.append(t, 0.0, fmt.text_format(ui)),
            InlineNode::Bold(c) => {
                let mut f = fmt;
                f.strong = true;
                append_inlines(ui, job, c, f);
            }
            InlineNode::Italic(c) => {
                let mut f = fmt;
                f.italics = true;
                append_inlines(ui, job, c, f);
            }
            InlineNode::Underline(c) => {
                let mut f = fmt;
                f.underline = true;
                append_inlines(ui, job, c, f);
            }
            InlineNode::Strike(c) => {
                let mut f = fmt;
                f.strike = true;
                append_inlines(ui, job, c, f);
            }
            InlineNode::Link { text, url } => {
                let mut f = fmt;
                f.color = Some(Color32::from_rgb(96, 156, 255));
                f.underline = true;
                if text.is_empty() {
                    job.append(url, 0.0, f.text_format(ui));
                } else {
                    append_inlines(ui, job, text, f);
                }
            }
            InlineNode::User { text, .. } => {
                let mut f = fmt;
                f.color = Some(Color32::from_rgb(96, 156, 255));
                append_inlines(ui, job, text, f);
            }
            InlineNode::Color { hex, content } => {
                let mut f = fmt;
                f.color = parse_hex(hex).or(f.color);
                append_inlines(ui, job, content, f);
            }
            InlineNode::Size { px, content } => {
                let mut f = fmt;
                f.size = *px as f32;
                append_inlines(ui, job, content, f);
            }
            InlineNode::Icon { url, .. } => {
                let mut f = fmt;
                f.color = Some(Color32::from_rgb(150, 150, 220));
                job.append(&format!("◆ {url}"), 0.0, f.text_format(ui));
            }
            InlineNode::Image { url, alt, .. } => {
                let mut f = fmt;
                f.color = Some(Color32::from_rgb(96, 156, 255));
                let label = if alt.is_empty() { format!("🖼 {url}") } else { format!("🖼 {alt}") };
                job.append(&label, 0.0, f.text_format(ui));
            }
            InlineNode::Code(code) => {
                let mut f = fmt;
                f.code = true;
                job.append(code, 0.0, f.text_format(ui));
            }
            InlineNode::SoftBreak | InlineNode::HardBreak => {
                job.append("\n", 0.0, fmt.text_format(ui));
            }
        }
    }
}
