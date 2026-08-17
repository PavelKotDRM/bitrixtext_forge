//! Renderer: Plain Text Profile — текст без BBCode со структурой.

use crate::diagnostics::Diagnostics;
use crate::model::{BlockNode, Document, InlineNode};
use crate::profiles::RenderOptions;

use super::RenderResult;

pub fn render(doc: &Document, opts: &RenderOptions) -> RenderResult {
    let mut r = PlainRenderer { opts, diags: Diagnostics::default() };
    let output = r.render_blocks(&doc.blocks, 0);
    RenderResult { output, diagnostics: r.diags }
}

struct PlainRenderer<'a> {
    opts: &'a RenderOptions,
    #[allow(dead_code)]
    diags: Diagnostics,
}

impl PlainRenderer<'_> {
    fn render_blocks(&mut self, blocks: &[BlockNode], depth: usize) -> String {
        let parts: Vec<String> =
            blocks.iter().map(|b| self.render_block(b, depth)).filter(|s| !s.is_empty()).collect();
        parts.join("\n\n")
    }

    fn render_block(&mut self, block: &BlockNode, depth: usize) -> String {
        match block {
            BlockNode::Paragraph(inlines) => self.render_inlines(inlines),
            BlockNode::Heading { content, .. } => self.render_inlines(content),
            BlockNode::Quote(blocks) => {
                let inner = self.render_blocks(blocks, depth);
                inner.lines().map(|l| format!("> {l}")).collect::<Vec<_>>().join("\n")
            }
            BlockNode::CodeBlock { code, .. } => {
                code.lines().map(|l| format!("    {l}")).collect::<Vec<_>>().join("\n")
            }
            BlockNode::List { ordered, start, items } => {
                let indent = "    ".repeat(depth);
                let mut lines = Vec::new();
                for (i, item) in items.iter().enumerate() {
                    let marker = if *ordered {
                        format!("{}. ", start + i as u64)
                    } else {
                        format!("{} ", self.opts.bullet_marker.as_str())
                    };
                    let inner = self.render_blocks(item, depth + 1);
                    for (j, line) in inner.lines().enumerate() {
                        if j == 0 {
                            lines.push(format!("{indent}{marker}{line}"));
                        } else {
                            lines.push(format!("{indent}    {line}"));
                        }
                    }
                }
                lines.join("\n")
            }
            BlockNode::Table { rows } => rows
                .iter()
                .map(|row| {
                    let cells = row.iter().map(|cell| self.render_inlines(cell)).collect::<Vec<_>>();
                    format!("| {} |", cells.join(" | "))
                })
                .collect::<Vec<_>>()
                .join("\n"),
            BlockNode::Image { url, alt, .. } => {
                if alt.is_empty() {
                    url.clone()
                } else {
                    format!("{alt}: {url}")
                }
            }
            BlockNode::HorizontalRule => self.opts.hr_text.clone(),
        }
    }

    fn render_inlines(&mut self, inlines: &[InlineNode]) -> String {
        let mut out = String::new();
        for node in inlines {
            match node {
                InlineNode::Text(t) | InlineNode::Code(t) => out.push_str(t),
                InlineNode::Bold(c)
                | InlineNode::Italic(c)
                | InlineNode::Underline(c)
                | InlineNode::Strike(c) => out.push_str(&self.render_inlines(c)),
                InlineNode::Link { text, url } => {
                    let label = self.render_inlines(text);
                    if label.is_empty() || label == *url {
                        out.push_str(url);
                    } else {
                        out.push_str(&format!("{label} ({url})"));
                    }
                }
                InlineNode::User { text, .. } => out.push_str(&self.render_inlines(text)),
                InlineNode::Color { content, .. } | InlineNode::Size { content, .. } => {
                    out.push_str(&self.render_inlines(content));
                }
                InlineNode::Icon { url, .. } => out.push_str(url),
                InlineNode::Image { url, alt, .. } => {
                    if alt.is_empty() {
                        out.push_str(url);
                    } else {
                        out.push_str(&format!("{alt}: {url}"));
                    }
                }
                InlineNode::SoftBreak | InlineNode::HardBreak => out.push('\n'),
            }
        }
        out
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

    #[test]
    fn no_bbcode_in_output() {
        let out = render_md("# H\n\n**b** *i* [t](https://e.com)\n\n```\ncode\n```");
        assert!(!out.contains("[b]"));
        assert!(!out.contains("[size"));
        assert!(!out.contains("[code]"));
        assert!(!out.contains("[url"));
    }

    #[test]
    fn link_readable() {
        assert_eq!(render_md("[текст](https://e.com)"), "текст (https://e.com)");
    }

    #[test]
    fn quote_prefixed() {
        assert_eq!(render_md("> цитата"), "> цитата");
    }

    #[test]
    fn code_indented() {
        assert_eq!(render_md("```\nlet a = 1;\n```"), "    let a = 1;");
    }
}
