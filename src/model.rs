//! Внутренняя AST-модель документа.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ImageSize {
    Small,
    Medium,
    Large,
}

impl ImageSize {
    pub fn as_str(&self) -> &'static str {
        match self {
            ImageSize::Small => "small",
            ImageSize::Medium => "medium",
            ImageSize::Large => "large",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "small" => Some(ImageSize::Small),
            "medium" => Some(ImageSize::Medium),
            "large" => Some(ImageSize::Large),
            _ => None,
        }
    }

    pub const ALL: [ImageSize; 3] = [ImageSize::Small, ImageSize::Medium, ImageSize::Large];
}

#[derive(Debug, Clone, PartialEq)]
pub enum InlineNode {
    Text(String),
    Bold(Vec<InlineNode>),
    Italic(Vec<InlineNode>),
    Underline(Vec<InlineNode>),
    Strike(Vec<InlineNode>),
    Link { text: Vec<InlineNode>, url: String },
    User { id: String, text: Vec<InlineNode> },
    Color { hex: String, content: Vec<InlineNode> },
    Size { px: u8, content: Vec<InlineNode> },
    Icon { url: String, params: String },
    Image { url: String, alt: String, size: Option<ImageSize> },
    Code(String),
    SoftBreak,
    HardBreak,
}

#[derive(Debug, Clone, PartialEq)]
pub enum BlockNode {
    Paragraph(Vec<InlineNode>),
    Heading { level: u8, content: Vec<InlineNode> },
    Quote(Vec<BlockNode>),
    CodeBlock { language: Option<String>, code: String },
    List { ordered: bool, start: u64, items: Vec<Vec<BlockNode>> },
    Table { rows: Vec<Vec<Vec<InlineNode>>> },
    Image { url: String, alt: String, size: Option<ImageSize> },
    HorizontalRule,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Document {
    pub blocks: Vec<BlockNode>,
}

/// Теги, исключённые из области поддержки.
pub const EXCLUDED_TAGS: [&str; 5] = ["send", "call", "put", "context", "chat"];

/// Проверка HEX-цвета: `#` + 3 или 6 hex-символов.
#[allow(dead_code)]
pub fn is_valid_hex_color(s: &str) -> bool {
    let s = s.strip_prefix('#').unwrap_or(s);
    (s.len() == 3 || s.len() == 6) && s.chars().all(|c| c.is_ascii_hexdigit())
}

/// Нормализует HEX-цвет к виду `#abc` / `#aabbcc`.
pub fn normalize_hex_color(s: &str) -> Option<String> {
    let raw = s.trim();
    let body = raw.strip_prefix('#').unwrap_or(raw);
    if (body.len() == 3 || body.len() == 6) && body.chars().all(|c| c.is_ascii_hexdigit()) {
        Some(format!("#{}", body.to_ascii_lowercase()))
    } else {
        None
    }
}

pub const SIZE_MIN: u8 = 8;
pub const SIZE_MAX: u8 = 30;

pub fn is_valid_size(px: u8) -> bool {
    (SIZE_MIN..=SIZE_MAX).contains(&px)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_color_validation() {
        assert!(is_valid_hex_color("#fff"));
        assert!(is_valid_hex_color("#a1b2c3"));
        assert!(is_valid_hex_color("fff"));
        assert!(!is_valid_hex_color("#ffff"));
        assert!(!is_valid_hex_color("#gggggg"));
        assert!(!is_valid_hex_color(""));
    }

    #[test]
    fn hex_color_normalization() {
        assert_eq!(normalize_hex_color("FFF").as_deref(), Some("#fff"));
        assert_eq!(normalize_hex_color("#A1B2C3").as_deref(), Some("#a1b2c3"));
        assert_eq!(normalize_hex_color("bad"), Some("#bad".to_string()));
        assert_eq!(normalize_hex_color("#12345"), None);
    }

    #[test]
    fn size_range() {
        assert!(is_valid_size(8));
        assert!(is_valid_size(30));
        assert!(!is_valid_size(7));
        assert!(!is_valid_size(31));
    }

    #[test]
    fn image_size_parse() {
        assert_eq!(ImageSize::parse("Small"), Some(ImageSize::Small));
        assert_eq!(ImageSize::parse("LARGE"), Some(ImageSize::Large));
        assert_eq!(ImageSize::parse("huge"), None);
    }
}
