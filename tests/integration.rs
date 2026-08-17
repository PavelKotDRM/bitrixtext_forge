//! Интеграционные и smoke-тесты: полный конвейер Markdown → BBCode,
//! GUI-логика preview, объёмные документы, Unicode.

use bitrixtext_forge::app::preview::show_preview;
use bitrixtext_forge::parser::parse_markdown;
use bitrixtext_forge::profiles::{ProfileKind, RenderOptions};
use bitrixtext_forge::render::render;
use bitrixtext_forge::settings::AppSettings;

fn convert(md: &str, profile: ProfileKind) -> String {
    let doc = parse_markdown(md).document;
    render(&doc, profile, &RenderOptions::default()).output
}

#[test]
fn full_pipeline_complex_document() {
    let md = "\
# Отчёт за неделю

Привет, **команда**! Вот *краткие* итоги:

- Закрыто ~~15~~ **17** задач
- Выпущен релиз [v2.1](https://example.com/release)

## Код

```rust
fn main() {
    println!(\"Привет, Bitrix24!\");
}
```

> Отличная работа!

---

![скриншот](https://example.com/shot.png \"large\")";

    let out = convert(md, ProfileKind::Full);
    assert!(out.contains("[b]Отчёт за неделю[/b]"));
    assert!(out.contains("[b]команда[/b]"));
    assert!(out.contains("[i]краткие[/i]"));
    assert!(out.contains("[s]15[/s]"));
    assert!(out.contains("[url=https://example.com/release]v2.1[/url]"));
    assert!(out.contains("    fn main() {"));
    assert!(out.contains(">>Отличная работа!"));
    assert!(out.contains("--------------------"));
    assert!(out.contains("[url=https://example.com/shot.png]скриншот[/url]"));
    assert!(out.contains("• Закрыто"));
}

#[test]
fn safe_profile_has_no_extended_tags() {
    let md = "# T\n\n[color=#f00]x[/color] [size=20]y[/size]\n\n![i](https://e.com/i.png)\n\n[icon=https://e.com/ic.png]";
    let out = convert(md, ProfileKind::CoreSafe);
    for tag in ["[color", "[size", "[img", "[code", "[icon"] {
        assert!(!out.contains(tag), "safe profile must not contain {tag}: {out}");
    }
}

#[test]
fn plain_profile_has_no_bbcode_at_all() {
    let md = "# T\n\n**b** [u]u[/u] [ссылка](https://e.com)\n\n```\ncode\n```";
    let out = convert(md, ProfileKind::PlainText);
    assert!(!out.contains('['), "plain text must not contain BBCode: {out}");
}

#[test]
fn excluded_tags_never_appear_as_generated_markup() {
    // приложение никогда не генерирует исключённые теги
    let md = "обычный **текст** и [ссылка](https://e.com)";
    for profile in ProfileKind::ALL {
        let out = convert(md, profile);
        for tag in ["[send]", "[call]", "[put]", "[context]", "[user]", "[chat]"] {
            assert!(!out.contains(tag));
        }
    }
}

#[test]
fn handles_100kb_document() {
    let paragraph = "Строка с **жирным**, *курсивом* и [ссылкой](https://example.com/path).\n\n";
    let mut md = String::new();
    while md.len() < 120_000 {
        md.push_str(paragraph);
    }
    let start = std::time::Instant::now();
    let out = convert(&md, ProfileKind::Full);
    assert!(!out.is_empty());
    assert!(
        start.elapsed().as_secs() < 5,
        "конвертация 100+ KB должна быть быстрой, заняла {:?}",
        start.elapsed()
    );
}

#[test]
fn unicode_mixed_languages() {
    let md = "Привет **мир**! Hello *world*! 你好 ~~世界~~! Émoji: 🚀";
    let out = convert(md, ProfileKind::Full);
    assert!(out.contains("[b]мир[/b]"));
    assert!(out.contains("[i]world[/i]"));
    assert!(out.contains("[s]世界[/s]"));
    assert!(out.contains("🚀"));
}

#[test]
fn code_fallback_never_generates_code_tag() {
    let doc = parse_markdown("```python\ndef f():\n    return 'hi'\n```").document;
    let res = render(&doc, ProfileKind::ManualCodeHighlight, &RenderOptions::default());
    assert!(!res.output.contains("[code]"), "не должно быть [code]: {}", res.output);
    assert_eq!(res.output, "    def f():\n        return 'hi'");
}

#[test]
fn disabled_manual_highlight_still_uses_text_fallback() {
    let doc = parse_markdown("```python\nprint('hi')\n```").document;
    let opts = RenderOptions { manual_code_colors: false, ..Default::default() };
    let res = render(&doc, ProfileKind::ManualCodeHighlight, &opts);
    assert_eq!(res.output, "    print('hi')");
    assert!(!res.output.contains("[code]"));
}

#[test]
fn markdown_conversion_generates_only_allowed_tags() {
    let md = "# H\n\n**b** *i* ~~s~~ [text](https://e.com)\n\n![image](https://e.com/i.png)\n\n```\nlet x = 1;\n```";
    let out = convert(md, ProfileKind::Full);
    for tag in ["[code", "[img", "[timestamp", "[disk", "[br", "[list", "[hr", "[color", "[size", "[icon"] {
        assert!(!out.contains(tag), "unsupported tag was generated: {tag}: {out}");
    }
}

#[test]
fn gui_smoke_preview_renders_without_panic() {
    let md = "# Заголовок\n\n**жирный** [u]подчёркнутый[/u] `код`\n\n```rust\nlet x = 1; // комментарий\n```\n\n> цитата\n\n- пункт 1\n- пункт 2\n\n[timestamp=1700000000 format=DD.MM.YYYY] [disk=7]\n\n![img](https://e.com/i.png \"small\")";
    let settings = AppSettings::default();
    let output = convert(md, ProfileKind::Full);

    let ctx = egui::Context::default();
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
        show_preview(ui, &output, &settings);
    });
    output.textures_delta.clear();
}

#[test]
fn newline_style_smoke() {
    let doc = parse_markdown("а\nб").document;
    let nl = render(&doc, ProfileKind::Full, &RenderOptions::default()).output;
    assert_eq!(nl, "а\nб");
}

#[test]
fn task_lists_and_nested_markdown_keep_structure() {
    let md = "- [ ] **Подготовить** [документ](https://example.com/doc)\n  - [x] `проверить`";
    let out = convert(md, ProfileKind::Full);

    assert_eq!(
        out,
        "• [ ] [b]Подготовить[/b] [url=https://example.com/doc]документ[/url]\n    • [x] [b]проверить[/b]"
    );
}

#[test]
fn tables_use_supported_bbcode_only() {
    let md = "| Имя | Статус |\n| --- | --- |\n| **Иван** и *Пётр* | [Готово](https://example.com/status) |";
    let out = convert(md, ProfileKind::Full);
    assert!(out.starts_with("[code]") && out.ends_with("[/code]"), "table is not wrapped in [code]: {out}");
    assert!(out.contains("Иван и Пётр"), "table text is missing: {out}");
    assert!(out.contains("Готово"), "table link text is missing: {out}");
    for tag in ["[b]", "[i]", "[u]", "[s]", "[url]", "[url=", "[user=", "[table]", "[tr]", "[td]", "[list]", "[hr]"] {
        assert!(!out.contains(tag), "BBCode formatting was generated in a table: {tag}: {out}");
    }

    let core_safe_out = convert(md, ProfileKind::CoreSafe);
    for tag in ["[b]", "[i]", "[u]", "[s]", "[url]", "[url=", "[user="] {
        assert!(!core_safe_out.contains(tag), "BBCode formatting was generated in a Core Safe table: {tag}: {core_safe_out}");
    }
}

#[test]
fn tables_align_all_rows_by_bbcode_width() {
    let md = "| Command | Description |\n| --- | --- |\n| git status | List all new or modified files |\n| git diff | Show file differences that haven't been staged |";
    let out = convert(md, ProfileKind::Full);
    let table = out
        .strip_prefix("[code]")
        .and_then(|text| text.strip_suffix("[/code]"))
        .expect("table must be wrapped in [code]");
    assert!(out.contains("Show file differences that haven't"), "long description was not wrapped: {out}");
    assert!(out.contains("been staged"), "long description tail is missing: {out}");
    assert!(table.lines().all(|line| line.starts_with('+') || line.starts_with('|')), "table structure is malformed: {out}");
}
