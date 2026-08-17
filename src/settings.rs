//! Настройки приложения (persist через модуль storage).

use serde::{Deserialize, Serialize};

use crate::model::ImageSize;
use crate::profiles::{BulletMarker, LineBreakStyle, ProfileKind, QuoteStyle, RenderOptions};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Theme {
    Dark,
    Light,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppSettings {
    pub default_profile: ProfileKind,
    pub auto_convert: bool,
    /// Синхронная прокрутка редактора и панели результата.
    pub sync_scroll: bool,
    pub theme: Theme,
    pub editor_font_size: f32,
    pub output_font_size: f32,
    /// Размеры `[size]` для уровней заголовков 1–6 (0 = только жирный).
    pub heading_sizes: [u8; 6],
    pub line_break: LineBreakStyle,
    pub default_image_size: ImageSize,
    /// Разрешить `[code]` в Core Safe Profile.
    pub safe_allow_code: bool,
    /// Ручная подсветка кода в preview.
    pub manual_highlight_preview: bool,
    /// Manual Code Highlight: цветная подсветка в BBCode без тега [code].
    pub manual_code_colors: bool,
    pub warn_on_format_loss: bool,
    pub autosave_enabled: bool,
    pub autosave_interval_secs: u64,
    pub bullet_marker: BulletMarker,
    pub hr_text: String,
    pub quote_style: QuoteStyle,
    /// Пользовательский каталог хранения (пусто = каталог по умолчанию).
    pub storage_dir: String,
    pub color_presets: Vec<String>,
    pub size_presets: Vec<u8>,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            default_profile: ProfileKind::Full,
            auto_convert: true,
            sync_scroll: true,
            theme: Theme::Dark,
            editor_font_size: 15.0,
            output_font_size: 15.0,
            heading_sizes: [30, 24, 20, 0, 0, 0],
            line_break: LineBreakStyle::Newline,
            default_image_size: ImageSize::Medium,
            safe_allow_code: false,
            manual_highlight_preview: true,
            manual_code_colors: true,
            warn_on_format_loss: true,
            autosave_enabled: true,
            autosave_interval_secs: 30,
            bullet_marker: BulletMarker::Bullet,
            hr_text: "--------------------".to_string(),
            quote_style: QuoteStyle::LinePrefix,
            storage_dir: String::new(),
            color_presets: vec![
                "#ff0000".into(),
                "#008000".into(),
                "#0000ff".into(),
                "#ff8c00".into(),
                "#800080".into(),
            ],
            size_presets: vec![10, 12, 14, 18, 22, 26, 30],
        }
    }
}

impl AppSettings {
    pub fn render_options(&self) -> RenderOptions {
        RenderOptions {
            line_break: self.line_break,
            heading_sizes: self.heading_sizes,
            default_image_size: self.default_image_size,
            bullet_marker: self.bullet_marker,
            hr_text: self.hr_text.clone(),
            quote_style: self.quote_style,
            safe_allow_code: self.safe_allow_code,
            manual_code_colors: self.manual_code_colors,
            warn_on_format_loss: self.warn_on_format_loss,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_roundtrip_json() {
        let s = AppSettings::default();
        let json = serde_json::to_string(&s).unwrap();
        let back: AppSettings = serde_json::from_str(&json).unwrap();
        assert_eq!(back.heading_sizes, s.heading_sizes);
        assert_eq!(back.default_profile, ProfileKind::Full);
    }

    #[test]
    fn render_options_derived() {
        let o = AppSettings::default().render_options();
        assert_eq!(o.line_break, LineBreakStyle::Newline);
    }
}
