//! Состояние приложения и главный цикл GUI.

pub mod dialogs;
pub mod preview;

use std::path::PathBuf;
use std::time::Instant;

use egui::text::{CCursor, CCursorRange};

use crate::diagnostics::{Diagnostics, Severity};
use crate::model::Document;
use crate::parser::parse_markdown;
use crate::profiles::ProfileKind;
use crate::render;
use crate::settings::{AppSettings, Theme};
use crate::storage::{RecentFiles, SessionState, Storage, export_json, read_document, write_document};
use crate::templates::{Template, builtin_templates};

use dialogs::{DialogKind, DialogResult, InsertDialog, TemplateEvent, TemplatesUi};

const EDITOR_ID: &str = "md_editor";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Bbcode,
    Preview,
    Diagnostics,
    Inserts,
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
    storage: Storage,

    markdown: String,
    output: String,
    document: Document,
    diagnostics: Diagnostics,

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
    scroll_sync: ScrollSync,

    last_autosave: Instant,
    autosave_status: String,
    status_message: String,
}

impl ForgeApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let bootstrap = Storage::new("");
        let mut settings = bootstrap.load_settings().unwrap_or_default();
        settings.output_font_size = settings.editor_font_size;
        let storage = Storage::new(&settings.storage_dir);

        let recent = storage.load_recent().unwrap_or_default();
        let user_templates = storage.load_user_templates().unwrap_or_default();
        let session = storage.load_session().ok().flatten().unwrap_or_default();

        apply_theme(&cc.egui_ctx, settings.theme);

        let profile = session.profile.unwrap_or(settings.default_profile);
        let mut app = Self {
            profile,
            markdown: session.markdown,
            output: String::new(),
            document: Document::default(),
            diagnostics: Diagnostics::default(),
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
            scroll_sync: ScrollSync::default(),
            last_autosave: Instant::now(),
            autosave_status: "—".to_string(),
            status_message: String::new(),
            settings,
            storage,
        };
        app.convert();
        app
    }

    // ------------------------------------------------------------------
    // Логика
    // ------------------------------------------------------------------

    fn convert(&mut self) {
        let parsed = parse_markdown(&self.markdown);
        let opts = self.settings.render_options();
        let res = render::render(&parsed.document, self.profile, &opts);
        self.output = res.output;
        let mut diags = parsed.diagnostics;
        diags.extend(res.diagnostics);
        self.diagnostics = diags;
        self.document = parsed.document;
        self.needs_convert = false;
    }

    fn insert_snippet(&mut self, ctx: &egui::Context, snippet: &str) {
        let id = egui::Id::new(EDITOR_ID);
        if let Some(mut state) = egui::text_edit::TextEditState::load(ctx, id) {
            if let Some(range) = state.cursor.char_range() {
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

    fn new_document(&mut self) {
        self.markdown.clear();
        self.file_path = None;
        self.doc_modified = false;
        self.mark_changed();
        self.doc_modified = false;
        self.status_message = "Создан новый документ".to_string();
    }

    fn open_document(&mut self) {
        let dialog = rfd::FileDialog::new()
            .add_filter("Markdown / текст", &["md", "txt", "markdown"])
            .add_filter("Все файлы", &["*"]);
        if let Some(path) = dialog.pick_file() {
            self.open_path(path);
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
                self.file_path = Some(path);
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
            self.status_message = match result {
                Ok(()) => format!("Экспортировано: {}", path.display()),
                Err(e) => format!("Ошибка экспорта: {e}"),
            };
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

    fn autosave_tick(&mut self) {
        if !self.settings.autosave_enabled {
            return;
        }
        let interval = self.settings.autosave_interval_secs.max(5);
        if self.last_autosave.elapsed().as_secs() >= interval {
            self.last_autosave = Instant::now();
            if self.doc_modified || !self.markdown.is_empty() {
                let session = SessionState {
                    markdown: self.markdown.clone(),
                    profile: Some(self.profile),
                    file_path: self.file_path.clone(),
                };
                let ok = self.storage.save_autosave(&self.markdown).is_ok()
                    && self.storage.save_session(&session).is_ok();
                self.autosave_status =
                    if ok { "автосохранено".to_string() } else { "ошибка автосохранения".to_string() };
            }
        }
    }

    fn save_all_state(&self) {
        let _ = self.storage.save_settings(&self.settings);
        let _ = self.storage.save_recent(&self.recent);
        let _ = self.storage.save_user_templates(&self.user_templates);
        let session = SessionState {
            markdown: self.markdown.clone(),
            profile: Some(self.profile),
            file_path: self.file_path.clone(),
        };
        let _ = self.storage.save_session(&session);
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
                    self.new_document();
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
                    self.open_path(p);
                }
                if ui.button("💾 Сохранить").clicked() {
                    self.save_document();
                }
                if ui.button("📤 Экспорт").clicked() {
                    self.export_result();
                }
                if ui.button("📋 Копировать результат").clicked() {
                    self.copy_result();
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
                    let _ = self.storage.save_settings(&self.settings);
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
                            preview::show_preview(ui, &self.document, &self.settings);
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
            let changed = dialogs::show_settings_window(ctx, &mut open, &mut self.settings);
            self.show_settings = open;
            if changed {
                apply_theme(ctx, self.settings.theme);
                self.needs_convert = true;
                let _ = self.storage.save_settings(&self.settings);
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

    fn on_exit(&mut self) {
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
