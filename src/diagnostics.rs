//! Модуль диагностики: предупреждения и информация о преобразовании.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Info,
    Warning,
    Error,
}

impl Severity {
    #[allow(dead_code)]
    pub fn label(&self) -> &'static str {
        match self {
            Severity::Info => "INFO",
            Severity::Warning => "WARN",
            Severity::Error => "ERROR",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Diagnostic {
    pub severity: Severity,
    pub message: String,
}

impl Diagnostic {
    pub fn info(msg: impl Into<String>) -> Self {
        Self { severity: Severity::Info, message: msg.into() }
    }
    pub fn warn(msg: impl Into<String>) -> Self {
        Self { severity: Severity::Warning, message: msg.into() }
    }
    #[allow(dead_code)]
    pub fn error(msg: impl Into<String>) -> Self {
        Self { severity: Severity::Error, message: msg.into() }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Diagnostics {
    pub items: Vec<Diagnostic>,
}

impl Diagnostics {
    pub fn push(&mut self, d: Diagnostic) {
        // не дублируем одинаковые сообщения
        if !self.items.iter().any(|x| x.message == d.message) {
            self.items.push(d);
        }
    }

    pub fn extend(&mut self, other: Diagnostics) {
        for d in other.items {
            self.push(d);
        }
    }

    pub fn warnings(&self) -> usize {
        self.items
            .iter()
            .filter(|d| d.severity == Severity::Warning || d.severity == Severity::Error)
            .count()
    }
}
