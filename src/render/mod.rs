//! Рендереры профилей вывода.

pub mod full;
pub mod plain;
pub mod safe;

use crate::diagnostics::Diagnostics;
use crate::model::Document;
use crate::profiles::{ProfileKind, RenderOptions};
use crate::tables::ExtractedTable;

pub struct RenderResult {
    pub output: String,
    pub diagnostics: Diagnostics,
    pub tables: Vec<ExtractedTable>,
}

pub fn render(doc: &Document, profile: ProfileKind, opts: &RenderOptions) -> RenderResult {
    match profile {
        ProfileKind::Full => full::render(doc, opts, false),
        ProfileKind::ManualCodeHighlight => full::render(doc, opts, true),
        ProfileKind::CoreSafe => safe::render(doc, opts),
        ProfileKind::PlainText => plain::render(doc, opts),
    }
}
