//! Диалоги спецвставок Bitrix24, окно настроек и окно шаблонов.

use crate::model::{ImageSize, SIZE_MAX, SIZE_MIN, is_valid_size, normalize_hex_color};
use crate::profiles::{BulletMarker, ProfileKind, QuoteStyle};
use crate::settings::{AppSettings, Theme};
use crate::templates::Template;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogKind {
    Underline,
    User,
    Size,
    Color,
    Icon,
}

impl DialogKind {
    pub fn title(&self) -> &'static str {
        match self {
            DialogKind::Underline => "Подчёркивание [u]",
            DialogKind::User => "Упоминание сотрудника [user]",
            DialogKind::Size => "Размер шрифта [size]",
            DialogKind::Color => "Цвет текста [color]",
            DialogKind::Icon => "Иконка [icon]",
        }
    }
}

pub enum DialogResult {
    Open,
    Cancel,
    Insert(String),
}

pub struct InsertDialog {
    pub kind: DialogKind,
    text: String,
    user_id: String,
    url: String,
    size_px: u8,
    color_hex: String,
    color_rgb: [u8; 3],
    icon_size: String,
    icon_title: String,
    error: Option<String>,
}

impl InsertDialog {
    pub fn new(kind: DialogKind, _settings: &AppSettings) -> Self {
        Self {
            kind,
            text: String::new(),
            user_id: String::new(),
            url: String::new(),
            size_px: 14,
            color_hex: "#ff0000".to_string(),
            color_rgb: [255, 0, 0],
            icon_size: String::new(),
            icon_title: String::new(),
            error: None,
        }
    }

    pub fn show(&mut self, ctx: &egui::Context, settings: &AppSettings) -> DialogResult {
        let mut result = DialogResult::Open;
        let mut open = true;
        egui::Window::new(self.kind.title())
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.set_min_width(340.0);
                match self.kind {
                    DialogKind::Underline => {
                        ui.label("Текст:");
                        ui.text_edit_singleline(&mut self.text);
                    }
                    DialogKind::User => {
                        ui.label("ID сотрудника:");
                        ui.text_edit_singleline(&mut self.user_id);
                        ui.label("Отображаемое имя:");
                        ui.text_edit_singleline(&mut self.text);
                    }
                    DialogKind::Size => {
                        ui.label("Текст:");
                        ui.text_edit_singleline(&mut self.text);
                        ui.add(
                            egui::Slider::new(&mut self.size_px, SIZE_MIN..=SIZE_MAX)
                                .text("размер, px (8–30)"),
                        );
                        ui.horizontal_wrapped(|ui| {
                            ui.label("Пресеты:");
                            for px in &settings.size_presets {
                                if ui.small_button(px.to_string()).clicked() {
                                    self.size_px = *px;
                                }
                            }
                        });
                    }
                    DialogKind::Color => {
                        ui.label("Текст:");
                        ui.text_edit_singleline(&mut self.text);
                        ui.horizontal(|ui| {
                            ui.label("Цвет:");
                            if egui::color_picker::color_edit_button_srgb(ui, &mut self.color_rgb)
                                .changed()
                            {
                                self.color_hex = format!(
                                    "#{:02x}{:02x}{:02x}",
                                    self.color_rgb[0], self.color_rgb[1], self.color_rgb[2]
                                );
                            }
                            ui.label("HEX:");
                            ui.add(egui::TextEdit::singleline(&mut self.color_hex).desired_width(90.0));
                        });
                        ui.horizontal_wrapped(|ui| {
                            ui.label("Пресеты:");
                            for preset in &settings.color_presets {
                                if ui.small_button(preset.as_str()).clicked() {
                                    self.color_hex = preset.clone();
                                }
                            }
                        });
                    }
                    DialogKind::Icon => {
                        ui.label("URL иконки:");
                        ui.text_edit_singleline(&mut self.url);
                        ui.horizontal(|ui| {
                            ui.label("size (необяз.):");
                            ui.add(egui::TextEdit::singleline(&mut self.icon_size).desired_width(60.0));
                            ui.label("title (необяз.):");
                            ui.text_edit_singleline(&mut self.icon_title);
                        });
                    }
                }

                if let Some(err) = &self.error {
                    ui.colored_label(egui::Color32::from_rgb(230, 100, 100), err);
                }

                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui.button("Вставить").clicked() {
                        match self.build_snippet() {
                            Ok(snippet) => result = DialogResult::Insert(snippet),
                            Err(e) => self.error = Some(e),
                        }
                    }
                    if ui.button("Отмена").clicked() {
                        result = DialogResult::Cancel;
                    }
                });
            });
        if !open {
            return DialogResult::Cancel;
        }
        result
    }

    fn build_snippet(&self) -> Result<String, String> {
        match self.kind {
            DialogKind::Underline => {
                let t = if self.text.is_empty() { "текст" } else { &self.text };
                Ok(format!("[u]{t}[/u]"))
            }
            DialogKind::User => {
                let id = self.user_id.trim();
                if id.is_empty() {
                    return Err("Укажите ID сотрудника.".into());
                }
                let text = if self.text.is_empty() { "Сотрудник" } else { &self.text };
                Ok(format!("[user={id}]{text}[/user]"))
            }
            DialogKind::Size => {
                if !is_valid_size(self.size_px) {
                    return Err("Размер должен быть в диапазоне 8–30.".into());
                }
                let t = if self.text.is_empty() { "текст" } else { &self.text };
                Ok(format!("[size={}]{t}[/size]", self.size_px))
            }
            DialogKind::Color => {
                let hex = normalize_hex_color(&self.color_hex)
                    .ok_or("Некорректный HEX: требуется 3 или 6 hex-символов.")?;
                let t = if self.text.is_empty() { "текст" } else { &self.text };
                Ok(format!("[color={hex}]{t}[/color]"))
            }
            DialogKind::Icon => {
                let url = self.url.trim();
                if url.is_empty() {
                    return Err("URL иконки обязателен.".into());
                }
                let mut params = String::new();
                if !self.icon_size.trim().is_empty() {
                    params.push_str(&format!(" size={}", self.icon_size.trim()));
                }
                if !self.icon_title.trim().is_empty() {
                    params.push_str(&format!(" title={}", self.icon_title.trim()));
                }
                Ok(format!("[icon={url}{params}]"))
            }
        }
    }
}

// ----------------------------------------------------------------------------
// Окно настроек
// ----------------------------------------------------------------------------

/// Возвращает true, если настройки изменились.
pub fn show_settings_window(ctx: &egui::Context, open: &mut bool, settings: &mut AppSettings) -> bool {
    let mut changed = false;
    egui::Window::new("Настройки")
        .open(open)
        .collapsible(false)
        .default_width(460.0)
        .show(ctx, |ui| {
            egui::ScrollArea::vertical().max_height(500.0).show(ui, |ui| {
                ui.heading("Общие");
                egui::ComboBox::from_label("Профиль по умолчанию")
                    .selected_text(settings.default_profile.label())
                    .show_ui(ui, |ui| {
                        for p in ProfileKind::ALL {
                            changed |= ui
                                .selectable_value(&mut settings.default_profile, p, p.label())
                                .changed();
                        }
                    });
                changed |= ui.checkbox(&mut settings.auto_convert, "Автоконвертация").changed();
                changed |= ui
                    .checkbox(&mut settings.sync_scroll, "Синхронная прокрутка редактора и результата")
                    .on_hover_text(
                        "Прокрутка одной панели пропорционально сдвигает вторую \
                         (вкладки BBCode и Preview).",
                    )
                    .changed();
                ui.horizontal(|ui| {
                    ui.label("Тема:");
                    changed |= ui.selectable_value(&mut settings.theme, Theme::Dark, "Тёмная").changed();
                    changed |=
                        ui.selectable_value(&mut settings.theme, Theme::Light, "Светлая").changed();
                });
                if ui
                    .add(egui::Slider::new(&mut settings.editor_font_size, 10.0..=24.0)
                        .text("Шрифт редактора и результата"))
                    .changed()
                {
                    settings.output_font_size = settings.editor_font_size;
                    changed = true;
                }

                ui.separator();
                ui.heading("Рендеринг");
                ui.label("Размеры заголовков H1–H6 ([size], 0 = только жирный):");
                for (i, size) in settings.heading_sizes.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        ui.label(format!("H{}", i + 1));
                        let mut v = *size as i32;
                        if ui
                            .add(egui::Slider::new(&mut v, 0..=30).custom_formatter(|n, _| {
                                if n == 0.0 { "жирный".to_string() } else { format!("{n}px") }
                            }))
                            .changed()
                        {
                            // 0 или документированный диапазон 8–30
                            *size = if v == 0 { 0 } else { (v.clamp(8, 30)) as u8 };
                            changed = true;
                        }
                    });
                }
                ui.horizontal(|ui| {
                    ui.label("Перенос строк:");
                    ui.label("\\n");
                });
                ui.horizontal(|ui| {
                    ui.label("Размер изображений по умолчанию:");
                    egui::ComboBox::from_id_salt("def_img_size")
                        .selected_text(settings.default_image_size.as_str())
                        .show_ui(ui, |ui| {
                            for s in ImageSize::ALL {
                                changed |= ui
                                    .selectable_value(&mut settings.default_image_size, s, s.as_str())
                                    .changed();
                            }
                        });
                });
                ui.horizontal(|ui| {
                    ui.label("Маркер списка:");
                    changed |= ui
                        .selectable_value(&mut settings.bullet_marker, BulletMarker::Bullet, "•")
                        .changed();
                    changed |= ui
                        .selectable_value(&mut settings.bullet_marker, BulletMarker::Dash, "-")
                        .changed();
                });
                ui.horizontal(|ui| {
                    ui.label("Стиль цитат:");
                    changed |= ui
                        .selectable_value(&mut settings.quote_style, QuoteStyle::LinePrefix, ">> построчно")
                        .changed();
                    changed |= ui
                        .selectable_value(&mut settings.quote_style, QuoteStyle::FullBlock, "------ блок ------")
                        .changed();
                });
                ui.horizontal(|ui| {
                    ui.label("Горизонтальная линия:");
                    changed |= ui.text_edit_singleline(&mut settings.hr_text).changed();
                });

                ui.separator();
                ui.heading("Код");
                changed |= ui
                    .checkbox(
                        &mut settings.safe_allow_code,
                        "Разрешить [code] в Core Safe Profile",
                    )
                    .changed();
                changed |= ui
                    .checkbox(
                        &mut settings.manual_highlight_preview,
                        "Ручная подсветка кода в preview",
                    )
                    .changed();
                changed |= ui
                    .checkbox(
                        &mut settings.manual_code_colors,
                        "Manual Code Highlight: подсветка в BBCode без [code]",
                    )
                    .on_hover_text(
                        "В профиле Manual Code Highlight код выводится без контейнера [code]: \
                         токены оборачиваются в [color=#HEX], каждая строка начинается \
                         с цитаты >> и табуляции в 4 пробела.",
                    )
                    .changed();
                changed |= ui
                    .checkbox(
                        &mut settings.warn_on_format_loss,
                        "Предупреждать о потере форматирования",
                    )
                    .changed();
                ui.weak("[code] — только контейнер: подсветка синтаксиса в Bitrix24 не гарантируется.");

                ui.separator();
                ui.heading("Хранение");
                changed |= ui.checkbox(&mut settings.autosave_enabled, "Автосохранение").changed();
                let mut secs = settings.autosave_interval_secs as u32;
                if ui
                    .add(egui::Slider::new(&mut secs, 5..=300).text("интервал, сек"))
                    .changed()
                {
                    settings.autosave_interval_secs = secs as u64;
                    changed = true;
                }
                ui.horizontal(|ui| {
                    ui.label("Каталог хранения (пусто = по умолчанию):");
                });
                changed |= ui.text_edit_singleline(&mut settings.storage_dir).changed();
            });
        });
    changed
}

// ----------------------------------------------------------------------------
// Окно шаблонов
// ----------------------------------------------------------------------------

pub enum TemplateEvent {
    None,
    Insert(String),
    SaveCurrent { name: String, category: String },
    DeleteUser(usize),
}

#[derive(Default)]
pub struct TemplatesUi {
    pub new_name: String,
    pub new_category: String,
    pub filter: String,
}

pub fn show_templates_window(
    ctx: &egui::Context,
    open: &mut bool,
    ui_state: &mut TemplatesUi,
    builtin: &[Template],
    user: &[Template],
) -> TemplateEvent {
    let mut event = TemplateEvent::None;
    egui::Window::new("Шаблоны")
        .open(open)
        .default_width(520.0)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("Фильтр:");
                ui.text_edit_singleline(&mut ui_state.filter);
            });
            ui.separator();
            egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                let filter = ui_state.filter.to_lowercase();
                ui.strong("Системные");
                for t in builtin {
                    if !filter.is_empty()
                        && !t.name.to_lowercase().contains(&filter)
                        && !t.category.to_lowercase().contains(&filter)
                    {
                        continue;
                    }
                    ui.horizontal(|ui| {
                        if ui.button("Вставить").clicked() {
                            event = TemplateEvent::Insert(t.markdown.clone());
                        }
                        ui.label(format!("{} · {}", t.name, t.category));
                        if !t.variables.is_empty() {
                            ui.weak(format!("переменные: {}", t.variables.join(", ")));
                        }
                    });
                }
                ui.separator();
                ui.strong("Пользовательские");
                if user.is_empty() {
                    ui.weak("Нет пользовательских шаблонов.");
                }
                for (i, t) in user.iter().enumerate() {
                    if !filter.is_empty()
                        && !t.name.to_lowercase().contains(&filter)
                        && !t.category.to_lowercase().contains(&filter)
                    {
                        continue;
                    }
                    ui.horizontal(|ui| {
                        if ui.button("Вставить").clicked() {
                            event = TemplateEvent::Insert(t.markdown.clone());
                        }
                        if ui.button("🗑").on_hover_text("Удалить").clicked() {
                            event = TemplateEvent::DeleteUser(i);
                        }
                        ui.label(format!("{} · {}", t.name, t.category));
                        if !t.modified.is_empty() {
                            ui.weak(&t.modified);
                        }
                    });
                }
            });
            ui.separator();
            ui.strong("Сохранить текущий документ как шаблон");
            ui.horizontal(|ui| {
                ui.label("Имя:");
                ui.text_edit_singleline(&mut ui_state.new_name);
                ui.label("Категория:");
                ui.text_edit_singleline(&mut ui_state.new_category);
                if ui.button("Сохранить").clicked() && !ui_state.new_name.trim().is_empty() {
                    event = TemplateEvent::SaveCurrent {
                        name: ui_state.new_name.trim().to_string(),
                        category: if ui_state.new_category.trim().is_empty() {
                            "Пользовательские".to_string()
                        } else {
                            ui_state.new_category.trim().to_string()
                        },
                    };
                    ui_state.new_name.clear();
                }
            });
        });
    event
}
