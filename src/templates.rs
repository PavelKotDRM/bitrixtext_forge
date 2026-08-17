//! Система шаблонов.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Template {
    pub name: String,
    pub category: String,
    pub markdown: String,
    /// Кэш итогового BBCode (наполняется при сохранении шаблона).
    #[serde(default)]
    pub bbcode: String,
    /// Служебные переменные вида `{{имя}}`.
    #[serde(default)]
    pub variables: Vec<String>,
    #[serde(default)]
    pub modified: String,
    /// Системный (встроенный) или пользовательский.
    #[serde(default)]
    pub system: bool,
}

impl Template {
    /// Извлекает переменные `{{...}}` из markdown.
    pub fn extract_variables(markdown: &str) -> Vec<String> {
        let mut vars = Vec::new();
        let mut rest = markdown;
        while let Some(start) = rest.find("{{") {
            if let Some(end) = rest[start + 2..].find("}}") {
                let name = rest[start + 2..start + 2 + end].trim().to_string();
                if !name.is_empty() && !vars.contains(&name) {
                    vars.push(name);
                }
                rest = &rest[start + 2 + end + 2..];
            } else {
                break;
            }
        }
        vars
    }
}

/// Встроенные (системные) шаблоны по требованиям ТЗ.
pub fn builtin_templates() -> Vec<Template> {
    let t = |name: &str, category: &str, markdown: &str| Template {
        name: name.to_string(),
        category: category.to_string(),
        markdown: markdown.to_string(),
        bbcode: String::new(),
        variables: Template::extract_variables(markdown),
        modified: String::new(),
        system: true,
    };

    vec![
        t(
            "Важное сообщение",
            "Сообщения",
            "# Важно!\n\n**{{тема}}**\n\nКоллеги, обратите внимание: {{текст}}\n\n[color=#ff0000]Срок: {{срок}}[/color]",
        ),
        t(
            "Сообщение со ссылкой",
            "Сообщения",
            "{{текст}}\n\nПодробнее: [{{название ссылки}}]({{url}})",
        ),
        t(
            "Сообщение с кодовым блоком",
            "Разработка",
            "{{описание}}\n\n```{{язык}}\n{{код}}\n```\n\nКомментарии приветствуются.",
        ),
        t(
            "Сообщение с изображением",
            "Сообщения",
            "{{текст}}\n\n![{{описание изображения}}]({{url изображения}} \"medium\")",
        ),
        t("Упоминание сотрудника", "Сообщения", "[user={{id сотрудника}}]{{имя сотрудника}}[/user], {{текст}}"),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_templates_present() {
        let ts = builtin_templates();
        assert_eq!(ts.len(), 5);
        assert!(ts.iter().all(|t| t.system));
        let names: Vec<_> = ts.iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains(&"Важное сообщение"));
        assert!(names.contains(&"Упоминание сотрудника"));
    }

    #[test]
    fn variables_extracted() {
        let vars = Template::extract_variables("Привет {{имя}}, срок {{срок}} и снова {{имя}}");
        assert_eq!(vars, vec!["имя".to_string(), "срок".to_string()]);
    }

    #[test]
    fn template_roundtrip_json() {
        let t = builtin_templates().remove(0);
        let json = serde_json::to_string(&t).unwrap();
        let back: Template = serde_json::from_str(&json).unwrap();
        assert_eq!(back.name, t.name);
    }
}
