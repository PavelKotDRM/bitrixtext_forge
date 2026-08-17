//! Профили вывода и параметры рендеринга.

use serde::{Deserialize, Serialize};

use crate::model::ImageSize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProfileKind {
    /// Bitrix24 Full Message Profile — полный документированный профиль.
    Full,
    /// Bitrix24 Core Safe Profile — безопасный базовый профиль.
    CoreSafe,
    /// Plain Text Profile — текст без BBCode.
    PlainText,
    /// Manual Code Highlight Profile — как Full, но с ручным оформлением кода
    /// и предупреждениями о непереносимости GUI-подсветки.
    ManualCodeHighlight,
}

impl ProfileKind {
    pub fn label(&self) -> &'static str {
        match self {
            ProfileKind::Full => "Bitrix24 Full Message",
            ProfileKind::CoreSafe => "Bitrix24 Core Safe",
            ProfileKind::PlainText => "Plain Text",
            ProfileKind::ManualCodeHighlight => "Manual Code Highlight",
        }
    }

    pub const ALL: [ProfileKind; 4] = [
        ProfileKind::Full,
        ProfileKind::CoreSafe,
        ProfileKind::PlainText,
        ProfileKind::ManualCodeHighlight,
    ];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LineBreakStyle {
    /// Реальный перенос строки `\n`.
    Newline,
}

impl LineBreakStyle {
    pub fn token(&self) -> &'static str {
        match self {
            LineBreakStyle::Newline => "\n",
        }
    }
    #[allow(dead_code)]
    pub fn label(&self) -> &'static str {
        match self {
            LineBreakStyle::Newline => "\\n (перенос строки)",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum QuoteStyle {
    /// Каждая строка цитаты с префиксом `>>` (работает в начале строки).
    LinePrefix,
    /// Полная цитата сообщения: `------ ... ------`.
    FullBlock,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BulletMarker {
    Bullet,
    Dash,
}

impl BulletMarker {
    pub fn as_str(&self) -> &'static str {
        match self {
            BulletMarker::Bullet => "•",
            BulletMarker::Dash => "-",
        }
    }
}

/// Параметры рендеринга, формируемые из настроек приложения.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenderOptions {
    pub line_break: LineBreakStyle,
    /// Размеры `[size]` для уровней заголовков 1–6; `0` = только `[b]`.
    pub heading_sizes: [u8; 6],
    pub default_image_size: ImageSize,
    pub bullet_marker: BulletMarker,
    pub hr_text: String,
    pub quote_style: QuoteStyle,
    /// Разрешить `[code]` в Core Safe Profile (только при явном включении).
    pub safe_allow_code: bool,
    /// Manual Code Highlight: выводить код без `[code]` — через `[color]`-токены
    /// и отступы в 4 пробела.
    pub manual_code_colors: bool,
    /// Предупреждать о потере форматирования.
    pub warn_on_format_loss: bool,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self {
            line_break: LineBreakStyle::Newline,
            heading_sizes: [30, 24, 20, 0, 0, 0],
            default_image_size: ImageSize::Medium,
            bullet_marker: BulletMarker::Bullet,
            hr_text: "--------------------".to_string(),
            quote_style: QuoteStyle::LinePrefix,
            safe_allow_code: false,
            manual_code_colors: true,
            warn_on_format_loss: true,
        }
    }
}
