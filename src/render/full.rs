//! Renderer: Bitrix24 Full Message Profile.
//!
//! Генерирует полный документированный BBCode Bitrix24.
//! В режиме `manual_code` код визуально выделяется префиксом `>>` и, при
//! включённой опции `manual_code_colors`, дополнительно окрашивается через
//! `[color]`-токены (см. `highlight::highlight_code_to_bbcode`).

use crate::diagnostics::{Diagnostic, Diagnostics};
use crate::highlight::{code_has_bbcode_like_tokens, escape_bbcode_tags, highlight_code_to_bbcode};
use crate::model::{BlockNode, Document, InlineNode, TableAlignment};
use crate::parser::plain_text_of;
use crate::profiles::{LineBreakStyle, RenderOptions};
use crate::tables::{ExtractedTable, table_file_name};

use super::{RenderResult, flatten_images_in_link_label};

const FULL_ALLOWED_TAGS: &[&str] =
    &["b", "i", "u", "s", "url", "user", "color", "size", "icon", "code"];

pub fn render(doc: &Document, opts: &RenderOptions, manual_code: bool) -> RenderResult {
    let mut r = FullRenderer { opts, manual_code, diags: Diagnostics::default(), tables: Vec::new() };
    let output = r.render_blocks(&doc.blocks, 0);
    RenderResult { output, diagnostics: r.diags, tables: r.tables }
}

struct FullRenderer<'a> {
    opts: &'a RenderOptions,
    manual_code: bool,
    diags: Diagnostics,
    tables: Vec<ExtractedTable>,
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
            BlockNode::Heading { level, content } => self.render_heading(*level, content),
            BlockNode::Quote(blocks) => self.render_quote(blocks),
            BlockNode::CodeBlock { language, code } => self.render_code_block(language.as_deref(), code),
            BlockNode::List { ordered, start, items } => {
                self.render_list(*ordered, *start, items, depth)
            }
            BlockNode::Table { rows, alignments } => self.render_table(rows, alignments),
            BlockNode::Image { url, alt, .. } => self.render_image_link(url, alt),
            BlockNode::HorizontalRule => self.opts.hr_text.clone(),
        }
    }

    /// Заголовок: `[b]...[/b]`, обёрнутый в `[size=N]`, если для этого уровня задан размер
    /// (`opts.heading_sizes`); `0` (по умолчанию для H4-H6) означает только жирный текст.
    fn render_heading(&mut self, level: u8, content: &[InlineNode]) -> String {
        let text = self.render_inlines(content);
        let size = self.opts.heading_sizes.get(level.saturating_sub(1) as usize).copied().unwrap_or(0);
        if size > 0 {
            format!("[size={size}][b]{text}[/b][/size]")
        } else {
            format!("[b]{text}[/b]")
        }
    }

    fn render_quote(&mut self, blocks: &[BlockNode]) -> String {
        // внутри цитаты всегда реальные переносы, т.к. `>>` работает только в начале строки
        let inner_opts = RenderOptions { line_break: LineBreakStyle::Newline, ..self.opts.clone() };
        let mut inner_renderer = FullRenderer {
            opts: &inner_opts,
            manual_code: self.manual_code,
            diags: Diagnostics::default(),
            tables: std::mem::take(&mut self.tables),
        };
        let inner = inner_renderer.render_blocks(blocks, 0);
        self.diags.extend(inner_renderer.diags);
        self.tables = inner_renderer.tables;

        match self.opts.quote_style {
            crate::profiles::QuoteStyle::LinePrefix => inner
                .lines()
                .map(|line| format!(">>{}", line.trim_start_matches('>')))
                .collect::<Vec<_>>()
                .join("\n"),
            crate::profiles::QuoteStyle::FullBlock if !inner.is_empty() => {
                format!("------\n{inner}\n------")
            }
            crate::profiles::QuoteStyle::FullBlock => String::new(),
        }
    }

    /// Оформление блока кода. Вне ручного режима — документированный `[code]...[/code]`
    /// (без подсветки синтаксиса, Bitrix24 её внутри тега не поддерживает).
    /// В `manual_code` код вместо этого визуально выделяется префиксом `>>` (как цитата),
    /// а при включённой опции `manual_code_colors` токены дополнительно оборачиваются
    /// в `[color]` (GUI-only приближение подсветки, без гарантии в Bitrix24).
    fn render_code_block(&mut self, language: Option<&str>, code: &str) -> String {
        if !self.manual_code {
            self.diags.push(Diagnostic::info(
                "Блок кода обёрнут в [code]: Bitrix24 не подсвечивает синтаксис внутри тега.",
            ));
            return format!("[code]{sep}{code}{sep}[/code]", sep = self.br());
        }
        self.diags.push(Diagnostic::info(
            "Код визуально выделен префиксом «>>»: подсветка синтаксиса не гарантируется Bitrix24.",
        ));
        if code_has_bbcode_like_tokens(code) {
            self.diags.push(Diagnostic::warn(
                "BBCode-подобные последовательности в коде экранированы полноширинными скобками.",
            ));
        }
        if self.opts.manual_code_colors {
            highlight_code_to_bbcode(language.unwrap_or(""), code)
        } else {
            code.lines()
                .map(|line| {
                    if line.is_empty() {
                        ">>".to_string()
                    } else {
                        let (line, _) = escape_bbcode_tags(line, &[]);
                        format!(">>    {line}")
                    }
                })
                .collect::<Vec<_>>()
                .join("\n")
        }
    }

    fn render_list(
        &mut self,
        ordered: bool,
        start: u64,
        items: &[Vec<BlockNode>],
        depth: usize,
    ) -> String {
        let indent = "    ".repeat(depth);
        let nested_indent = "    ".repeat(depth + 1);
        let mut lines = Vec::new();
        for (index, item) in items.iter().enumerate() {
            let rendered = self.render_item_blocks(item, depth);
            let marker = if ordered {
                format!("{}. ", start + index as u64)
            } else {
                format!("{} ", self.opts.bullet_marker.as_str())
            };
            for (line_index, line) in rendered.iter().enumerate() {
                if line_index > 0 && line.starts_with(&nested_indent) {
                    lines.push(line.to_string());
                } else {
                    let prefix = if line_index == 0 { marker.as_str() } else { "    " };
                    lines.push(format!("{indent}{prefix}{line}"));
                }
            }
        }
        lines.join(self.br())
    }

    fn render_table(&mut self, rows: &[Vec<Vec<InlineNode>>], alignments: &[TableAlignment]) -> String {
        let name = table_file_name(self.tables.len());
        self.diags.push(Diagnostic::info(format!(
            "Таблица Markdown подготовлена для отдельного файла Excel «{name}»: \
             для выбора каталога используйте кнопку «📦 Ресурсы»."
        )));
        let rendered_rows = rows
            .iter()
            .map(|row| row.iter().map(|cell| plain_text_of(cell).replace(['\n', '\r'], " ")).collect())
            .collect();
        self.tables.push(ExtractedTable { rows: rendered_rows, alignments: alignments.to_vec() });
        format!("[b]Таблица:[/b] {name}")
    }

    /// Рендер блоков элемента списка в набор строк.
    fn render_item_blocks(&mut self, blocks: &[BlockNode], depth: usize) -> Vec<String> {
        let mut lines = Vec::new();
        for b in blocks {
            match b {
                BlockNode::List { ordered, start, items } => {
                    lines.extend(
                        self.render_list(*ordered, *start, items, depth + 1)
                            .split(self.br())
                            .map(str::to_string),
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
                InlineNode::Text(t) => {
                    let (text, escaped) = escape_bbcode_tags(t, FULL_ALLOWED_TAGS);
                    if escaped {
                        self.diags.push(Diagnostic::warn(
                            "BBCode-теги вне профиля экранированы полноширинными скобками.",
                        ));
                    }
                    out.push_str(&text);
                }
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
                    let label_nodes = flatten_images_in_link_label(text);
                    let label = self.render_inlines(&label_nodes);
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
                    out.push_str(&format!("[color={hex}]{inner}[/color]"));
                }
                InlineNode::Size { px, content } => {
                    let inner = self.render_inlines(content);
                    out.push_str(&format!("[size={px}]{inner}[/size]"));
                }
                InlineNode::Icon { url, params } => {
                    let params = if params.is_empty() {
                        String::new()
                    } else {
                        format!(" {params}")
                    };
                    out.push_str(&format!("[icon={url}{params}]"));
                }
                InlineNode::Image { url, alt, .. } => {
                    let image_link = self.render_image_link(url, alt);
                    out.push_str(&image_link);
                }
                InlineNode::Code(code) => {
                    let (code, escaped) = escape_bbcode_tags(code, &[]);
                    if escaped {
                        self.diags.push(Diagnostic::warn(
                            "BBCode-подобные последовательности в inline-коде экранированы полноширинными скобками.",
                        ));
                    }
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
    use crate::profiles::{BulletMarker, QuoteStyle};

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
    fn headings_use_configured_size_by_default() {
        // дефолтные heading_sizes = [30, 24, 20, 0, 0, 0]
        assert_eq!(render_md("# H"), "[size=30][b]H[/b][/size]");
        assert_eq!(render_md("## H"), "[size=24][b]H[/b][/size]");
        assert_eq!(render_md("### H"), "[size=20][b]H[/b][/size]");
        assert_eq!(render_md("#### H"), "[b]H[/b]");
    }

    #[test]
    fn heading_size_setting_is_rendered() {
        let mut opts = RenderOptions::default();
        opts.heading_sizes[0] = 99;
        assert_eq!(render_md_opts("# H", &opts), "[size=99][b]H[/b][/size]");
    }

    #[test]
    fn heading_size_zero_is_bold_only() {
        let mut opts = RenderOptions::default();
        opts.heading_sizes[0] = 0;
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
    fn quote_style_full_block_wraps_the_quote() {
        let opts = RenderOptions { quote_style: QuoteStyle::FullBlock, ..Default::default() };
        assert_eq!(render_md_opts("> цитата", &opts), "------\nцитата\n------");
    }

    #[test]
    fn code_block() {
        assert_eq!(render_md("```\nlet x = 1;\n```"), "[code]\nlet x = 1;\n[/code]");
    }

    #[test]
    fn inline_code_uses_configured_color_and_bold() {
        assert_eq!(
            render_md("run `ls` now"),
            "run [color=#6b7280][b]ls[/b][/color] now"
        );
    }

    #[test]
    fn code_block_uses_code_tag_without_syntax_highlighting() {
        let doc = parse_markdown("```rust\nfn f() {}\n```").document;
        let res = render(&doc, &RenderOptions::default(), false);
        assert_eq!(res.output, "[code]\nfn f() {}\n[/code]");
        assert!(res.diagnostics.items.iter().any(|d| d.message.contains("не подсвечивает синтаксис")));
    }

    #[test]
    fn manual_code_highlight_uses_quote_prefix_and_colors() {
        let doc = parse_markdown("```csharp\npublic class Foo {}\n```").document;
        let res = render(&doc, &RenderOptions::default(), true);
        assert!(!res.output.contains("[code]"));
        assert!(res.output.lines().all(|l| l.starts_with(">>")), "код без >>: {}", res.output);
        assert!(res.output.contains("[color=#c678dd]public[/color]"));
        assert!(res.output.contains("[color=#c678dd]class[/color]"));
    }

    #[test]
    fn manual_code_highlight_without_colors_still_uses_quote_prefix() {
        let doc = parse_markdown("```csharp\npublic class Foo {}\n```").document;
        let opts = RenderOptions { manual_code_colors: false, ..Default::default() };
        let res = render(&doc, &opts, true);
        assert!(!res.output.contains("[color="));
        assert_eq!(res.output, ">>    public class Foo {}");
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
    fn linked_image_uses_the_outer_link_without_nesting_urls() {
        let out = render_md("[![alt](https://e.com/i.png)](https://e.com/page)");
        assert_eq!(out, "[url=https://e.com/page]alt[/url]");
    }

    #[test]
    fn icon_render() {
        assert_eq!(
            render_md("[icon=https://e.com/i.png size=16 title=Hello]"),
            "[icon=https://e.com/i.png size=16 title=Hello]"
        );
    }

    #[test]
    fn color_and_size_are_preserved() {
        assert_eq!(
            render_md("[color=#F00]красный[/color]"),
            "[color=#f00]красный[/color]"
        );
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

    #[test]
    fn excluded_bbcode_tags_are_escaped_before_paste() {
        let doc = parse_markdown("[send=1]name[/send]").document;
        let result = render(&doc, &RenderOptions::default(), false);

        assert_eq!(result.output, "［send=1］name［/send］");
        assert!(result.diagnostics.warnings() > 0);
    }

    #[test]
    fn supported_special_tags_inside_markdown_formatting_stay_active() {
        assert_eq!(render_md("[u]**bold**[/u]"), "[u][b]bold[/b][/u]");
    }

    #[test]
    fn manual_code_highlight_escapes_bbcode_like_source() {
        let doc = parse_markdown("```rust\n[b]literal[/b]\n```").document;
        let result = render(&doc, &RenderOptions::default(), true);

        assert!(result.output.contains("［b］literal［/b］"));
        assert!(result.diagnostics.items.iter().any(|diagnostic| {
            diagnostic.message.contains("экранированы полноширинными скобками")
        }));
    }
}
