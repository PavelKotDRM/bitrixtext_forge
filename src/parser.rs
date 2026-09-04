//! Markdown parser: Markdown → внутренняя AST-модель.
//!
//! Использует `pulldown-cmark` для разбора Markdown и дополнительный сканер
//! спецвставок Bitrix24 (`[u]`, `[user]`, `[color]`, `[size]`, `[icon]`),
//! которые не имеют Markdown-аналога и вставляются через GUI-команды.

use std::borrow::Cow;

use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};

use crate::diagnostics::{Diagnostic, Diagnostics};
use crate::model::{
    BlockNode, Document, EXCLUDED_TAGS, ImageSize, InlineNode, TableAlignment, is_valid_size,
    normalize_hex_color,
};

pub struct ParseResult {
    pub document: Document,
    pub diagnostics: Diagnostics,
}

enum Container {
    Root(Vec<BlockNode>),
    Quote(Vec<BlockNode>),
    List { ordered: bool, start: u64, items: Vec<Vec<BlockNode>> },
    Item(Vec<BlockNode>),
}

impl Container {
    fn push_block(&mut self, b: BlockNode) {
        match self {
            Container::Root(v) | Container::Quote(v) | Container::Item(v) => v.push(b),
            Container::List { items, .. } => {
                // защитный случай: блок вне Item — заводим отдельный элемент
                items.push(vec![b]);
            }
        }
    }
}

enum InlineFrame {
    Paragraph,
    Heading(u8),
    Bold,
    Italic,
    Underline,
    Strike,
    Link(String),
    Image { url: String, title: String },
}

enum HtmlInlineTag {
    Bold,
    Italic,
    Underline,
    Strike,
    Link,
}

struct TableBuilder {
    rows: Vec<Vec<Vec<InlineNode>>>,
    current_row: Vec<Vec<InlineNode>>,
    alignments: Vec<TableAlignment>,
}

/// Символ и длина ведущей серии `` ` ``/`~` в начале строки (>= 3), если такая есть —
/// используется для отслеживания ОТКРЫВАЮЩЕЙ строки фенса (после серии может идти info-string).
fn fence_open_char_and_len(rest: &str) -> Option<(char, usize)> {
    let c = rest.chars().next()?;
    if c != '`' && c != '~' {
        return None;
    }
    let n = rest.chars().take_while(|&ch| ch == c).count();
    (n >= 3).then_some((c, n))
}

/// Если `rest` (без учёта конца строки) — валидный, с точностью до пробелов/табов вокруг,
/// ЗАКРЫВАЮЩИЙ фенс для уже открытого блока (символ `want_char`, длина не меньше `want_len`),
/// возвращает фактическую длину серии символов.
fn fence_close_run_len(rest: &str, want_char: char, want_len: usize) -> Option<usize> {
    let run = rest.chars().take_while(|&ch| ch == want_char).count();
    if run < want_len {
        return None;
    }
    rest[run..].chars().all(|ch| ch == ' ' || ch == '\t').then_some(run)
}

/// CommonMark закрывает `` ``` ``/`~~~` только если у закрывающей строки отступ не более
/// 3 колонок и после серии символов идут только пробелы; таб (таб-стоп 4) нарушает оба этих
/// условия и делает блок «незакрытым» — остаток документа проглатывается как код без видимого
/// закрывающего тега. Такой таб чаще попадает из буфера обмена/редактора с автоотступом, чем
/// пишется осознанно, поэтому здесь мы «долечиваем» именно закрывающую строку уже открытого
/// фенса — убираем пробелы/табы вокруг серии символов, если среди них встретился таб.
fn normalize_tab_indented_fence_closers(input: &str) -> Cow<'_, str> {
    let mut open_fence: Option<(char, usize)> = None;
    let mut changed = false;
    let mut out = String::with_capacity(input.len());

    for line in input.split_inclusive('\n') {
        let content = line.trim_end_matches(['\r', '\n']);
        let eol = &line[content.len()..];
        let ws_len: usize =
            content.chars().take_while(|c| *c == ' ' || *c == '\t').map(|c| c.len_utf8()).sum();
        let (ws, rest) = content.split_at(ws_len);

        match open_fence {
            None => {
                if let Some((c, n)) = fence_open_char_and_len(rest) {
                    open_fence = Some((c, n));
                }
                out.push_str(line);
            }
            Some((open_c, open_n)) => {
                if let Some(run) = fence_close_run_len(rest, open_c, open_n) {
                    open_fence = None;
                    if ws.contains('\t') || rest[run..].contains('\t') {
                        changed = true;
                        out.push_str(&rest[..run]);
                        out.push_str(eol);
                    } else {
                        out.push_str(line);
                    }
                } else {
                    out.push_str(line);
                }
            }
        }
    }

    if changed { Cow::Owned(out) } else { Cow::Borrowed(input) }
}

/// Редакторы и примеры в документации иногда добавляют четыре пробела ко всему фрагменту
/// списка. В CommonMark это превращает его в кодовый блок. Если строка явно начинает
/// такой список, снимаем общий отступ в четыре пробела у всего непрерывного блока.
fn normalize_indented_list_blocks(input: &str) -> Cow<'_, str> {
    let mut changed = false;
    let mut in_list_block = false;
    let mut at_block_start = true;
    let mut out = String::with_capacity(input.len());

    for line in input.split_inclusive('\n') {
        let content = line.trim_end_matches(['\r', '\n']);
        let eol = &line[content.len()..];
        let trimmed = content.trim_start_matches(' ');
        let indent = content.len() - trimmed.len();
        let is_list_item = matches!(trimmed.as_bytes(), [b'-' | b'+' | b'*', b' ', ..]);

        if at_block_start && is_list_item && indent >= 4 {
            in_list_block = true;
        }

        if !in_list_block {
            out.push_str(line);
            at_block_start = content.trim().is_empty();
            continue;
        }

        if in_list_block && (content.trim().is_empty() || indent >= 4) {
            let stripped = content.strip_prefix("    ").unwrap_or(content);
            let normalized = if is_list_item && indent == 5 {
                format!("  {}", stripped.trim_start_matches(' '))
            } else {
                stripped.to_string()
            };
            changed |= normalized != content;
            out.push_str(&normalized);
            out.push_str(eol);
            at_block_start = false;
        } else {
            in_list_block = false;
            out.push_str(line);
            at_block_start = content.trim().is_empty();
        }
    }

    if changed { Cow::Owned(out) } else { Cow::Borrowed(input) }
}

pub fn parse_markdown(input: &str) -> ParseResult {
    let input = normalize_tab_indented_fence_closers(input);
    let input = normalize_indented_list_blocks(input.as_ref());
    let input = input.as_ref();
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_TASKLISTS);
    opts.insert(Options::ENABLE_TABLES);

    let parser = Parser::new_ext(input, opts);

    let mut diags = Diagnostics::default();
    let mut containers: Vec<Container> = vec![Container::Root(Vec::new())];
    // стек inline-фреймов; каждый со своим буфером узлов
    let mut inline_stack: Vec<(InlineFrame, Vec<InlineNode>)> = Vec::new();
    let mut code_block: Option<(Option<String>, String)> = None;
    let mut table: Option<TableBuilder> = None;

    let ensure_implicit_paragraph = |stack: &mut Vec<(InlineFrame, Vec<InlineNode>)>| {
        if stack.is_empty() {
            stack.push((InlineFrame::Paragraph, Vec::new()));
        }
    };

    fn flush_implicit(
        stack: &mut Vec<(InlineFrame, Vec<InlineNode>)>,
        containers: &mut [Container],
        diags: &mut Diagnostics,
    ) {
        if stack.len() == 1
            && let Some((InlineFrame::Paragraph, nodes)) = stack.pop()
        {
            let nodes = postprocess_inlines(nodes, diags);
            if !nodes.is_empty() {
                containers
                    .last_mut()
                    .expect("root container")
                    .push_block(BlockNode::Paragraph(nodes));
            }
        }
    }

    for event in parser {
        match event {
            Event::Start(tag) => match tag {
                Tag::Paragraph => {
                    inline_stack.push((InlineFrame::Paragraph, Vec::new()));
                }
                Tag::Heading { level, .. } => {
                    inline_stack.push((InlineFrame::Heading(level as u8), Vec::new()));
                }
                Tag::Emphasis => {
                    ensure_implicit_paragraph(&mut inline_stack);
                    inline_stack.push((InlineFrame::Italic, Vec::new()));
                }
                Tag::Strong => {
                    ensure_implicit_paragraph(&mut inline_stack);
                    inline_stack.push((InlineFrame::Bold, Vec::new()));
                }
                Tag::Strikethrough => {
                    ensure_implicit_paragraph(&mut inline_stack);
                    inline_stack.push((InlineFrame::Strike, Vec::new()));
                }
                Tag::Link { dest_url, .. } => {
                    ensure_implicit_paragraph(&mut inline_stack);
                    inline_stack.push((InlineFrame::Link(dest_url.to_string()), Vec::new()));
                }
                Tag::Image { dest_url, title, .. } => {
                    ensure_implicit_paragraph(&mut inline_stack);
                    inline_stack.push((
                        InlineFrame::Image { url: dest_url.to_string(), title: title.to_string() },
                        Vec::new(),
                    ));
                }
                Tag::CodeBlock(kind) => {
                    flush_implicit(&mut inline_stack, &mut containers, &mut diags);
                    let lang = match kind {
                        CodeBlockKind::Fenced(info) => {
                            let l = info.trim().to_string();
                            if l.is_empty() { None } else { Some(l) }
                        }
                        CodeBlockKind::Indented => None,
                    };
                    code_block = Some((lang, String::new()));
                }
                Tag::BlockQuote(_) => {
                    flush_implicit(&mut inline_stack, &mut containers, &mut diags);
                    containers.push(Container::Quote(Vec::new()));
                }
                Tag::List(start) => {
                    flush_implicit(&mut inline_stack, &mut containers, &mut diags);
                    containers.push(Container::List {
                        ordered: start.is_some(),
                        start: start.unwrap_or(1),
                        items: Vec::new(),
                    });
                }
                Tag::Item => {
                    containers.push(Container::Item(Vec::new()));
                }
                Tag::Table(alignments) => {
                    flush_implicit(&mut inline_stack, &mut containers, &mut diags);
                    table = Some(TableBuilder {
                        rows: Vec::new(),
                        current_row: Vec::new(),
                        alignments: alignments
                            .iter()
                            .map(|alignment| match alignment {
                                pulldown_cmark::Alignment::Center => TableAlignment::Center,
                                pulldown_cmark::Alignment::Right => TableAlignment::Right,
                                _ => TableAlignment::Left,
                            })
                            .collect(),
                    });
                }
                Tag::TableRow => {
                    if let Some(table) = table.as_mut() {
                        table.current_row.clear();
                    }
                }
                Tag::TableCell => {
                    inline_stack.push((InlineFrame::Paragraph, Vec::new()));
                }
                _ => {}
            },
            Event::End(tag_end) => match tag_end {
                TagEnd::Paragraph => {
                    if let Some((_, nodes)) = inline_stack.pop() {
                        let nodes = postprocess_inlines(nodes, &mut diags);
                        push_paragraph(&mut containers, nodes);
                    }
                }
                TagEnd::Heading(_) => {
                    if let Some((frame, nodes)) = inline_stack.pop() {
                        let nodes = postprocess_inlines(nodes, &mut diags);
                        let level = match frame {
                            InlineFrame::Heading(l) => l,
                            _ => 1,
                        };
                        containers
                            .last_mut()
                            .expect("container")
                            .push_block(BlockNode::Heading { level, content: nodes });
                    }
                }
                TagEnd::Emphasis => {
                    close_inline(&mut inline_stack, &mut diags, InlineNode::Italic)
                }
                TagEnd::Strong => {
                    close_inline(&mut inline_stack, &mut diags, InlineNode::Bold)
                }
                TagEnd::Strikethrough => {
                    close_inline(&mut inline_stack, &mut diags, InlineNode::Strike)
                }
                TagEnd::Link => {
                    if let Some((frame, nodes)) = inline_stack.pop() {
                        let nodes = postprocess_inlines(nodes, &mut diags);
                        let url = match frame {
                            InlineFrame::Link(u) => u,
                            _ => String::new(),
                        };
                        append_inline(&mut inline_stack, InlineNode::Link { text: nodes, url });
                    }
                }
                TagEnd::Image => {
                    if let Some((InlineFrame::Image { url, title }, nodes)) = inline_stack.pop() {
                        let alt = plain_text_of(&nodes);
                        // размер можно указать в title: ![alt](url "medium")
                        let size = ImageSize::parse(&title);
                        append_inline(&mut inline_stack, InlineNode::Image { url, alt, size });
                    }
                }
                TagEnd::CodeBlock => {
                    if let Some((language, mut code)) = code_block.take() {
                        if code.ends_with('\n') {
                            code.pop();
                        }
                        containers
                            .last_mut()
                            .expect("container")
                            .push_block(BlockNode::CodeBlock { language, code });
                    }
                }
                TagEnd::BlockQuote(_) => {
                    flush_implicit(&mut inline_stack, &mut containers, &mut diags);
                    if let Some(Container::Quote(blocks)) = containers.pop() {
                        containers.last_mut().expect("container").push_block(BlockNode::Quote(blocks));
                    }
                }
                TagEnd::List(_) => {
                    if let Some(Container::List { ordered, start, items }) = containers.pop() {
                        containers
                            .last_mut()
                            .expect("container")
                            .push_block(BlockNode::List { ordered, start, items });
                    }
                }
                TagEnd::Item => {
                    flush_implicit(&mut inline_stack, &mut containers, &mut diags);
                    if let Some(Container::Item(blocks)) = containers.pop()
                        && let Some(Container::List { items, .. }) = containers.last_mut()
                    {
                        items.push(blocks);
                    }
                }
                TagEnd::TableCell => {
                    if let (Some(table), Some((_, nodes))) = (table.as_mut(), inline_stack.pop()) {
                        table.current_row.push(postprocess_inlines(nodes, &mut diags));
                    }
                }
                TagEnd::TableRow => {
                    if let Some(table) = table.as_mut()
                        && !table.current_row.is_empty()
                    {
                        table.rows.push(std::mem::take(&mut table.current_row));
                    }
                }
                TagEnd::TableHead => {
                    if let Some(table) = table.as_mut()
                        && !table.current_row.is_empty()
                    {
                        table.rows.push(std::mem::take(&mut table.current_row));
                    }
                }
                TagEnd::Table => {
                    if let Some(table) = table.take() {
                        containers
                            .last_mut()
                            .expect("container")
                            .push_block(BlockNode::Table { rows: table.rows, alignments: table.alignments });
                    }
                }
                _ => {}
            },
            Event::Text(t) => {
                if let Some((_, buf)) = code_block.as_mut() {
                    buf.push_str(&t);
                } else {
                    ensure_implicit_paragraph(&mut inline_stack);
                    // сырой текст; сканер спецвставок отработает после слияния прогонов
                    append_inline(&mut inline_stack, InlineNode::Text(t.to_string()));
                }
            }
            Event::Code(t) => {
                ensure_implicit_paragraph(&mut inline_stack);
                append_inline(&mut inline_stack, InlineNode::Code(t.to_string()));
            }
            Event::SoftBreak => {
                ensure_implicit_paragraph(&mut inline_stack);
                append_inline(&mut inline_stack, InlineNode::SoftBreak);
            }
            Event::HardBreak => {
                ensure_implicit_paragraph(&mut inline_stack);
                append_inline(&mut inline_stack, InlineNode::HardBreak);
            }
            Event::Rule => {
                flush_implicit(&mut inline_stack, &mut containers, &mut diags);
                containers.last_mut().expect("container").push_block(BlockNode::HorizontalRule);
            }
            Event::Html(t) | Event::InlineHtml(t) => {
                match parse_inline_html_tag(&t) {
                    Some(HtmlToken::Start(frame)) => {
                        ensure_implicit_paragraph(&mut inline_stack);
                        inline_stack.push((frame, Vec::new()));
                    }
                    Some(HtmlToken::End(tag)) => close_html_inline(&mut inline_stack, &mut diags, tag),
                    Some(HtmlToken::Break) => {
                        ensure_implicit_paragraph(&mut inline_stack);
                        append_inline(&mut inline_stack, InlineNode::HardBreak);
                    }
                    None => {
                        diags.push(Diagnostic::warn(
                            "HTML-тег не имеет безопасного BBCode-эквивалента и передан как обычный текст.",
                        ));
                        ensure_implicit_paragraph(&mut inline_stack);
                        append_inline(&mut inline_stack, InlineNode::Text(t.to_string()));
                    }
                }
            }
            Event::TaskListMarker(checked) => {
                ensure_implicit_paragraph(&mut inline_stack);
                let mark = if checked { "[x] " } else { "[ ] " };
                append_inline(&mut inline_stack, InlineNode::Text(mark.to_string()));
            }
            _ => {}
        }
    }

    // остаточный implicit-параграф
    flush_implicit(&mut inline_stack, &mut containers, &mut diags);

    let blocks = match containers.pop() {
        Some(Container::Root(v)) => v,
        _ => Vec::new(),
    };

    ParseResult { document: Document { blocks }, diagnostics: diags }
}

fn push_paragraph(containers: &mut [Container], nodes: Vec<InlineNode>) {
    if nodes.is_empty() {
        return;
    }
    // одиночное изображение в абзаце поднимаем до блочного узла
    if nodes.len() == 1
        && let InlineNode::Image { url, alt, size } = &nodes[0]
    {
        containers.last_mut().expect("container").push_block(BlockNode::Image {
            url: url.clone(),
            alt: alt.clone(),
            size: *size,
        });
        return;
    }
    containers.last_mut().expect("container").push_block(BlockNode::Paragraph(nodes));
}

fn close_inline(
    stack: &mut Vec<(InlineFrame, Vec<InlineNode>)>,
    diags: &mut Diagnostics,
    wrap: impl FnOnce(Vec<InlineNode>) -> InlineNode,
) {
    if let Some((_, nodes)) = stack.pop() {
        let nodes = postprocess_inlines(nodes, diags);
        append_inline(stack, wrap(nodes));
    }
}

enum HtmlToken {
    Start(InlineFrame),
    End(HtmlInlineTag),
    Break,
}

fn parse_inline_html_tag(input: &str) -> Option<HtmlToken> {
    let tag = input.trim();
    let body = tag.strip_prefix('<')?.strip_suffix('>')?.trim();
    if body.starts_with('!') || body.starts_with('?') {
        return None;
    }

    let closing = body.strip_prefix('/').map(str::trim);
    let name_and_attrs = closing.unwrap_or(body).trim_end_matches('/').trim();
    let name_end = name_and_attrs.find(char::is_whitespace).unwrap_or(name_and_attrs.len());
    let name = name_and_attrs[..name_end].to_ascii_lowercase();

    if closing.is_some() {
        return match name.as_str() {
            "b" | "strong" => Some(HtmlToken::End(HtmlInlineTag::Bold)),
            "i" | "em" => Some(HtmlToken::End(HtmlInlineTag::Italic)),
            "u" => Some(HtmlToken::End(HtmlInlineTag::Underline)),
            "s" | "del" | "strike" => Some(HtmlToken::End(HtmlInlineTag::Strike)),
            "a" => Some(HtmlToken::End(HtmlInlineTag::Link)),
            _ => None,
        };
    }

    match name.as_str() {
        "br" => Some(HtmlToken::Break),
        "b" | "strong" => Some(HtmlToken::Start(InlineFrame::Bold)),
        "i" | "em" => Some(HtmlToken::Start(InlineFrame::Italic)),
        "u" => Some(HtmlToken::Start(InlineFrame::Underline)),
        "s" | "del" | "strike" => Some(HtmlToken::Start(InlineFrame::Strike)),
        "a" => html_attribute(name_and_attrs, "href")
            .map(|url| HtmlToken::Start(InlineFrame::Link(url.to_string()))),
        _ => None,
    }
}

fn html_attribute<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let lower = tag.to_ascii_lowercase();
    let needle = format!("{name}=");
    let start = lower.find(&needle)? + needle.len();
    let value = &tag[start..];
    let quote = value.chars().next()?;
    if quote == '\'' || quote == '"' {
        let end = value[1..].find(quote)? + 1;
        Some(&value[1..end])
    } else {
        Some(value.split_whitespace().next().unwrap_or_default())
    }
}

fn close_html_inline(
    stack: &mut Vec<(InlineFrame, Vec<InlineNode>)>,
    diags: &mut Diagnostics,
    tag: HtmlInlineTag,
) {
    let Some((frame, nodes)) = stack.pop() else {
        return;
    };
    let nodes = postprocess_inlines(nodes, diags);
    let node = match (tag, frame) {
        (HtmlInlineTag::Bold, InlineFrame::Bold) => InlineNode::Bold(nodes),
        (HtmlInlineTag::Italic, InlineFrame::Italic) => InlineNode::Italic(nodes),
        (HtmlInlineTag::Underline, InlineFrame::Underline) => InlineNode::Underline(nodes),
        (HtmlInlineTag::Strike, InlineFrame::Strike) => InlineNode::Strike(nodes),
        (HtmlInlineTag::Link, InlineFrame::Link(url)) => InlineNode::Link { text: nodes, url },
        (_, frame) => {
            diags.push(Diagnostic::warn("Некорректно вложенный HTML-тег передан как обычный текст."));
            append_inline(stack, InlineNode::Text(plain_text_of(&nodes)));
            match frame {
                InlineFrame::Link(url) => InlineNode::Text(url),
                _ => return,
            }
        }
    };
    append_inline(stack, node);
}

/// Сливает соседние текстовые прогоны и прогоняет их через сканер
/// спецвставок Bitrix24 (pulldown-cmark дробит `[u]…` на несколько событий).
fn postprocess_inlines(nodes: Vec<InlineNode>, diags: &mut Diagnostics) -> Vec<InlineNode> {
    let mut out = Vec::new();
    let mut buf = String::new();
    for n in nodes {
        match n {
            InlineNode::Text(t) => buf.push_str(&t),
            other => {
                if !buf.is_empty() {
                    out.extend(scan_inline_bbcode(&std::mem::take(&mut buf), diags));
                }
                out.push(other);
            }
        }
    }
    if !buf.is_empty() {
        out.extend(scan_inline_bbcode(&buf, diags));
    }
    out
}

fn append_inline(stack: &mut [(InlineFrame, Vec<InlineNode>)], node: InlineNode) {
    if let Some((_, top)) = stack.last_mut() {
        top.push(node);
    }
}

/// Плоский текст из inline-узлов (для alt изображений и т.п.).
pub fn plain_text_of(nodes: &[InlineNode]) -> String {
    let mut s = String::new();
    for n in nodes {
        match n {
            InlineNode::Text(t) | InlineNode::Code(t) => s.push_str(t),
            InlineNode::Bold(c)
            | InlineNode::Italic(c)
            | InlineNode::Underline(c)
            | InlineNode::Strike(c) => s.push_str(&plain_text_of(c)),
            InlineNode::Link { text, .. } | InlineNode::User { text, .. } => s.push_str(&plain_text_of(text)),
            InlineNode::Color { content, .. } | InlineNode::Size { content, .. } => {
                s.push_str(&plain_text_of(content))
            }
            InlineNode::Image { alt, .. } => s.push_str(alt),
            InlineNode::SoftBreak | InlineNode::HardBreak => s.push(' '),
            _ => {}
        }
    }
    s
}

/// Сканер спецвставок Bitrix24 внутри текстового прогона.
///
/// Распознаёт: `[u]...[/u]`, `[user=ID]...[/user]`, `[color=#HEX]...[/color]`,
/// `[size=N]...[/size]`, `[icon=URL ...]`.
/// Исключённые ТЗ теги (`[send]` и др.) оставляет текстом и выдаёт предупреждение.
fn scan_inline_bbcode(text: &str, diags: &mut Diagnostics) -> Vec<InlineNode> {
    let mut nodes = Vec::new();
    let mut plain = String::new();
    let bytes = text.as_bytes();
    let mut i = 0;

    let flush = |plain: &mut String, nodes: &mut Vec<InlineNode>| {
        if !plain.is_empty() {
            nodes.push(InlineNode::Text(std::mem::take(plain)));
        }
    };

    while i < bytes.len() {
        if bytes[i] == b'[' {
            let rest = &text[i..];
            if let Some((node, consumed)) = try_parse_bbcode_token(rest, diags) {
                flush(&mut plain, &mut nodes);
                if let Some(node) = node {
                    nodes.push(node);
                }
                i += consumed;
                continue;
            }
            // проверка исключённых тегов
            for tag in EXCLUDED_TAGS {
                let open = format!("[{tag}");
                if rest
                    .as_bytes()
                    .get(..open.len())
                    .is_some_and(|prefix| prefix.eq_ignore_ascii_case(open.as_bytes()))
                {
                    diags.push(Diagnostic::warn(format!(
                        "Тег [{tag}] исключён из области поддержки и оставлен как обычный текст."
                    )));
                    break;
                }
            }
        }
        let ch_len = text[i..].chars().next().map(|c| c.len_utf8()).unwrap_or(1);
        plain.push_str(&text[i..i + ch_len]);
        i += ch_len;
    }
    flush(&mut plain, &mut nodes);
    nodes
}

/// Пытается разобрать один BBCode-токен спецвставки в начале строки.
/// Возвращает (узел | None, потреблено байт).
fn try_parse_bbcode_token(s: &str, diags: &mut Diagnostics) -> Option<(Option<InlineNode>, usize)> {
    let lower = s.to_ascii_lowercase();

    // [u]...[/u]
    if lower.starts_with("[u]")
        && let Some(end) = lower.find("[/u]")
    {
        let inner = &s[3..end];
        return Some((
            Some(InlineNode::Underline(vec![InlineNode::Text(inner.to_string())])),
            end + 4,
        ));
    }

    // [user=ID]...[/user]
    if lower.starts_with("[user=")
        && let Some(close) = s.find(']')
    {
        let id = s[6..close].trim();
        if let Some(end) = lower.find("[/user]") {
            let inner = &s[close + 1..end];
            if id.is_empty() {
                diags.push(Diagnostic::warn("Пустой идентификатор сотрудника в [user=]."));
                return Some((Some(InlineNode::Text(inner.to_string())), end + 7));
            }
            return Some((
                Some(InlineNode::User {
                    id: id.to_string(),
                    text: vec![InlineNode::Text(inner.to_string())],
                }),
                end + 7,
            ));
        }
    }

    // [color=#HEX]...[/color]
    if lower.starts_with("[color=")
        && let Some(close) = s.find(']')
    {
        let value = &s[7..close];
        if let Some(end) = lower.find("[/color]") {
            let inner = &s[close + 1..end];
            let content = vec![InlineNode::Text(inner.to_string())];
            return match normalize_hex_color(value) {
                Some(hex) => Some((Some(InlineNode::Color { hex, content }), end + 8)),
                None => {
                    diags.push(Diagnostic::warn(format!(
                        "Некорректный HEX-цвет «{value}»: требуется 3 или 6 hex-символов. Цвет отброшен."
                    )));
                    Some((Some(InlineNode::Text(inner.to_string())), end + 8))
                }
            };
        }
    }

    // [size=N]...[/size]
    if lower.starts_with("[size=")
        && let Some(close) = s.find(']')
    {
        let value = &s[6..close];
        if let Some(end) = lower.find("[/size]") {
            let inner = &s[close + 1..end];
            let content = vec![InlineNode::Text(inner.to_string())];
            return match value.trim().parse::<u8>().ok().filter(|value| is_valid_size(*value)) {
                Some(px) => Some((Some(InlineNode::Size { px, content }), end + 7)),
                None => {
                    diags.push(Diagnostic::warn(format!(
                        "Некорректное значение size «{value}»: допустим диапазон 8–30. Размер отброшен."
                    )));
                    Some((Some(InlineNode::Text(inner.to_string())), end + 7))
                }
            };
        }
    }

    // [icon=URL params]
    if lower.starts_with("[icon=")
        && let Some(close) = s.find(']')
    {
        let body = &s[6..close];
        let (url, params) = match body.split_once(char::is_whitespace) {
            Some((url, params)) => (url.trim().to_string(), params.trim().to_string()),
            None => (body.trim().to_string(), String::new()),
        };
        if url.is_empty() {
            diags.push(Diagnostic::warn("Пустой URL в [icon=]."));
            return Some((None, close + 1));
        }
        return Some((Some(InlineNode::Icon { url, params }), close + 1));
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(input: &str) -> Document {
        parse_markdown(input).document
    }

    #[test]
    fn parses_paragraph_with_bold_italic() {
        let doc = parse("Hello **bold** and *italic*");
        assert_eq!(doc.blocks.len(), 1);
        match &doc.blocks[0] {
            BlockNode::Paragraph(nodes) => {
                assert!(nodes.iter().any(|n| matches!(n, InlineNode::Bold(_))));
                assert!(nodes.iter().any(|n| matches!(n, InlineNode::Italic(_))));
            }
            other => panic!("unexpected block: {other:?}"),
        }
    }

    #[test]
    fn parses_strikethrough() {
        let doc = parse("~~gone~~");
        match &doc.blocks[0] {
            BlockNode::Paragraph(nodes) => {
                assert!(matches!(nodes[0], InlineNode::Strike(_)));
            }
            other => panic!("unexpected block: {other:?}"),
        }
    }

    #[test]
    fn parses_heading_levels() {
        let doc = parse("# H1\n\n### H3");
        assert!(matches!(doc.blocks[0], BlockNode::Heading { level: 1, .. }));
        assert!(matches!(doc.blocks[1], BlockNode::Heading { level: 3, .. }));
    }

    #[test]
    fn parses_link() {
        let doc = parse("[text](https://example.com)");
        match &doc.blocks[0] {
            BlockNode::Paragraph(nodes) => match &nodes[0] {
                InlineNode::Link { url, text } => {
                    assert_eq!(url, "https://example.com");
                    assert_eq!(plain_text_of(text), "text");
                }
                other => panic!("unexpected inline: {other:?}"),
            },
            other => panic!("unexpected block: {other:?}"),
        }
    }

    #[test]
    fn parses_fenced_code_block() {
        let doc = parse("```rust\nfn main() {}\n```");
        match &doc.blocks[0] {
            BlockNode::CodeBlock { language, code } => {
                assert_eq!(language.as_deref(), Some("rust"));
                assert_eq!(code, "fn main() {}");
            }
            other => panic!("unexpected block: {other:?}"),
        }
    }

    #[test]
    fn parses_quote() {
        let doc = parse("> line one\n> line two");
        assert!(matches!(doc.blocks[0], BlockNode::Quote(_)));
    }

    #[test]
    fn parses_lists() {
        let doc = parse("- a\n- b\n\n1. x\n2. y");
        match &doc.blocks[0] {
            BlockNode::List { ordered, items, .. } => {
                assert!(!ordered);
                assert_eq!(items.len(), 2);
            }
            other => panic!("unexpected block: {other:?}"),
        }
        match &doc.blocks[1] {
            BlockNode::List { ordered, items, .. } => {
                assert!(*ordered);
                assert_eq!(items.len(), 2);
            }
            other => panic!("unexpected block: {other:?}"),
        }
    }

    #[test]
    fn parses_task_list_markers() {
        let doc = parse("- [ ] В работе\n- [x] Готово");
        match &doc.blocks[0] {
            BlockNode::List { items, .. } => {
                for (item, marker) in items.iter().zip(["[ ] ", "[x] "]) {
                    match &item[0] {
                        BlockNode::Paragraph(nodes) => {
                            assert!(matches!(nodes.first(), Some(InlineNode::Text(text)) if text.starts_with(marker)));
                        }
                        other => panic!("unexpected list item: {other:?}"),
                    }
                }
            }
            other => panic!("unexpected block: {other:?}"),
        }
    }

    #[test]
    fn parses_tables_as_structured_rows() {
        let doc = parse("| Имя | Статус | Сумма |\n| :--- | :---: | ---: |\n| **Иван** | Готово | 1600 |");
        match &doc.blocks[0] {
            BlockNode::Table { rows, alignments } => {
                assert_eq!(rows.len(), 2);
                assert_eq!(plain_text_of(&rows[0][0]), "Имя");
                assert!(matches!(rows[1][0].first(), Some(InlineNode::Bold(_))));
                assert_eq!(
                    alignments,
                    &[TableAlignment::Left, TableAlignment::Center, TableAlignment::Right]
                );
            }
            other => panic!("unexpected block: {other:?}"),
        }
    }

    #[test]
    fn parses_standalone_image_as_block() {
        let doc = parse("![alt](https://ex.com/i.png \"medium\")");
        match &doc.blocks[0] {
            BlockNode::Image { url, size, .. } => {
                assert_eq!(url, "https://ex.com/i.png");
                assert_eq!(*size, Some(ImageSize::Medium));
            }
            other => panic!("unexpected block: {other:?}"),
        }
    }

    #[test]
    fn parses_horizontal_rule() {
        let doc = parse("a\n\n---\n\nb");
        assert!(doc.blocks.iter().any(|b| matches!(b, BlockNode::HorizontalRule)));
    }

    #[test]
    fn converts_supported_inline_html_to_ast() {
        let doc = parse("<strong>важно</strong> <a href=\"https://example.com\">ссылка</a><br>далее");
        match &doc.blocks[0] {
            BlockNode::Paragraph(nodes) => {
                assert!(matches!(nodes.first(), Some(InlineNode::Bold(_))));
                assert!(nodes.iter().any(|node| matches!(node, InlineNode::Link { url, .. } if url == "https://example.com")));
                assert!(nodes.iter().any(|node| matches!(node, InlineNode::HardBreak)));
            }
            other => panic!("unexpected block: {other:?}"),
        }
    }

    #[test]
    fn preserves_unsupported_inline_html_as_text() {
        let result = parse_markdown("<mark>выделено</mark>");
        assert!(result.diagnostics.warnings() > 0);
        assert_eq!(plain_text_of(match &result.document.blocks[0] {
            BlockNode::Paragraph(nodes) => nodes,
            other => panic!("unexpected block: {other:?}"),
        }), "<mark>выделено</mark>");
    }

    #[test]
    fn scans_underline_passthrough() {
        let doc = parse("text [u]underlined[/u] tail");
        match &doc.blocks[0] {
            BlockNode::Paragraph(nodes) => {
                assert!(nodes.iter().any(|n| matches!(n, InlineNode::Underline(_))));
            }
            other => panic!("unexpected block: {other:?}"),
        }
    }

    #[test]
    fn scans_color_and_size_passthrough() {
        let doc = parse("[color=#ff0000]red[/color] and [size=20]big[/size]");
        match &doc.blocks[0] {
            BlockNode::Paragraph(nodes) => {
                assert!(nodes.iter().any(|n| matches!(n, InlineNode::Color { .. })));
                assert!(nodes.iter().any(|n| matches!(n, InlineNode::Size { px: 20, .. })));
            }
            other => panic!("unexpected block: {other:?}"),
        }
    }

    #[test]
    fn rejects_invalid_color_and_size() {
        let res = parse_markdown("[color=zzz]x[/color] [size=99]y[/size]");
        assert!(res.diagnostics.warnings() >= 2);
        match &res.document.blocks[0] {
            BlockNode::Paragraph(nodes) => {
                assert!(!nodes.iter().any(|n| matches!(n, InlineNode::Color { .. })));
                assert!(!nodes.iter().any(|n| matches!(n, InlineNode::Size { .. })));
            }
            other => panic!("unexpected block: {other:?}"),
        }
    }

    #[test]
    fn scans_user_and_icon() {
        let doc = parse("[user=111]Иван Иванов[/user] [icon=https://e.com/i.png size=16 title=Hi]");
        match &doc.blocks[0] {
            BlockNode::Paragraph(nodes) => {
                assert!(nodes.iter().any(|n| matches!(n, InlineNode::User { id, text } if id == "111" && plain_text_of(text) == "Иван Иванов")));
                assert!(nodes.iter().any(|n| matches!(n, InlineNode::Icon { url, .. } if url == "https://e.com/i.png")));
            }
            other => panic!("unexpected block: {other:?}"),
        }
    }

    #[test]
    fn excluded_tags_stay_text_with_warning() {
        let res = parse_markdown("hello [send=1]name[/send]");
        assert!(res.diagnostics.items.iter().any(|d| d.message.contains("[send]")));
        match &res.document.blocks[0] {
            BlockNode::Paragraph(nodes) => {
                let text = plain_text_of(nodes);
                assert!(text.contains("[send=1]"));
            }
            other => panic!("unexpected block: {other:?}"),
        }
    }

    #[test]
    fn inline_code_parsed() {
        let doc = parse("run `cargo build` now");
        match &doc.blocks[0] {
            BlockNode::Paragraph(nodes) => {
                assert!(nodes.iter().any(|n| matches!(n, InlineNode::Code(c) if c == "cargo build")));
            }
            other => panic!("unexpected block: {other:?}"),
        }
    }

    #[test]
    fn handles_cyrillic() {
        let doc = parse("Привет, **мир**! Ёмкость — 100 μF");
        assert_eq!(doc.blocks.len(), 1);
    }
}
