//! Renderer: Bitrix24 Full Message Profile.
//!
//! Генерирует полный документированный BBCode Bitrix24.
//! В режиме `manual_code` дополнительно оформляет код пустыми строками
//! и предупреждает о непереносимости GUI-подсветки.

use crate::diagnostics::{Diagnostic, Diagnostics};
use crate::model::{BlockNode, Document, InlineNode, is_valid_size};
use crate::parser::plain_text_of;
use crate::profiles::{LineBreakStyle, QuoteStyle, RenderOptions};

use super::RenderResult;

pub fn render(doc: &Document, opts: &RenderOptions, manual_code: bool) -> RenderResult {
    let mut r = FullRenderer { opts, manual_code, diags: Diagnostics::default() };
    let output = r.render_blocks(&doc.blocks, 0);
    RenderResult { output, diagnostics: r.diags }
}

struct FullRenderer<'a> {
    opts: &'a RenderOptions,
    manual_code: bool,
    diags: Diagnostics,
}

impl FullRenderer<'_> {
    fn br(&self) -> &'static str {
        self.opts.line_break.token()
    }

    /// Разделитель блоков — пустая строка (двойной перенос).
    fn block_sep(&self) -> String {
        format!("{}{}", self.br(), self.br())
    }

    fn render_blocks(&mut self, blocks: &[BlockNode], depth: usize) -> String {
        let parts: Vec<String> =
            blocks.iter().map(|b| self.render_block(b, depth)).filter(|s| !s.is_empty()).collect();
        parts.join(&self.block_sep())
    }

    fn render_block(&mut self, block: &BlockNode, depth: usize) -> String {
        match block {
            BlockNode::Paragraph(inlines) => self.render_inlines(inlines),
            BlockNode::Heading { level, content } => {
                let inner = format!("[b]{}[/b]", self.render_inlines(content));
                let idx = (level.saturating_sub(1) as usize).min(5);
                let px = self.opts.heading_sizes[idx];
                if px == 0 {
                    inner
                } else {
                    let px = px.clamp(crate::model::SIZE_MIN, crate::model::SIZE_MAX);
                    format!("[size={px}]{inner}[/size]")
                }
            }
            BlockNode::Quote(blocks) => self.render_quote(blocks),
            BlockNode::CodeBlock { code, .. } => {
                self.diags.push(Diagnostic::info(
                    "Блок кода выведен обычным текстом: тег [code] не входит в поддерживаемый набор.",
                ));
                code.lines().map(|line| format!("    {line}")).collect::<Vec<_>>().join(self.br())
            }
            BlockNode::List { ordered, start, items } => {
                self.render_list(*ordered, *start, items, depth)
            }
            BlockNode::Table { rows } => self.render_table(rows),
            BlockNode::Image { url, alt, .. } => self.render_image_link(url, alt),
            BlockNode::HorizontalRule => self.opts.hr_text.clone(),
        }
    }

    fn render_quote(&mut self, blocks: &[BlockNode]) -> String {
        // внутри цитаты всегда реальные переносы, т.к. `>>` работает только в начале строки
        let inner_opts = RenderOptions { line_break: LineBreakStyle::Newline, ..self.opts.clone() };
        let mut inner_renderer =
            FullRenderer { opts: &inner_opts, manual_code: self.manual_code, diags: Diagnostics::default() };
        let inner = inner_renderer.render_blocks(blocks, 0);
        self.diags.extend(inner_renderer.diags);

        match self.opts.quote_style {
            QuoteStyle::LinePrefix => inner
                .lines()
                .map(|l| format!(">>{l}"))
                .collect::<Vec<_>>()
                .join("\n"),
            QuoteStyle::FullBlock => format!("------\n{inner}\n------"),
        }
    }

    fn render_list(
        &mut self,
        ordered: bool,
        start: u64,
        items: &[Vec<BlockNode>],
        depth: usize,
    ) -> String {
        // документированная табуляция — 4 пробела
        let indent = "    ".repeat(depth);
        let mut lines: Vec<String> = Vec::new();
        for (i, item) in items.iter().enumerate() {
            let marker = if ordered {
                format!("{}. ", start + i as u64)
            } else {
                format!("{} ", self.opts.bullet_marker.as_str())
            };
            let rendered = self.render_item_blocks(item, depth);
            for (j, line) in rendered.into_iter().enumerate() {
                if j == 0 {
                    lines.push(format!("{indent}{marker}{line}"));
                } else {
                    lines.push(format!("{indent}    {line}"));
                }
            }
        }
        if self.opts.warn_on_format_loss {
            self.diags.push(Diagnostic::info(
                "Списки Markdown преобразованы в текстовые маркеры: отдельные BBCode-теги списков не документированы Bitrix24.",
            ));
        }
        lines.join(self.br())
    }

    fn render_table(&mut self, rows: &[Vec<Vec<InlineNode>>]) -> String {
        self.diags.push(Diagnostic::info(
            "Таблица Markdown выведена текстовыми строками: BBCode-тег таблицы не документирован Bitrix24.",
        ));
        rows
            .iter()
            .map(|row| {
                let cells = row.iter().map(|cell| self.render_inlines(cell)).collect::<Vec<_>>();
                format!("| {} |", cells.join(" | "))
            })
            .collect::<Vec<_>>()
            .join(self.br())
    }

    /// Рендер блоков элемента списка в набор строк.
    fn render_item_blocks(&mut self, blocks: &[BlockNode], depth: usize) -> Vec<String> {
        let mut lines = Vec::new();
        for b in blocks {
            match b {
                BlockNode::List { ordered, start, items } => {
                    let nested = self.render_list(*ordered, *start, items, depth + 1);
                    for l in nested.split(self.br()) {
                        // вложенный список уже с отступом родителя
                        lines.push(l.trim_start_matches(&"    ".repeat(depth + 1)).to_string());
                    }
                }
                other => {
                    let s = self.render_block(other, depth);
                    for l in s.split(self.br()) {
                        lines.push(l.to_string());
                    }
                }
            }
        }
        if lines.is_empty() {
            lines.push(String::new());
        }
        lines
    }

    fn render_image_link(&mut self, url: &str, alt: &str) -> String {
        self.diags.push(Diagnostic::info(
            "Изображение заменено ссылкой: тег [img] не входит в поддерживаемый набор.",
        ));
        if alt.is_empty() {
            format!("[url]{url}[/url]")
        } else {
            format!("[url={url}]{alt}[/url]")
        }
    }

    fn render_inlines(&mut self, inlines: &[InlineNode]) -> String {
        let mut out = String::new();
        for node in inlines {
            match node {
                InlineNode::Text(t) => out.push_str(t),
                InlineNode::Bold(c) => {
                    let inner = self.render_inlines(c);
                    out.push_str(&format!("[b]{inner}[/b]"));
                }
                InlineNode::Italic(c) => {
                    let inner = self.render_inlines(c);
                    out.push_str(&format!("[i]{inner}[/i]"));
                }
                InlineNode::Underline(c) => {
                    let inner = self.render_inlines(c);
                    out.push_str(&format!("[u]{inner}[/u]"));
                }
                InlineNode::Strike(c) => {
                    let inner = self.render_inlines(c);
                    out.push_str(&format!("[s]{inner}[/s]"));
                }
                InlineNode::Link { text, url } => {
                    let label = self.render_inlines(text);
                    if label.is_empty() || label == *url {
                        out.push_str(&format!("[url]{url}[/url]"));
                    } else {
                        out.push_str(&format!("[url={url}]{label}[/url]"));
                    }
                }
                InlineNode::User { id, text } => {
                    let label = self.render_inlines(text);
                    out.push_str(&format!("[user={id}]{label}[/user]"));
                }
                InlineNode::Color { hex, content } => {
                    let inner = self.render_inlines(content);
                    match crate::model::normalize_hex_color(hex) {
                        Some(h) => out.push_str(&format!("[color={h}]{inner}[/color]")),
                        None => {
                            self.diags.push(Diagnostic::warn(format!(
                                "Некорректный HEX-цвет «{hex}» пропущен при рендеринге."
                            )));
                            out.push_str(&inner);
                        }
                    }
                }
                InlineNode::Size { px, content } => {
                    let inner = self.render_inlines(content);
                    if is_valid_size(*px) {
                        out.push_str(&format!("[size={px}]{inner}[/size]"));
                    } else {
                        self.diags.push(Diagnostic::warn(format!(
                            "Значение size={px} вне диапазона 8–30 и пропущено при рендеринге."
                        )));
                        out.push_str(&inner);
                    }
                }
                InlineNode::Icon { url, params } => {
                    if params.is_empty() {
                        out.push_str(&format!("[icon={url}]"));
                    } else {
                        out.push_str(&format!("[icon={url} {params}]"));
                    }
                }
                InlineNode::Image { url, alt, .. } => {
                    let image_link = self.render_image_link(url, alt);
                    out.push_str(&image_link);
                }
                InlineNode::Code(code) => {
                    out.push_str(&format!("[color=#6b7280][b]{code}[/b][/color]"));
                }
                InlineNode::SoftBreak | InlineNode::HardBreak => out.push_str(self.br()),
            }
        }
        out
    }
}

#[allow(dead_code)]
fn alt_or_url(nodes: &[InlineNode], url: &str) -> String {
    let t = plain_text_of(nodes);
    if t.is_empty() { url.to_string() } else { t }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse_markdown;
    use crate::profiles::BulletMarker;

    fn render_md(input: &str) -> String {
        let doc = parse_markdown(input).document;
        render(&doc, &RenderOptions::default(), false).output
    }

    fn render_md_opts(input: &str, opts: &RenderOptions) -> String {
        let doc = parse_markdown(input).document;
        render(&doc, opts, false).output
    }

    #[test]
    fn bold_italic_strike() {
        assert_eq!(render_md("**b** *i* ~~s~~"), "[b]b[/b] [i]i[/i] [s]s[/s]");
    }

    #[test]
    fn underline_via_passthrough() {
        assert_eq!(render_md("[u]под[/u]"), "[u]под[/u]");
    }

    #[test]
    fn link_with_text() {
        assert_eq!(
            render_md("[текст](https://example.com)"),
            "[url=https://example.com]текст[/url]"
        );
    }

    #[test]
    fn link_text_equals_url() {
        assert_eq!(
            render_md("[https://example.com](https://example.com)"),
            "[url]https://example.com[/url]"
        );
    }

    #[test]
    fn heading_sizes_mapping() {
        assert_eq!(render_md("# H"), "[size=30][b]H[/b][/size]");
        assert_eq!(render_md("## H"), "[size=24][b]H[/b][/size]");
        assert_eq!(render_md("### H"), "[size=20][b]H[/b][/size]");
        // уровень 4 → только [b]
        assert_eq!(render_md("#### H"), "[b]H[/b]");
    }

    #[test]
    fn heading_size_clamped_to_range() {
        let mut opts = RenderOptions::default();
        opts.heading_sizes[0] = 99; // вне диапазона — должен ограничиться 30
        assert_eq!(render_md_opts("# H", &opts), "[size=30][b]H[/b][/size]");
    }

    #[test]
    fn quote_line_prefix() {
        assert_eq!(render_md("> раз\n> два"), ">>раз\n>>два");
    }

    #[test]
    fn quote_full_block() {
        let opts = RenderOptions { quote_style: QuoteStyle::FullBlock, ..Default::default() };
        assert_eq!(render_md_opts("> цитата", &opts), "------\nцитата\n------");
    }

    #[test]
    fn code_block() {
        assert_eq!(render_md("```\nlet x = 1;\n```"), "    let x = 1;");
    }

    #[test]
    fn inline_code() {
        assert_eq!(
            render_md("run `ls` now"),
            "run [color=#6b7280][b]ls[/b][/color] now"
        );
    }

    #[test]
    fn code_block_falls_back_without_code_tag() {
        let doc = parse_markdown("```rust\nfn f() {}\n```").document;
        let res = render(&doc, &RenderOptions::default(), false);
        assert!(!res.output.contains("[code]"));
        assert!(res.diagnostics.items.iter().any(|d| d.message.contains("не входит")));
    }

    #[test]
    fn image_falls_back_to_link() {
        let out = render_md("![alt](https://e.com/i.png \"large\")");
        assert_eq!(out, "[url=https://e.com/i.png]alt[/url]");
    }

    #[test]
    fn image_without_alt_falls_back_to_url_link() {
        let out = render_md("![](https://e.com/i.png)");
        assert_eq!(out, "[url]https://e.com/i.png[/url]");
    }

    #[test]
    fn icon_render() {
        assert_eq!(
            render_md("[icon=https://e.com/i.png size=16 title=Hello]"),
            "[icon=https://e.com/i.png size=16 title=Hello]"
        );
    }

    #[test]
    fn color_and_size_render() {
        assert_eq!(render_md("[color=#F00]красный[/color]"), "[color=#f00]красный[/color]");
        assert_eq!(render_md("[size=20]большой[/size]"), "[size=20]большой[/size]");
    }

    #[test]
    fn linebreak_styles() {
        assert_eq!(render_md("раз\nдва"), "раз\nдва");
    }

    #[test]
    fn paragraphs_separated_by_blank_line() {
        assert_eq!(render_md("один\n\nдва"), "один\n\nдва");
    }

    #[test]
    fn unordered_list_fallback() {
        assert_eq!(render_md("- a\n- b"), "• a\n• b");
        let opts = RenderOptions { bullet_marker: BulletMarker::Dash, ..Default::default() };
        assert_eq!(render_md_opts("- a\n- b", &opts), "- a\n- b");
    }

    #[test]
    fn ordered_list_fallback() {
        assert_eq!(render_md("1. x\n2. y"), "1. x\n2. y");
    }

    #[test]
    fn nested_list_indented_4_spaces() {
        let out = render_md("- a\n    - b");
        assert_eq!(out, "• a\n    • b");
    }

    #[test]
    fn horizontal_rule() {
        assert_eq!(render_md("---"), "--------------------");
    }

    #[test]
    fn user_render() {
        assert_eq!(render_md("[user=111]Иван Иванов[/user]"), "[user=111]Иван Иванов[/user]");
    }
}
