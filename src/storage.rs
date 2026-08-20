//! Модуль файлового хранения: настройки, сессия, шаблоны, автосохранение,
//! история последних файлов.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

use crate::profiles::ProfileKind;
use crate::settings::AppSettings;
use crate::templates::Template;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SessionState {
    pub markdown: String,
    pub profile: Option<ProfileKind>,
    pub file_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RecentFiles {
    pub items: Vec<PathBuf>,
}

impl RecentFiles {
    pub fn push(&mut self, path: PathBuf) {
        self.items.retain(|p| p != &path);
        self.items.insert(0, path);
        self.items.truncate(10);
    }
}

pub struct Storage {
    base_dir: PathBuf,
}

impl Storage {
    pub fn new(custom_dir: &str) -> Self {
        let base_dir = if custom_dir.trim().is_empty() {
            ProjectDirs::from("com", "BitrixTextForge", "BitrixText Forge")
                .map(|d| d.data_dir().to_path_buf())
                .unwrap_or_else(|| PathBuf::from("."))
        } else {
            PathBuf::from(custom_dir.trim())
        };
        Self { base_dir }
    }

    #[allow(dead_code)]
    pub fn base_dir(&self) -> &Path {
        &self.base_dir
    }

    fn ensure_dir(&self) -> Result<()> {
        fs::create_dir_all(&self.base_dir)
            .with_context(|| format!("Не удалось создать каталог {}", self.base_dir.display()))
    }

    fn path(&self, name: &str) -> PathBuf {
        self.base_dir.join(name)
    }

    // --- generic json helpers ---

    fn save_json<T: Serialize>(&self, name: &str, value: &T) -> Result<()> {
        self.ensure_dir()?;
        let json = serde_json::to_string_pretty(value)?;
        fs::write(self.path(name), json)
            .with_context(|| format!("Не удалось записать {}", name))
    }

    fn load_json<T: for<'de> Deserialize<'de>>(&self, name: &str) -> Result<Option<T>> {
        let p = self.path(name);
        if !p.exists() {
            return Ok(None);
        }
        let data = fs::read_to_string(&p)
            .with_context(|| format!("Не удалось прочитать {}", p.display()))?;
        Ok(Some(serde_json::from_str(&data)
            .with_context(|| format!("Некорректный JSON в {}", p.display()))?))
    }

    // --- settings ---

    pub fn load_settings(&self) -> Result<AppSettings> {
        Ok(self.load_json::<AppSettings>("settings.json")?.unwrap_or_default())
    }

    pub fn save_settings(&self, s: &AppSettings) -> Result<()> {
        self.save_json("settings.json", s)
    }

    // --- session / autosave ---

    pub fn load_session(&self) -> Result<Option<SessionState>> {
        self.load_json("session.json")
    }

    pub fn save_session(&self, s: &SessionState) -> Result<()> {
        self.save_json("session.json", s)
    }

    pub fn save_autosave(&self, markdown: &str) -> Result<()> {
        self.ensure_dir()?;
        fs::write(self.path("autosave.md"), markdown).context("Не удалось записать автосохранение")
    }

    // --- templates ---

    pub fn load_user_templates(&self) -> Result<Vec<Template>> {
        Ok(self.load_json::<Vec<Template>>("templates.json")?.unwrap_or_default())
    }

    pub fn save_user_templates(&self, templates: &[Template]) -> Result<()> {
        self.save_json("templates.json", &templates)
    }

    // --- recent files ---

    pub fn load_recent(&self) -> Result<RecentFiles> {
        Ok(self.load_json::<RecentFiles>("recent.json")?.unwrap_or_default())
    }

    pub fn save_recent(&self, r: &RecentFiles) -> Result<()> {
        self.save_json("recent.json", r)
    }
}

// --- документы пользователя ---

pub fn read_document(path: &Path) -> Result<String> {
    fs::read_to_string(path).with_context(|| format!("Не удалось открыть {}", path.display()))
}

pub fn write_document(path: &Path, content: &str) -> Result<()> {
    fs::write(path, content).with_context(|| format!("Не удалось сохранить {}", path.display()))
}

/// Экспорт результата в JSON с исходником и выводом.
#[derive(Debug, Serialize)]
pub struct ExportJson<'a> {
    pub markdown: &'a str,
    pub bbcode: &'a str,
    pub profile: &'a str,
}

pub fn export_json(path: &Path, markdown: &str, bbcode: &str, profile: &str) -> Result<()> {
    let data = serde_json::to_string_pretty(&ExportJson { markdown, bbcode, profile })?;
    fs::write(path, data).with_context(|| format!("Не удалось экспортировать {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_storage() -> (Storage, PathBuf) {
        use std::sync::atomic::{AtomicU32, Ordering};
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("btf_test_{}_{}", std::process::id(), n));
        let _ = fs::remove_dir_all(&dir);
        (Storage::new(dir.to_str().unwrap()), dir)
    }

    #[test]
    fn settings_roundtrip() {
        let (s, dir) = temp_storage();
        let cfg = AppSettings { autosave_interval_secs: 99, ..Default::default() };
        s.save_settings(&cfg).unwrap();
        let loaded = s.load_settings().unwrap();
        assert_eq!(loaded.autosave_interval_secs, 99);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn session_roundtrip() {
        let (s, dir) = temp_storage();
        let sess = SessionState {
            markdown: "# Привет".into(),
            profile: Some(ProfileKind::CoreSafe),
            file_path: None,
        };
        s.save_session(&sess).unwrap();
        let loaded = s.load_session().unwrap().unwrap();
        assert_eq!(loaded.markdown, "# Привет");
        assert_eq!(loaded.profile, Some(ProfileKind::CoreSafe));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn recent_files_dedup_and_limit() {
        let mut r = RecentFiles::default();
        for i in 0..15 {
            r.push(PathBuf::from(format!("f{i}.md")));
        }
        r.push(PathBuf::from("f14.md"));
        assert_eq!(r.items.len(), 10);
        assert_eq!(r.items[0], PathBuf::from("f14.md"));
    }

    #[test]
    fn missing_settings_returns_default() {
        let (s, dir) = temp_storage();
        let cfg = s.load_settings().unwrap();
        assert!(cfg.auto_convert);
        let _ = fs::remove_dir_all(dir);
    }
}
