//! Renderer: Bitrix24 Core Safe Profile.
//!
//! Ограниченный набор конструкций: `[b]`, `[i]`, `[u]`, `[s]`, `[url]`,
//! `[url=URL]`, `[br]`/`\n`; `[code]` — только при явном включении.
//! Медиа и служебные элементы заменяются безопасным текстом с предупреждением.

use crate::diagnostics::{Diagnostic, Diagnostics};
use crate::highlight::escape_bbcode_tags;
use crate::model::{BlockNode, Document, InlineNode, TableAlignment};
use crate::parser::plain_text_of;
use crate::profiles::RenderOptions;
use crate::tables::{ExtractedTable, table_file_name};

use super::{RenderResult, flatten_images_in_link_label};

const CORE_SAFE_ALLOWED_TAGS: &[&str] = &["b", "i", "u", "s", "url"];

pub fn render(doc: &Document, opts: &RenderOptions) -> RenderResult {
    let mut r = SafeRenderer { opts, diags: Diagnostics::default(), tables: Vec::new() };
    let output = r.render_blocks(&doc.blocks, 0);
    RenderResult { output, diagnostics: r.diags, tables: r.tables }
}

struct SafeRenderer<'a> {
    opts: &'a RenderOptions,
    diags: Diagnostics,
    tables: Vec<ExtractedTable>,
}

impl SafeRenderer<'_> {
    fn br(&self) -> &'static str {
        self.opts.line_break.token()
    }

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
            BlockNode::Heading { content, .. } => {
                // в безопасном профиле заголовки — только [b]
                format!("[b]{}[/b]", self.render_inlines(content))
            }
            BlockNode::Quote(blocks) => {
                let inner = self.render_blocks(blocks, depth);
                inner
                    .split(self.br())
                    .map(|l| format!("> {l}"))
                    .collect::<Vec<_>>()
                    .join(self.br())
            }
            BlockNode::CodeBlock { code, .. } => {
                if self.opts.safe_allow_code {
                    self.diags.push(Diagnostic::info(
                        "Блок кода обёрнут в [code]: Bitrix24 не подсвечивает синтаксис внутри тега.",
                    ));
                    format!("[code]{sep}{code}{sep}[/code]", sep = self.br())
                } else {
                    self.diags.push(Diagnostic::info(
                        "Код оформлен отступом в 4 пробела: тег [code] отключён в Core Safe Profile (см. настройки).",
                    ));
                    let mut escaped = false;
                    let output = code
                        .lines()
                        .map(|line| {
                            let (line, changed) = escape_bbcode_tags(line, &[]);
                            escaped |= changed;
                            format!("    {line}")
                        })
                        .collect::<Vec<_>>()
                        .join(self.br());
                    if escaped {
                        self.diags.push(Diagnostic::warn(
                            "BBCode-подобные последовательности в коде экранированы полноширинными скобками.",
                        ));
                    }
                    output
                }
            }
            BlockNode::List { ordered, start, items } => {
                let indent = "    ".repeat(depth);
                let nested_indent = "    ".repeat(depth + 1);
                let mut lines = Vec::new();
                for (i, item) in items.iter().enumerate() {
                    let marker = if *ordered {
                        format!("{}. ", start + i as u64)
                    } else {
                        format!("{} ", self.opts.bullet_marker.as_str())
                    };
                    let inner = self.render_item_blocks(item, depth);
                    for (j, line) in inner.iter().enumerate() {
                        if j > 0 && line.starts_with(&nested_indent) {
                            lines.push(line.to_string());
                        } else if j == 0 {
                            lines.push(format!("{indent}{marker}{line}"));
                        } else {
                            lines.push(format!("{indent}    {line}"));
                        }
                    }
                }
                lines.join(self.br())
            }
            BlockNode::Table { rows, alignments } => self.render_table(rows, alignments),
            BlockNode::Image { url, alt, .. } => {
                self.warn_loss("Изображение заменено текстовой ссылкой (Core Safe Profile).");
                if alt.is_empty() {
                    format!("[url]{url}[/url]")
                } else {
                    format!("[url={url}]{alt}[/url]")
                }
            }
            BlockNode::HorizontalRule => self.opts.hr_text.clone(),
        }
    }

    fn render_item_blocks(&mut self, blocks: &[BlockNode], depth: usize) -> Vec<String> {
        let mut lines = Vec::new();
        for block in blocks {
            match block {
                BlockNode::List { ordered, start, items } => lines.extend(
                    self.render_block(
                        &BlockNode::List { ordered: *ordered, start: *start, items: items.clone() },
                        depth + 1,
                    )
                    .split(self.br())
                    .map(str::to_string),
                ),
                other => lines.extend(
                    self.render_block(other, depth).split(self.br()).map(str::to_string),
                ),
            }
        }
        if lines.is_empty() {
            lines.push(String::new());
        }
        lines
    }

    fn warn_loss(&mut self, msg: &str) {
        if self.opts.warn_on_format_loss {
            self.diags.push(Diagnostic::warn(msg.to_string()));
        }
    }

    fn render_table(&mut self, rows: &[Vec<Vec<InlineNode>>], alignments: &[TableAlignment]) -> String {
        let name = table_file_name(self.tables.len());
        self.warn_loss(&format!(
            "Таблица Markdown подготовлена для отдельного файла Excel «{name}»: \
             для выбора каталога используйте кнопку «📦 Ресурсы»."
        ));
        let rendered_rows = rows
            .iter()
            .map(|row| row.iter().map(|cell| plain_text_of(cell)).collect())
            .collect();
        self.tables.push(ExtractedTable { rows: rendered_rows, alignments: alignments.to_vec() });
        format!("Таблица: {name}")
    }

    fn render_inlines(&mut self, inlines: &[InlineNode]) -> String {
        let mut out = String::new();
        for node in inlines {
            match node {
                InlineNode::Text(t) => {
                    let (text, escaped) = escape_bbcode_tags(t, CORE_SAFE_ALLOWED_TAGS);
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
                InlineNode::User { text, .. } => {
                    self.warn_loss("Тег [user] не входит в Core Safe Profile: упоминание заменено именем.");
                    out.push_str(&self.render_inlines_owned(text));
                }
                InlineNode::Color { content, .. } => {
                    self.warn_loss("Тег [color] не входит в Core Safe Profile: цвет удалён.");
                    out.push_str(&self.render_inlines_owned(content));
                }
                InlineNode::Size { content, .. } => {
                    self.warn_loss("Тег [size] не входит в Core Safe Profile: размер удалён.");
                    out.push_str(&self.render_inlines_owned(content));
                }
                InlineNode::Icon { url, .. } => {
                    self.warn_loss("Тег [icon] не входит в Core Safe Profile: подставлен URL.");
                    out.push_str(url);
                }
                InlineNode::Image { url, alt, .. } => {
                    self.warn_loss("Изображение заменено текстовой ссылкой (Core Safe Profile).");
                    if alt.is_empty() {
                        out.push_str(&format!("[url]{url}[/url]"));
                    } else {
                        out.push_str(&format!("[url={url}]{alt}[/url]"));
                    }
                }
                InlineNode::Code(code) => {
                    self.warn_loss(
                        "Inline code Markdown выведен обычным текстом: Bitrix24 поддерживает [code] только как блочный элемент.",
                    );
                    let (code, escaped) = escape_bbcode_tags(code, &[]);
                    if escaped {
                        self.diags.push(Diagnostic::warn(
                            "BBCode-подобные последовательности в inline-коде экранированы полноширинными скобками.",
                        ));
                    }
                    out.push_str(&code);
                }
                InlineNode::SoftBreak | InlineNode::HardBreak => out.push_str(self.br()),
            }
        }
        out
    }

    fn render_inlines_owned(&mut self, inlines: &[InlineNode]) -> String {
        self.render_inlines(inlines)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse_markdown;

    fn render_md(input: &str) -> String {
        let doc = parse_markdown(input).document;
        render(&doc, &RenderOptions::default()).output
    }

    fn render_md_res(input: &str, opts: &RenderOptions) -> super::RenderResult {
        let doc = parse_markdown(input).document;
        render(&doc, opts)
    }

    #[test]
    fn basic_formatting_allowed() {
        assert_eq!(render_md("**b** *i* ~~s~~ [u]u[/u]"), "[b]b[/b] [i]i[/i] [s]s[/s] [u]u[/u]");
    }

    #[test]
    fn links_allowed() {
        assert_eq!(render_md("[t](https://e.com)"), "[url=https://e.com]t[/url]");
    }

    #[test]
    fn color_and_size_stripped_with_warning() {
        let res = render_md_res("[color=#f00]x[/color][size=20]y[/size]", &RenderOptions::default());
        assert_eq!(res.output, "xy");
        assert!(res.diagnostics.warnings() >= 2);
    }

    #[test]
    fn heading_is_bold_only() {
        assert_eq!(render_md("# Заголовок"), "[b]Заголовок[/b]");
    }

    #[test]
    fn inline_code_is_not_rendered_as_a_code_block() {
        let opts = RenderOptions { safe_allow_code: true, ..Default::default() };
        assert_eq!(render_md_res("run `ls` now", &opts).output, "run ls now");
    }

    #[test]
    fn code_disabled_by_default() {
        let out = render_md("```\nlet x = 1;\n```");
        assert_eq!(out, "    let x = 1;");
        assert!(!out.contains("[code]"));
    }

    #[test]
    fn code_allowed_renders_as_code_tag_when_enabled() {
        let opts = RenderOptions { safe_allow_code: true, ..Default::default() };
        let res = render_md_res("```\nlet x = 1;\n```", &opts);
        assert_eq!(res.output, "[code]\nlet x = 1;\n[/code]");
    }

    #[test]
    fn image_falls_back_to_url() {
        let res = render_md_res("![альт](https://e.com/i.png)", &RenderOptions::default());
        assert_eq!(res.output, "[url=https://e.com/i.png]альт[/url]");
        assert!(res.diagnostics.warnings() >= 1);
    }

    #[test]
    fn linked_image_uses_the_outer_link_without_nesting_urls() {
        let res = render_md_res(
            "[![alt](https://e.com/i.png)](https://e.com/page)",
            &RenderOptions::default(),
        );
        assert_eq!(res.output, "[url=https://e.com/page]alt[/url]");
    }

    #[test]
    fn unsupported_tags_remain_plain_text() {
        let res = render_md_res("[timestamp=1700000000] [disk=5]", &RenderOptions::default());
        assert_eq!(res.output, "［timestamp=1700000000］ ［disk=5］");
    }

    #[test]
    fn excluded_tags_are_escaped_even_when_wrapping_markdown() {
        let res = render_md_res("[send=1]**name**[/send]", &RenderOptions::default());

        assert_eq!(res.output, "［send=1］[b]name[/b]［/send］");
        assert!(res.diagnostics.warnings() > 0);
    }
}
