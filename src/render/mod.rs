//! Рендереры профилей вывода.

pub mod full;
pub mod html;
pub mod plain;
pub mod safe;

use crate::diagnostics::Diagnostics;
use crate::model::{Document, InlineNode};
use crate::profiles::{ProfileKind, RenderOptions};
use crate::tables::ExtractedTable;

pub struct RenderResult {
    pub output: String,
    pub diagnostics: Diagnostics,
    pub tables: Vec<ExtractedTable>,
}

pub(super) fn flatten_images_in_link_label(nodes: &[InlineNode]) -> Vec<InlineNode> {
    nodes
        .iter()
        .map(|node| match node {
            InlineNode::Bold(children) => {
                InlineNode::Bold(flatten_images_in_link_label(children))
            }
            InlineNode::Italic(children) => {
                InlineNode::Italic(flatten_images_in_link_label(children))
            }
            InlineNode::Underline(children) => {
                InlineNode::Underline(flatten_images_in_link_label(children))
            }
            InlineNode::Strike(children) => {
                InlineNode::Strike(flatten_images_in_link_label(children))
            }
            InlineNode::Link { text, url } => InlineNode::Link {
                text: flatten_images_in_link_label(text),
                url: url.clone(),
            },
            InlineNode::User { id, text } => InlineNode::User {
                id: id.clone(),
                text: flatten_images_in_link_label(text),
            },
            InlineNode::Color { hex, content } => InlineNode::Color {
                hex: hex.clone(),
                content: flatten_images_in_link_label(content),
            },
            InlineNode::Size { px, content } => InlineNode::Size {
                px: *px,
                content: flatten_images_in_link_label(content),
            },
            InlineNode::Image { url, alt, .. } => InlineNode::Text(if alt.is_empty() {
                url.clone()
            } else {
                alt.clone()
            }),
            other => other.clone(),
        })
        .collect()
}

pub fn render(doc: &Document, profile: ProfileKind, opts: &RenderOptions) -> RenderResult {
    match profile {
        ProfileKind::Full => full::render(doc, opts, false),
        ProfileKind::ManualCodeHighlight => full::render(doc, opts, true),
        ProfileKind::CoreSafe => safe::render(doc, opts),
        ProfileKind::PlainText => plain::render(doc, opts),
    }
}
