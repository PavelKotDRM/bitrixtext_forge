//! Интеграционные и smoke-тесты: полный конвейер Markdown → BBCode,
//! GUI-логика preview, объёмные документы, Unicode.

use bitrixtext_forge::app::preview::show_preview;
use bitrixtext_forge::model::BlockNode;
use bitrixtext_forge::parser::parse_markdown;
use bitrixtext_forge::profiles::{ProfileKind, RenderOptions};
use bitrixtext_forge::render::render;
use bitrixtext_forge::settings::AppSettings;

fn convert(md: &str, profile: ProfileKind) -> String {
    let doc = parse_markdown(md).document;
    render(&doc, profile, &RenderOptions::default()).output
}

#[test]
fn trailing_tab_after_closing_fence_still_closes_the_code_block() {
    // Таб ПОСЛЕ ``` на закрывающей строке тоже нарушает CommonMark-закрытие фенса
    // (не только таб перед ним) — тот же класс бага, что и с ведущим табом.
    let md = "# Headers\n\n```\n# h1 Heading 8-)\nAlt-H2\n------\n```\t\n";
    let doc = parse_markdown(md).document;
    match &doc.blocks[1] {
        BlockNode::CodeBlock { code, .. } => {
            assert_eq!(code, "# h1 Heading 8-)\nAlt-H2\n------", "закрывающий фенс не распознан: {doc:#?}");
        }
        other => panic!("ожидался CodeBlock, получено {other:?}"),
    }
}

#[test]
fn tab_indented_closing_fence_still_closes_the_code_block() {
    // Реальный баг: редактор/буфер обмена иногда добавляет случайный таб перед закрывающим
    // ```. Таб раскрывается в 4 колонки — по CommonMark это уже не валидный закрывающий фенс,
    // и без нормализации весь остаток документа "проглатывается" как код без [/code].
    let md = "# Headers\n\n```\n# h1 Heading 8-)\nAlt-H2\n------\n\t```\n";
    let doc = parse_markdown(md).document;
    match &doc.blocks[1] {
        BlockNode::CodeBlock { code, .. } => {
            assert_eq!(code, "# h1 Heading 8-)\nAlt-H2\n------", "закрывающий фенс не распознан: {doc:#?}");
        }
        other => panic!("ожидался CodeBlock, получено {other:?}"),
    }
    let out = convert(md, ProfileKind::Full);
    assert_eq!(out, "[size=30][b]Headers[/b][/size]\n\n[code]\n# h1 Heading 8-)\nAlt-H2\n------\n[/code]");
}

#[test]
fn tab_indentation_inside_code_content_is_preserved() {
    // Таб внутри содержимого кода (не на строке-разделителе) не должен трогаться.
    let md = "```\nfn f() {\n\treturn 1;\n}\n```";
    let doc = parse_markdown(md).document;
    match &doc.blocks[0] {
        BlockNode::CodeBlock { code, .. } => assert!(code.contains("\treturn 1;")),
        other => panic!("ожидался CodeBlock, получено {other:?}"),
    }
}

#[test]
fn code_block_with_markdown_headings_inside_is_always_closed() {
    // Содержимое фенса — само по себе демонстрация Markdown-заголовков (ATX и setext),
    // но т.к. это код внутри ``` ``` ```, оно не должно парситься как реальные заголовки,
    // а [code] должен закрываться независимо от содержимого.
    let md = "# Headers\n\n```\n# h1 Heading 8-)\n## h2 Heading\n### h3 Heading\n#### h4 Heading\n##### h5 Heading\n###### h6 Heading\n\nAlternatively, for H1 and H2, an underline-ish style:\n\nAlt-H1\n======\n\nAlt-H2\n------\n```";
    let out = convert(md, ProfileKind::Full);
    assert!(out.starts_with("[size=30][b]Headers[/b][/size]\n\n[code]\n"), "заголовок вне кода не сохранён: {out}");
    assert!(out.trim_end().ends_with("[/code]"), "код должен закрываться [/code]: {out}");
    assert!(out.contains("# h1 Heading 8-)"), "содержимое фенса должно остаться литеральным текстом: {out}");
    assert!(!out.contains("[b]h1"), "заголовки внутри code fence не должны парситься: {out}");
}

#[test]
fn code_block_with_crlf_line_endings_is_always_closed() {
    let md = "# Headers\r\n\r\n```\r\n# h1 Heading 8-)\r\nAlt-H2\r\n------\r\n```\r\n";
    let out = convert(md, ProfileKind::Full);
    assert!(out.trim_end().ends_with("[/code]"), "код должен закрываться [/code] даже с CRLF: {out}");
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
    assert!(out.contains("[code]\nfn main() {"));
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
    assert!(res.output.lines().all(|l| l.starts_with(">>")), "код должен быть выделен >>: {}", res.output);
    assert!(res.output.contains("[color=#c678dd]def[/color]"), "ключевые слова должны подсвечиваться: {}", res.output);
}

#[test]
fn disabled_manual_highlight_still_uses_text_fallback() {
    let doc = parse_markdown("```python\nprint('hi')\n```").document;
    let opts = RenderOptions { manual_code_colors: false, ..Default::default() };
    let res = render(&doc, ProfileKind::ManualCodeHighlight, &opts);
    assert_eq!(res.output, ">>    print('hi')");
    assert!(!res.output.contains("[code]"));
    assert!(!res.output.contains("[color="));
}

#[test]
fn markdown_conversion_generates_only_allowed_tags() {
    let md = "# H\n\n**b** *i* ~~s~~ [text](https://e.com)\n\n![image](https://e.com/i.png)\n\n```\nlet x = 1;\n```";
    let out = convert(md, ProfileKind::Full);
    assert!(out.contains("[code]") && out.contains("[/code]"), "[code] should be generated by Full profile: {out}");
    assert!(out.contains("[size=30]"), "H1 should use the configured heading size: {out}");
    for tag in ["[img", "[timestamp", "[disk", "[br", "[list", "[hr", "[color", "[icon"] {
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
fn list_item_keeps_multiple_adjacent_formatted_runs() {
    let md = "* **Список** **Жирный**\n* Не жирный";
    let out = convert(md, ProfileKind::Full);

    assert_eq!(out, "• [b]Список[/b] [b]Жирный[/b]\n• Не жирный");
}

#[test]
fn indented_nested_lists_keep_structure() {
    let md = "    + Create a list by starting a line with `+`, `-`, or `*`\n    + Sub-lists are made by indenting 2 spaces:\n     - Marker character change forces new list start:\n        * Ac tristique libero volutpat at\n        + Facilisis in pretium nisl aliquet\n        - Nulla volutpat aliquam velit\n    + Very easy!";
    let plain_expected = "• Create a list by starting a line with +, -, or *\n• Sub-lists are made by indenting 2 spaces:\n    • Marker character change forces new list start:\n        • Ac tristique libero volutpat at\n        • Facilisis in pretium nisl aliquet\n        • Nulla volutpat aliquam velit\n• Very easy!";

    assert_eq!(
        convert(md, ProfileKind::Full),
        "• Create a list by starting a line with [b]+[/b], [b]-[/b], or [b]*[/b]\n• Sub-lists are made by indenting 2 spaces:\n    • Marker character change forces new list start:\n        • Ac tristique libero volutpat at\n        • Facilisis in pretium nisl aliquet\n        • Nulla volutpat aliquam velit\n• Very easy!"
    );
    assert_eq!(convert(md, ProfileKind::CoreSafe), plain_expected);
    assert_eq!(convert(md, ProfileKind::PlainText), plain_expected);
}

#[test]
fn tables_use_supported_bbcode_only() {
    let md = "| Имя | Статус |\n| --- | --- |\n| **Иван** и *Пётр* | [Готово](https://example.com/status) |";
    let doc = parse_markdown(md).document;
    let res = render(&doc, ProfileKind::Full, &RenderOptions::default());
    let out = res.output;
    assert!(out.contains("table_1.xlsx"), "table filename reference is missing: {out}");
    for tag in ["[table]", "[tr]", "[td]", "[list]", "[hr]"] {
        assert!(!out.contains(tag), "unsupported BBCode tag leaked into output: {tag}: {out}");
    }
    assert_eq!(res.tables.len(), 1, "table data should be extracted for xlsx export");
    let table = &res.tables[0];
    assert!(table.rows.iter().flatten().any(|cell| cell.contains("Иван и Пётр")));
    assert!(table.rows.iter().flatten().any(|cell| cell.contains("Готово")));

    let core_safe_out = convert(md, ProfileKind::CoreSafe);
    assert!(core_safe_out.contains("table_1.xlsx"));
    for tag in ["[b]", "[i]", "[u]", "[s]", "[url]", "[url=", "[user="] {
        assert!(!core_safe_out.contains(tag), "BBCode formatting was generated in a Core Safe table: {tag}: {core_safe_out}");
    }
}

#[test]
fn tables_are_numbered_sequentially_and_extracted_for_export() {
    let md = "| A | B |\n| --- | --- |\n| 1 | 2 |\n\n| C | D |\n| --- | --- |\n| 3 | 4 |";
    let doc = parse_markdown(md).document;
    let res = render(&doc, ProfileKind::Full, &RenderOptions::default());
    assert!(res.output.contains("table_1.xlsx"));
    assert!(res.output.contains("table_2.xlsx"));
    assert_eq!(res.tables.len(), 2);
}

