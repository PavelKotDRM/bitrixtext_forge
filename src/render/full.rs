//! Renderer: Bitrix24 Full Message Profile.
//!
//! Генерирует полный документированный BBCode Bitrix24.
//! В режиме `manual_code` дополнительно оформляет код пустыми строками
//! и предупреждает о непереносимости GUI-подсветки.

use crate::diagnostics::{Diagnostic, Diagnostics};
use crate::model::{BlockNode, Document, InlineNode, TableAlignment};
use crate::parser::plain_text_of;
use crate::profiles::{LineBreakStyle, RenderOptions};

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

const MAX_TABLE_COLUMN_WIDTH: usize = 36;
const SPACE_WIDTH: usize = 4;
const DASH_WIDTH: usize = 5;
const PLUS_WIDTH: usize = 9;
const PIPE_WIDTH: usize = 4;

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
            BlockNode::Heading { content, .. } => format!("[b]{}[/b]", self.render_inlines(content)),
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
            BlockNode::Table { rows, alignments } => self.render_table(rows, alignments),
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

        inner
            .lines()
            .map(|line| format!(">>{}", line.trim_start_matches('>')))
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn render_list(
        &mut self,
        ordered: bool,
        start: u64,
        items: &[Vec<BlockNode>],
        depth: usize,
    ) -> String {
        let indent = "    ".repeat(depth);
        let mut lines = Vec::new();
        for (index, item) in items.iter().enumerate() {
            let rendered = self.render_item_blocks(item, depth);
            let marker = if ordered {
                format!("{}. ", start + index as u64)
            } else {
                format!("{} ", self.opts.bullet_marker.as_str())
            };
            for (line_index, line) in rendered.iter().enumerate() {
                let prefix = if line_index == 0 { marker.as_str() } else { "    " };
                lines.push(format!("{indent}{prefix}{line}"));
            }
        }
        lines.join(self.br())
    }

    fn render_table(&mut self, rows: &[Vec<Vec<InlineNode>>], alignments: &[TableAlignment]) -> String {
        self.diags.push(Diagnostic::info(
            "Таблица Markdown выведена в контейнере [code]: теги [table], [tr] и [td] не поддерживаются Bitrix24.",
        ));
        let rendered_rows = rows
            .iter()
            .map(|row| {
                row.iter()
                    .map(|cell| plain_text_of(cell).replace(['\n', '\r'], " "))
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        let column_count = rendered_rows.iter().map(Vec::len).max().unwrap_or(0);
        let mut widths = vec![0; column_count];
        for row in &rendered_rows {
            for (column, content) in row.iter().enumerate() {
                let content_width = content.chars().count();
                let longest_word_width = content
                    .split_whitespace()
                    .map(|word| word.chars().count())
                    .max()
                    .unwrap_or(0);
                let target_width = content_width.min(MAX_TABLE_COLUMN_WIDTH).max(longest_word_width);
                widths[column] = widths[column].max(target_width);
            }
        }
        let wrapped_rows = rendered_rows
            .iter()
            .map(|row| {
                widths
                    .iter()
                    .enumerate()
                    .map(|(column, width)| match row.get(column) {
                        Some(content) => wrap_table_cell(content, *width),
                        None => vec![String::new()],
                    })
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        let mut visual_widths = vec![0; column_count];
        for row in &wrapped_rows {
            for (column, cell_lines) in row.iter().enumerate() {
                for line in cell_lines {
                    visual_widths[column] = visual_widths[column].max(estimated_text_width(line));
                }
            }
        }
        let border = format_table_border(&visual_widths);
        let mut lines = vec![border.clone()];
        for (row_index, wrapped_cells) in wrapped_rows.iter().enumerate() {
            let row_height = wrapped_cells.iter().map(Vec::len).max().unwrap_or(1);
            for line_index in 0..row_height {
                let cells = visual_widths
                    .iter()
                    .enumerate()
                    .map(|(column, width)| {
                        let content = wrapped_cells[column].get(line_index).map_or("", String::as_str);
                        format_table_cell(
                            content,
                            *width,
                            alignments.get(column).unwrap_or(&TableAlignment::Left),
                        )
                    })
                    .collect::<Vec<_>>();
                lines.push(format!("|{}|", cells.join("|")));
            }
            if row_index == 0 {
                lines.push(border.clone());
            }
        }
        lines.push(border);
        format!("[code]{}[/code]", lines.join(self.br()))
    }

    /// Рендер блоков элемента списка в набор строк.
    fn render_item_blocks(&mut self, blocks: &[BlockNode], depth: usize) -> Vec<String> {
        let mut lines = Vec::new();
        for b in blocks {
            match b {
                BlockNode::List { ordered, start, items } => {
                    let nested_indent = "    ".repeat(depth + 1);
                    lines.extend(
                        self.render_list(*ordered, *start, items, depth + 1)
                            .split(self.br())
                            .map(|line| line.trim_start_matches(&nested_indent).to_string()),
                    );
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
                InlineNode::Color { content, .. } => {
                    let inner = self.render_inlines(content);
                    self.diags.push(Diagnostic::warn("Цвет удалён: тег [color] не входит в поддерживаемый набор."));
                    out.push_str(&inner);
                }
                InlineNode::Size { content, .. } => {
                    let inner = self.render_inlines(content);
                    self.diags.push(Diagnostic::warn("Размер текста удалён: тег [size] не входит в поддерживаемый набор."));
                    out.push_str(&inner);
                }
                InlineNode::Icon { url, .. } => {
                    self.diags.push(Diagnostic::warn("Иконка заменена URL: тег [icon] не входит в поддерживаемый набор."));
                    out.push_str(url);
                }
                InlineNode::Image { url, alt, .. } => {
                    let image_link = self.render_image_link(url, alt);
                    out.push_str(&image_link);
                }
                InlineNode::Code(code) => {
                    out.push_str(&format!("[b]{code}[/b]"));
                }
                InlineNode::SoftBreak | InlineNode::HardBreak => out.push_str(self.br()),
            }
        }
        out
    }
}

fn wrap_table_cell(content: &str, width: usize) -> Vec<String> {
    if content.chars().count() <= width {
        return vec![content.to_string()];
    }

    let mut lines = Vec::new();
    let mut line = String::new();
    for word in content.split_whitespace() {
        if !line.is_empty() && line.chars().count() + 1 + word.chars().count() > width {
            lines.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    if !line.is_empty() {
        lines.push(line);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

fn estimated_text_width(text: &str) -> usize {
    text.chars()
        .map(|character| match character {
            ' ' => SPACE_WIDTH,
            'i' | 'j' | 'l' | 't' | 'I' | '1' | '.' | ',' | ':' | ';' | '!' | '\'' | '|' => 3,
            'm' | 'w' | 'M' | 'W' | '@' | '%' | '&' => 10,
            '-' => DASH_WIDTH,
            '+' => PLUS_WIDTH,
            '0'..='9' => 7,
            'A'..='Z' | 'А'..='Я' | 'Ё' => 8,
            'a'..='z' | 'а'..='я' | 'ё' => 7,
            _ => 8,
        })
        .sum()
}

fn format_table_border(widths: &[usize]) -> String {
    let segments = widths
        .iter()
        .map(|width| {
            let row_segment_width = PIPE_WIDTH + 2 * SPACE_WIDTH + width;
            "-".repeat(row_segment_width.div_ceil(DASH_WIDTH).max(1))
        })
        .collect::<Vec<_>>();
    format!("+{}+", segments.join("+"))
}

fn format_table_cell(content: &str, width: usize, alignment: &TableAlignment) -> String {
    let padding_width = width.saturating_sub(estimated_text_width(content));
    let padding_spaces = padding_width.div_ceil(SPACE_WIDTH);
    let (left_padding, right_padding) = match alignment {
        TableAlignment::Left => (0, padding_spaces),
        TableAlignment::Center => (padding_spaces / 2, padding_spaces - padding_spaces / 2),
        TableAlignment::Right => (padding_spaces, 0),
    };
    format!(" {}{content}{} ", " ".repeat(left_padding), " ".repeat(right_padding))
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
    use crate::profiles::{BulletMarker, QuoteStyle};

    #[test]
    fn table_border_segments_cover_their_cell_widths() {
        let widths = [16, 31];
        let border = format_table_border(&widths);

        for (segment, width) in border.trim_matches('+').split('+').zip(widths) {
            let row_segment_width = PIPE_WIDTH + 2 * SPACE_WIDTH + width;
            assert!(estimated_text_width(segment) >= row_segment_width);
        }
    }

    #[test]
    fn table_cells_use_the_requested_alignment() {
        assert_eq!(format_table_cell("x", 12, &TableAlignment::Left), " x   ");
        assert_eq!(format_table_cell("x", 12, &TableAlignment::Center), "  x  ");
        assert_eq!(format_table_cell("x", 12, &TableAlignment::Right), "   x ");
    }

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
    fn headings_use_bold_only() {
        assert_eq!(render_md("# H"), "[b]H[/b]");
        assert_eq!(render_md("## H"), "[b]H[/b]");
        assert_eq!(render_md("### H"), "[b]H[/b]");
        assert_eq!(render_md("#### H"), "[b]H[/b]");
    }

    #[test]
    fn heading_size_setting_is_not_rendered() {
        let mut opts = RenderOptions::default();
        opts.heading_sizes[0] = 99;
        assert_eq!(render_md_opts("# H", &opts), "[b]H[/b]");
    }

    #[test]
    fn quote_line_prefix() {
        assert_eq!(render_md("> раз\n> два"), ">>раз\n>>два");
    }

    #[test]
    fn nested_quotes_use_a_single_bitrix_prefix() {
        assert_eq!(render_md("> > > вложенная цитата"), ">>вложенная цитата");
    }

    #[test]
    fn quote_always_uses_line_prefix() {
        let opts = RenderOptions { quote_style: QuoteStyle::FullBlock, ..Default::default() };
        assert_eq!(render_md_opts("> цитата", &opts), ">>цитата");
    }

    #[test]
    fn code_block() {
        assert_eq!(render_md("```\nlet x = 1;\n```"), "    let x = 1;");
    }

    #[test]
    fn inline_code_uses_bold_only() {
        assert_eq!(render_md("run `ls` now"), "run [b]ls[/b] now");
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
        assert_eq!(render_md("[icon=https://e.com/i.png size=16 title=Hello]"), "https://e.com/i.png");
    }

    #[test]
    fn color_and_size_are_removed() {
        assert_eq!(render_md("[color=#F00]красный[/color]"), "красный");
        assert_eq!(render_md("[size=20]большой[/size]"), "большой");
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
    fn unordered_list_uses_text_markers() {
        assert_eq!(render_md("- a\n- b"), "• a\n• b");
        let opts = RenderOptions { bullet_marker: BulletMarker::Dash, ..Default::default() };
        assert_eq!(render_md_opts("- a\n- b", &opts), "- a\n- b");
    }

    #[test]
    fn ordered_list_uses_text_markers() {
        assert_eq!(render_md("1. x\n2. y"), "1. x\n2. y");
    }

    #[test]
    fn nested_list_uses_indentation() {
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
