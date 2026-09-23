//! Состояние приложения и главный цикл GUI.

pub mod dialogs;
pub mod preview;

use std::path::{Path, PathBuf};
use std::time::Instant;

use egui::text::{CCursor, CCursorRange};

use crate::diagnostics::{Diagnostics, Severity};
use crate::parser::parse_markdown;
use crate::profiles::ProfileKind;
use crate::render;
use crate::resources::{ExtractedImage, collect_images, export_resources as write_resources};
use crate::settings::{AppSettings, Theme};
use crate::storage::{RecentFiles, SessionState, Storage, export_json, read_document, write_document};
use crate::tables::ExtractedTable;
use crate::templates::{Template, builtin_templates};

use dialogs::{DialogKind, DialogResult, InsertDialog, TemplateEvent, TemplatesUi};

const EDITOR_ID: &str = "md_editor";

fn install_unicode_fonts(ctx: &egui::Context) {
    let mut database = fontdb::Database::new();
    database.load_system_fonts();

    let mut definitions = egui::FontDefinitions::default();
    for family_name in [
        "Segoe UI",
        "Segoe UI Symbol",
        "Segoe UI Emoji",
        "Microsoft YaHei UI",
        "Meiryo UI",
        "Malgun Gothic",
        "Arial Unicode MS",
        "Noto Sans",
        "Noto Sans CJK SC",
        "Noto Sans CJK JP",
        "Noto Sans CJK KR",
        "Noto Color Emoji",
        "DejaVu Sans",
    ] {
        let query = fontdb::Query {
            families: &[fontdb::Family::Name(family_name)],
            ..Default::default()
        };
        let Some(face_id) = database.query(&query) else {
            continue;
        };
        let font_name = format!("unicode-{family_name}");
        let Some(font_data) = database.with_face_data(face_id, |data, _| data.to_vec()) else {
            continue;
        };

        definitions.font_data.insert(font_name.clone(), egui::FontData::from_owned(font_data).into());
        definitions.families.entry(egui::FontFamily::Proportional).or_default().push(font_name.clone());
        definitions.families.entry(egui::FontFamily::Monospace).or_default().push(font_name);
    }
    ctx.set_fonts(definitions);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Bbcode,
    Preview,
    Diagnostics,
    Inserts,
}

enum PendingDocumentAction {
    New,
    Open(PathBuf),
}

/// Позиция прокрутки панели в долях от её полного хода.
#[derive(Default, Clone, Copy)]
struct PaneScroll {
    ratio: f32,
    max: f32,
}

#[derive(Default)]
struct ScrollSync {
    editor: PaneScroll,
    output: PaneScroll,
    apply_editor: Option<f32>,
    apply_output: Option<f32>,
    last_tab: Option<Tab>,
}

fn pane_scroll<R>(out: &egui::scroll_area::ScrollAreaOutput<R>) -> PaneScroll {
    let max = (out.content_size.y - out.inner_rect.height()).max(0.0);
    let ratio = if max > 1.0 { (out.state.offset.y / max).clamp(0.0, 1.0) } else { 0.0 };
    PaneScroll { ratio, max }
}

pub struct ForgeApp {
    settings: AppSettings,
    settings_storage: Storage,
    storage: Storage,
    storage_dir_draft: String,

    markdown: String,
    output: String,
    diagnostics: Diagnostics,
    pending_tables: Vec<ExtractedTable>,
    pending_images: Vec<ExtractedImage>,

    profile: ProfileKind,
    file_path: Option<PathBuf>,
    doc_modified: bool,
    needs_convert: bool,

    recent: RecentFiles,
    builtin_templates: Vec<Template>,
    user_templates: Vec<Template>,

    active_tab: Tab,
    show_settings: bool,
    show_templates: bool,
    templates_ui: TemplatesUi,
    insert_dialog: Option<InsertDialog>,
    pending_document_action: Option<PendingDocumentAction>,
    scroll_sync: ScrollSync,

    last_autosave: Instant,
    autosave_status: String,
    status_message: String,
}

impl ForgeApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let settings_storage = Storage::new("");
        let mut settings = settings_storage.load_settings().unwrap_or_default();
        let storage = Storage::new(&settings.storage_dir);
        if storage.base_dir() != settings_storage.base_dir() {
            match storage.load_settings_if_exists() {
                Ok(Some(mut stored_settings)) => {
                    stored_settings.storage_dir = settings.storage_dir.clone();
                    settings = stored_settings;
                }
                Ok(None) => {}
                Err(error) => eprintln!("Не удалось загрузить настройки из каталога хранения: {error:#}"),
            }
        }
        settings.output_font_size = settings.editor_font_size;
        let storage_dir_draft = settings.storage_dir.clone();

        let recent = storage.load_recent().unwrap_or_default();
        let user_templates = storage.load_user_templates().unwrap_or_default();
        let session = storage.load_session().ok().flatten().unwrap_or_default();

        install_unicode_fonts(&cc.egui_ctx);
        apply_theme(&cc.egui_ctx, settings.theme);

        let profile = session.profile.unwrap_or(settings.default_profile);
        let mut app = Self {
            profile,
            markdown: session.markdown,
            output: String::new(),
            diagnostics: Diagnostics::default(),
            pending_tables: Vec::new(),
            pending_images: Vec::new(),
            file_path: session.file_path,
            doc_modified: false,
            needs_convert: true,
            recent,
            builtin_templates: builtin_templates(),
            user_templates,
            active_tab: Tab::Bbcode,
            show_settings: false,
            show_templates: false,
            templates_ui: TemplatesUi::default(),
            insert_dialog: None,
            pending_document_action: None,
            scroll_sync: ScrollSync::default(),
            last_autosave: Instant::now(),
            autosave_status: "—".to_string(),
            status_message: String::new(),
            settings,
            settings_storage,
            storage,
            storage_dir_draft,
        };
        app.convert();
        app
    }

    // ------------------------------------------------------------------
    // Логика
    // ------------------------------------------------------------------

    fn convert(&mut self) {
        let parsed = parse_markdown(&self.markdown);
        let images = collect_images(&parsed.document);
        let opts = self.settings.render_options();
        let res = render::render(&parsed.document, self.profile, &opts);
        self.output = res.output;
        self.pending_tables = res.tables;
        self.pending_images = images;
        let mut diags = parsed.diagnostics;
        diags.extend(res.diagnostics);
        self.diagnostics = diags;
        self.needs_convert = false;
    }

    fn persist_settings(&self) -> anyhow::Result<()> {
        self.settings_storage.save_settings(&self.settings)?;
        if self.storage.base_dir() != self.settings_storage.base_dir() {
            self.storage.save_settings(&self.settings)?;
        }
        Ok(())
    }

    fn switch_storage(&mut self) -> anyhow::Result<()> {
        let next_storage = Storage::new(&self.settings.storage_dir);
        if next_storage.base_dir() == self.storage.base_dir() {
            return Ok(());
        }

        let mut recent = next_storage.load_recent()?;
        for path in self.recent.items.iter().rev() {
            recent.push(path.clone());
        }

        let mut user_templates = self.user_templates.clone();
        for template in next_storage.load_user_templates()? {
            if !user_templates.iter().any(|current| {
                current.name == template.name && current.category == template.category
            }) {
                user_templates.push(template);
            }
        }

        let session = SessionState {
            markdown: self.markdown.clone(),
            profile: Some(self.profile),
            file_path: self.file_path.clone(),
        };
        next_storage.save_autosave(&self.markdown)?;
        next_storage.save_session(&session)?;
        next_storage.save_recent(&recent)?;
        next_storage.save_user_templates(&user_templates)?;

        self.storage = next_storage;
        self.recent = recent;
        self.user_templates = user_templates;
        Ok(())
    }

    fn save_recovery_state(&mut self) {
        let session = SessionState {
            markdown: self.markdown.clone(),
            profile: Some(self.profile),
            file_path: self.file_path.clone(),
        };
        let autosave_result = self.storage.save_autosave(&self.markdown);
        let session_result = self.storage.save_session(&session);
        let mut errors = Vec::new();
        if let Err(error) = autosave_result {
            errors.push(format!("autosave.md: {error:#}"));
        }
        if let Err(error) = session_result {
            errors.push(format!("session.json: {error:#}"));
        }
        self.autosave_status = if errors.is_empty() {
            "автосохранено".to_string()
        } else {
            format!("ошибка автосохранения: {}", errors.join("; "))
        };
    }

    fn insert_snippet(&mut self, ctx: &egui::Context, snippet: &str) {
        let id = egui::Id::new(EDITOR_ID);
        if let Some(mut state) = egui::text_edit::TextEditState::load(ctx, id)
            && let Some(range) = state.cursor.char_range()
        {
            let (a, b) = (range.primary.index.0, range.secondary.index.0);
            let (min, max) = (a.min(b), a.max(b));
            let start = char_to_byte(&self.markdown, min);
            let end = char_to_byte(&self.markdown, max);
            self.markdown.replace_range(start..end, snippet);
            let pos = min + snippet.chars().count();
            state.cursor.set_char_range(Some(CCursorRange::one(CCursor::new(pos))));
            state.store(ctx, id);
            self.mark_changed();
            return;
        }
        if !self.markdown.is_empty() && !self.markdown.ends_with('\n') {
            self.markdown.push('\n');
        }
        self.markdown.push_str(snippet);
        self.mark_changed();
    }

    fn mark_changed(&mut self) {
        self.doc_modified = true;
        self.needs_convert = true;
    }

    fn request_new_document(&mut self) {
        if self.doc_modified {
            self.pending_document_action = Some(PendingDocumentAction::New);
        } else {
            self.new_document();
        }
    }

    fn request_open_path(&mut self, path: PathBuf) {
        if self.doc_modified {
            self.pending_document_action = Some(PendingDocumentAction::Open(path));
        } else {
            self.open_path(path);
        }
    }

    fn new_document(&mut self) {
        self.markdown.clear();
        self.file_path = None;
        self.doc_modified = false;
        self.needs_convert = true;
        self.status_message = "Создан новый документ".to_string();
        self.save_recovery_state();
    }

    fn open_document(&mut self) {
        let dialog = rfd::FileDialog::new()
            .add_filter("Markdown / текст", &["md", "txt", "markdown"])
            .add_filter("Все файлы", &["*"]);
        if let Some(path) = dialog.pick_file() {
            self.request_open_path(path);
        }
    }

    fn open_path(&mut self, path: PathBuf) {
        match read_document(&path) {
            Ok(text) => {
                self.markdown = text;
                self.recent.push(path.clone());
                let _ = self.storage.save_recent(&self.recent);
                self.file_path = Some(path);
                self.doc_modified = false;
                self.needs_convert = true;
                self.status_message = "Файл открыт".to_string();
                self.save_recovery_state();
            }
            Err(e) => self.status_message = format!("Ошибка: {e}"),
        }
    }

    fn save_document(&mut self) {
        let path = match &self.file_path {
            Some(p) => p.clone(),
            None => {
                let dialog = rfd::FileDialog::new()
                    .add_filter("Markdown", &["md"])
                    .add_filter("Текст", &["txt"])
                    .set_file_name("сообщение.md");
                match dialog.save_file() {
                    Some(p) => p,
                    None => return,
                }
            }
        };
        match write_document(&path, &self.markdown) {
            Ok(()) => {
                self.recent.push(path.clone());
                let _ = self.storage.save_recent(&self.recent);
                self.file_path = Some(path.clone());
                self.doc_modified = false;
                self.status_message = "Сохранено".to_string();
            }
            Err(e) => self.status_message = format!("Ошибка: {e}"),
        }
    }

    fn export_result(&mut self) {
        if self.needs_convert {
            self.convert();
        }
        let dialog = rfd::FileDialog::new()
            .add_filter("Текст", &["txt"])
            .add_filter("BBCode", &["bbcode.txt"])
            .add_filter("JSON (markdown + bbcode)", &["json"])
            .set_file_name("сообщение.bbcode.txt");
        if let Some(path) = dialog.save_file() {
            let is_json = path.extension().map(|e| e == "json").unwrap_or(false);
            let result = if is_json {
                export_json(&path, &self.markdown, &self.output, self.profile.label())
            } else {
                write_document(&path, &self.output)
            };
            self.status_message = match &result {
                Ok(()) => format!("Экспортировано: {}", path.display()),
                Err(e) => format!("Ошибка экспорта: {e}"),
            };
        }
    }

    fn export_resources(&mut self) {
        if self.needs_convert {
            self.convert();
        }
        if self.pending_tables.is_empty() && self.pending_images.is_empty() {
            self.status_message = "В документе нет изображений или таблиц для экспорта".to_string();
            return;
        }

        let dialog = rfd::FileDialog::new()
            .set_title("Куда экспортировать изображения и таблицы")
            .pick_folder();
        let Some(destination) = dialog else {
            return;
        };
        let source_dir = self.file_path.as_deref().and_then(Path::parent);
        match write_resources(
            &self.pending_tables,
            &self.pending_images,
            &destination,
            source_dir,
        ) {
            Ok(exported) => {
                self.status_message = format!(
                    "Ресурсы экспортированы в {} (таблиц: {}, изображений: {}, реестр: {})",
                    destination.display(),
                    exported.table_paths.len(),
                    exported.image_paths.len(),
                    exported
                        .manifest_path
                        .file_name()
                        .map(|name| name.to_string_lossy())
                        .unwrap_or_else(|| exported.manifest_path.to_string_lossy()),
                );
            }
            Err(e) => self.status_message = format!("Ошибка экспорта ресурсов: {e}"),
        }
    }

    fn copy_result(&mut self) {
        if self.needs_convert {
            self.convert();
        }
        match arboard::Clipboard::new().and_then(|mut c| c.set_text(self.output.clone())) {
            Ok(()) => self.status_message = "Результат скопирован в буфер обмена".to_string(),
            Err(e) => self.status_message = format!("Ошибка буфера обмена: {e}"),
        }
    }

    fn copy_formatted_result(&mut self) {
        if self.needs_convert {
            self.convert();
        }
        let html = render::html::bbcode_to_html(&self.output);
        let fallback = self.output.clone();
        match arboard::Clipboard::new().and_then(|mut c| c.set_html(html, Some(fallback))) {
            Ok(()) => {
                self.status_message =
                    "Форматированный результат скопирован в буфер обмена".to_string();
            }
            Err(e) => self.status_message = format!("Ошибка буфера обмена: {e}"),
        }
    }

    fn autosave_tick(&mut self) {
        if !self.settings.autosave_enabled {
            return;
        }
        let interval = self.settings.autosave_interval_secs.max(5);
        if self.last_autosave.elapsed().as_secs() >= interval {
            self.last_autosave = Instant::now();
            self.save_recovery_state();
        }
    }

    fn save_all_state(&mut self) {
        if let Err(error) = self.persist_settings() {
            eprintln!("Не удалось сохранить настройки: {error:#}");
        }
        if let Err(error) = self.storage.save_recent(&self.recent) {
            eprintln!("Не удалось сохранить список недавних файлов: {error:#}");
        }
        if let Err(error) = self.storage.save_user_templates(&self.user_templates) {
            eprintln!("Не удалось сохранить шаблоны: {error:#}");
        }
        let session = SessionState {
            markdown: self.markdown.clone(),
            profile: Some(self.profile),
            file_path: self.file_path.clone(),
        };
        if let Err(error) = self.storage.save_session(&session) {
            eprintln!("Не удалось сохранить сессию: {error:#}");
        }
    }

    // ------------------------------------------------------------------
    // UI
    // ------------------------------------------------------------------

    fn ui_top_panel(&mut self, ui: &mut egui::Ui) {
        egui::Panel::top("top_panel").show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.strong("BitrixText Forge");
                ui.separator();

                if ui.button("📄 Новый").clicked() {
                    self.request_new_document();
                }
                if ui.button("📂 Открыть").clicked() {
                    self.open_document();
                }
                let mut open_recent: Option<PathBuf> = None;
                if !self.recent.items.is_empty() {
                    ui.menu_button("🕘", |ui| {
                        ui.label("Последние файлы");
                        for p in &self.recent.items {
                            let name = p.file_name().map(|n| n.to_string_lossy().to_string())
                                .unwrap_or_else(|| p.display().to_string());
                            if ui.button(name).on_hover_text(p.display().to_string()).clicked() {
                                open_recent = Some(p.clone());
                                ui.close();
                            }
                        }
                    });
                }
                if let Some(p) = open_recent {
                    self.request_open_path(p);
                }
                if ui.button("💾 Сохранить").clicked() {
                    self.save_document();
                }
                if ui.button("📤 Экспорт").clicked() {
                    self.export_result();
                }
                if ui
                    .button("📦 Ресурсы")
                    .on_hover_text("Выбрать каталог для экспорта изображений и таблиц")
                    .clicked()
                {
                    self.export_resources();
                }
                if ui.button("📋 Копировать результат").clicked() {
                    self.copy_result();
                }
                if ui
                    .button("✨ Копировать форматированный")
                    .on_hover_text("Скопировать HTML с BBCode как текстовым fallback")
                    .clicked()
                {
                    self.copy_formatted_result();
                }

                ui.separator();
                let prev_profile = self.profile;
                egui::ComboBox::from_id_salt("profile_combo")
                    .selected_text(self.profile.label())
                    .show_ui(ui, |ui| {
                        for p in ProfileKind::ALL {
                            ui.selectable_value(&mut self.profile, p, p.label());
                        }
                    });
                if prev_profile != self.profile {
                    self.convert();
                }

                if ui
                    .checkbox(&mut self.settings.auto_convert, "Авто")
                    .on_hover_text("Автообновление результата при вводе")
                    .changed()
                {
                    if let Err(error) = self.persist_settings() {
                        self.status_message = format!("Ошибка сохранения настроек: {error:#}");
                    }
                }
                if !self.settings.auto_convert && ui.button("⟳ Конвертировать").clicked() {
                    self.convert();
                }

                ui.separator();
                if ui.button("🧩 Шаблоны").clicked() {
                    self.show_templates = !self.show_templates;
                }
                if ui.button("➕ Спецвставки").clicked() {
                    self.active_tab = Tab::Inserts;
                }
                if ui.button("⚙ Настройки").clicked() {
                    self.show_settings = !self.show_settings;
                }
            });
        });
    }

    fn ui_status_bar(&mut self, ui: &mut egui::Ui) {
        egui::Panel::bottom("status_bar").show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(format!("Профиль: {}", self.profile.label()));
                ui.separator();
                let draft = if self.doc_modified { "черновик: изменён" } else { "черновик: сохранён" };
                ui.label(draft);
                ui.separator();
                ui.label(format!("Автосохранение: {}", self.autosave_status));
                ui.separator();
                ui.label(format!(
                    "{} симв. / {} строк",
                    self.markdown.chars().count(),
                    self.markdown.lines().count()
                ));
                ui.separator();
                let warns = self.diagnostics.warnings();
                if warns > 0 {
                    ui.colored_label(
                        egui::Color32::from_rgb(230, 170, 60),
                        format!("⚠ предупреждений: {warns}"),
                    );
                } else {
                    ui.weak("без предупреждений");
                }
                if !self.status_message.is_empty() {
                    ui.separator();
                    ui.weak(&self.status_message);
                }
                ui.separator();
                ui.weak(crate::build_info());
            });
        });
    }

    fn ui_central(&mut self, ui: &mut egui::Ui) {
        let mut editor_pane = PaneScroll::default();
        let mut output_pane = None;
        ui.columns(2, |columns| {
            let editor_ui = &mut columns[0];
            editor_ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    ui.strong("Markdown");
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let tables = self.pending_tables.len();
                        let images = self.pending_images.len();
                        if tables > 0 || images > 0 {
                            ui.weak(format!("таблиц: {tables}, изображений: {images}"));
                        }
                        if let Some(p) = &self.file_path {
                            ui.weak(p.file_name().map(|n| n.to_string_lossy().to_string())
                                .unwrap_or_default());
                        }
                    });
                });
                ui.separator();
                let mut area = egui::ScrollArea::vertical().id_salt("editor_scroll");
                if let Some(offset) = self.scroll_sync.apply_editor.take() {
                    area = area.vertical_scroll_offset(offset);
                }
                let out = area.show(ui, |ui| {
                    let editor = egui::TextEdit::multiline(&mut self.markdown)
                        .id(egui::Id::new(EDITOR_ID))
                        .font(egui::FontId::monospace(self.settings.editor_font_size))
                        .desired_width(f32::INFINITY)
                        .desired_rows(30)
                        .hint_text("Введите Markdown…");
                    if ui.add(editor).changed() {
                        self.mark_changed();
                    }
                });
                editor_pane = pane_scroll(&out);
            });

            let output_ui = &mut columns[1];
            output_ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.active_tab, Tab::Bbcode, "BBCode");
                    ui.selectable_value(&mut self.active_tab, Tab::Preview, "Preview");
                    let warns = self.diagnostics.warnings();
                    let diag_label = if warns > 0 {
                        format!("Диагностика ({warns})")
                    } else {
                        "Диагностика".to_string()
                    };
                    ui.selectable_value(&mut self.active_tab, Tab::Diagnostics, diag_label);
                    ui.selectable_value(&mut self.active_tab, Tab::Inserts, "Спецвставки");
                });
                ui.separator();

                output_pane = match self.active_tab {
                    Tab::Bbcode => {
                        let mut area = egui::ScrollArea::vertical().id_salt("bbcode_scroll");
                        if let Some(offset) = self.scroll_sync.apply_output.take() {
                            area = area.vertical_scroll_offset(offset);
                        }
                        let out = area.show(ui, |ui| {
                            let mut out = self.output.clone();
                            let view = egui::TextEdit::multiline(&mut out)
                                .font(egui::FontId::monospace(self.settings.editor_font_size))
                                .desired_width(f32::INFINITY)
                                .desired_rows(30)
                                .interactive(true);
                            ui.add(view);
                        });
                        Some(pane_scroll(&out))
                    }
                    Tab::Preview => {
                        let mut area = egui::ScrollArea::vertical().id_salt("preview_scroll");
                        if let Some(offset) = self.scroll_sync.apply_output.take() {
                            area = area.vertical_scroll_offset(offset);
                        }
                        let out = area.show(ui, |ui| {
                            preview::show_preview(ui, &self.output, &self.settings);
                        });
                        Some(pane_scroll(&out))
                    }
                    Tab::Diagnostics => {
                        egui::ScrollArea::vertical().id_salt("diag_scroll").show(ui, |ui| {
                            if self.diagnostics.items.is_empty() {
                                ui.weak("Диагностических сообщений нет.");
                            }
                            for d in &self.diagnostics.items {
                                let (icon, color) = match d.severity {
                                    Severity::Info => ("ℹ", egui::Color32::from_rgb(120, 160, 220)),
                                    Severity::Warning => ("⚠", egui::Color32::from_rgb(230, 170, 60)),
                                    Severity::Error => ("⛔", egui::Color32::from_rgb(230, 100, 100)),
                                };
                                ui.horizontal_wrapped(|ui| {
                                    ui.colored_label(color, icon);
                                    ui.label(&d.message);
                                });
                            }
                        });
                        None
                    }
                    Tab::Inserts => {
                        self.ui_inserts_tab(ui);
                        None
                    }
                };
            });
        });

        self.sync_scroll(ui.ctx(), editor_pane, output_pane);
    }

    /// Пропорционально связывает прокрутку редактора и активной панели результата.
    fn sync_scroll(
        &mut self,
        ctx: &egui::Context,
        editor: PaneScroll,
        output: Option<PaneScroll>,
    ) {
        let tab_changed = self.scroll_sync.last_tab != Some(self.active_tab);
        self.scroll_sync.last_tab = Some(self.active_tab);
        let prev_editor = self.scroll_sync.editor.ratio;
        let prev_output = self.scroll_sync.output.ratio;

        let Some(output) = output else {
            self.scroll_sync.editor = editor;
            self.scroll_sync.output = PaneScroll::default();
            return;
        };
        if !self.settings.sync_scroll {
            self.scroll_sync.editor = editor;
            self.scroll_sync.output = output;
            return;
        }

        const EPS: f32 = 0.001;
        let editor_moved = (editor.ratio - prev_editor).abs() > EPS;
        let output_moved = (output.ratio - prev_output).abs() > EPS;

        let target = if tab_changed || (editor_moved && !output_moved) {
            self.scroll_sync.apply_output = Some(editor.ratio * output.max);
            editor.ratio
        } else if output_moved {
            self.scroll_sync.apply_editor = Some(output.ratio * editor.max);
            output.ratio
        } else {
            output.ratio
        };

        self.scroll_sync.output = PaneScroll { ratio: target, max: output.max };
        self.scroll_sync.editor = PaneScroll { ratio: target, max: editor.max };
        if (target - editor.ratio).abs() > EPS || (target - output.ratio).abs() > EPS {
            ctx.request_repaint();
        }
    }

    fn ui_inserts_tab(&mut self, ui: &mut egui::Ui) {
        ui.label("Специальные вставки Bitrix24 — вставляются в позицию курсора редактора.");
        ui.add_space(8.0);

        let mut open_dialog: Option<DialogKind> = None;

        egui::Grid::new("inserts_grid").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
            if ui.button("[u] Подчёркивание").clicked() {
                open_dialog = Some(DialogKind::Underline);
            }
            ui.label("Подчёркнутый текст (нет Markdown-аналога)");
            ui.end_row();

            if ui.button("[user] Упоминание").clicked() {
                open_dialog = Some(DialogKind::User);
            }
            ui.label("Упоминание сотрудника по ID");
            ui.end_row();

            if ui.button("[size] Размер шрифта").clicked() {
                open_dialog = Some(DialogKind::Size);
            }
            ui.label("Размер 8–30 px");
            ui.end_row();

            if ui.button("[color] Цвет текста").clicked() {
                open_dialog = Some(DialogKind::Color);
            }
            ui.label("HEX-цвет из 3 или 6 символов");
            ui.end_row();

            if ui.button("[icon] Иконка").clicked() {
                open_dialog = Some(DialogKind::Icon);
            }
            ui.label("Встроенная иконка с параметрами size и title");
            ui.end_row();

        });

        ui.add_space(8.0);
        ui.weak("Поддерживаются только [b], [i], [u], [s], [url], [user], [icon], [color] и [size].");

        if let Some(kind) = open_dialog {
            self.insert_dialog = Some(InsertDialog::new(kind, &self.settings));
        }
    }

    fn ui_windows(&mut self, ctx: &egui::Context) {
        if self.pending_document_action.is_some() {
            let mut open = true;
            let mut discard = false;
            let mut cancel = false;
            egui::Window::new("Несохранённые изменения")
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.label("Текущий документ изменён. Сохраните его или отмените замену.");
                    ui.horizontal(|ui| {
                        if ui.button("Не сохранять").clicked() {
                            discard = true;
                        }
                        if ui.button("Отмена").clicked() {
                            cancel = true;
                        }
                    });
                });
            if !open || cancel {
                self.pending_document_action = None;
            } else if discard {
                match self.pending_document_action.take() {
                    Some(PendingDocumentAction::New) => self.new_document(),
                    Some(PendingDocumentAction::Open(path)) => self.open_path(path),
                    None => {}
                }
            }
        }

        // диалог спецвставки
        if let Some(dialog) = &mut self.insert_dialog {
            match dialog.show(ctx, &self.settings) {
                DialogResult::Open => {}
                DialogResult::Cancel => self.insert_dialog = None,
                DialogResult::Insert(snippet) => {
                    self.insert_dialog = None;
                    self.insert_snippet(ctx, &snippet);
                }
            }
        }

        // настройки
        if self.show_settings {
            let mut open = self.show_settings;
            let previous_storage_dir = self.settings.storage_dir.clone();
            let change = dialogs::show_settings_window(
                ctx,
                &mut open,
                &mut self.settings,
                &mut self.storage_dir_draft,
            );
            self.show_settings = open;
            if change.storage_dir_changed {
                if let Err(error) = self.switch_storage() {
                    self.settings.storage_dir = previous_storage_dir.clone();
                    self.storage_dir_draft = previous_storage_dir;
                    self.status_message = format!("Ошибка смены каталога хранения: {error:#}");
                }
            }
            if change.changed {
                apply_theme(ctx, self.settings.theme);
                self.needs_convert = true;
                if let Err(error) = self.persist_settings() {
                    self.status_message = format!("Ошибка сохранения настроек: {error:#}");
                }
            }
        }

        // шаблоны
        if self.show_templates {
            let mut open = self.show_templates;
            let event = dialogs::show_templates_window(
                ctx,
                &mut open,
                &mut self.templates_ui,
                &self.builtin_templates,
                &self.user_templates,
            );
            self.show_templates = open;
            match event {
                TemplateEvent::None => {}
                TemplateEvent::Insert(md) => {
                    self.insert_snippet(ctx, &md);
                }
                TemplateEvent::SaveCurrent { name, category } => {
                    if self.needs_convert {
                        self.convert();
                    }
                    let t = Template {
                        name,
                        category,
                        markdown: self.markdown.clone(),
                        bbcode: self.output.clone(),
                        variables: Template::extract_variables(&self.markdown),
                        modified: String::new(),
                        system: false,
                    };
                    self.user_templates.push(t);
                    let _ = self.storage.save_user_templates(&self.user_templates);
                    self.status_message = "Шаблон сохранён".to_string();
                }
                TemplateEvent::DeleteUser(i) => {
                    if i < self.user_templates.len() {
                        self.user_templates.remove(i);
                        let _ = self.storage.save_user_templates(&self.user_templates);
                    }
                }
            }
        }
    }
}

impl eframe::App for ForgeApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.autosave_tick();

        self.ui_top_panel(ui);
        self.ui_status_bar(ui);
        egui::CentralPanel::default().show(ui, |ui| self.ui_central(ui));
        self.ui_windows(&ctx);

        if self.needs_convert && self.settings.auto_convert {
            self.convert();
        }

        // периодическая перерисовка для таймера автосохранения
        if self.settings.autosave_enabled {
            ctx.request_repaint_after(std::time::Duration::from_secs(1));
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.save_all_state();
    }
}

fn apply_theme(ctx: &egui::Context, theme: Theme) {
    match theme {
        Theme::Dark => ctx.set_visuals(egui::Visuals::dark()),
        Theme::Light => ctx.set_visuals(egui::Visuals::light()),
    }
}

fn char_to_byte(s: &str, char_idx: usize) -> usize {
    s.char_indices().nth(char_idx).map(|(b, _)| b).unwrap_or(s.len())
}
