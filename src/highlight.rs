//! Модуль ручной обработки кода: псевдоподсветка синтаксиса.
//!
//! Используется как в GUI-preview, так и в BBCode-выводе Manual Code Highlight
//! профиля (через `[color]`-токены): `[code]` в Bitrix24 — только контейнер без
//! документированной подсветки, поэтому это лишь визуальное приближение,
//! не гарантированное самим Bitrix24.

use crate::model::EXCLUDED_TAGS;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    Keyword,
    String,
    Comment,
    Number,
    Plain,
}

pub struct Token {
    pub text: String,
    pub kind: TokenKind,
}

fn keywords_for(lang: &str) -> &'static [&'static str] {
    match lang.to_ascii_lowercase().as_str() {
        "rust" | "rs" => &[
            "fn", "let", "mut", "pub", "struct", "enum", "impl", "trait", "match", "if", "else",
            "for", "while", "loop", "return", "use", "mod", "const", "static", "async", "await",
            "move", "ref", "self", "Self", "super", "crate", "where", "type", "dyn", "unsafe",
        ],
        "python" | "py" => &[
            "def", "class", "import", "from", "return", "if", "elif", "else", "for", "while",
            "try", "except", "finally", "with", "as", "lambda", "yield", "async", "await", "pass",
            "raise", "in", "not", "and", "or", "is", "None", "True", "False", "global", "del",
        ],
        "javascript" | "js" | "typescript" | "ts" => &[
            "function", "const", "let", "var", "return", "if", "else", "for", "while", "class",
            "extends", "import", "export", "from", "async", "await", "new", "this", "typeof",
            "interface", "type", "enum", "null", "undefined", "true", "false", "switch", "case",
        ],
        "sql" => &[
            "SELECT", "FROM", "WHERE", "INSERT", "UPDATE", "DELETE", "JOIN", "LEFT", "RIGHT",
            "INNER", "ON", "GROUP", "BY", "ORDER", "HAVING", "LIMIT", "CREATE", "TABLE", "INDEX",
            "select", "from", "where", "insert", "update", "delete", "join", "and", "or", "not",
        ],
        "php" => &[
            "function", "class", "public", "private", "protected", "return", "if", "else",
            "foreach", "for", "while", "echo", "new", "use", "namespace", "static", "try", "catch",
        ],
        "csharp" | "c#" | "cs" => &[
            "using", "namespace", "class", "interface", "struct", "enum", "public", "private",
            "protected", "internal", "static", "readonly", "const", "void", "var", "new", "this",
            "base", "return", "if", "else", "for", "foreach", "while", "do", "switch", "case",
            "break", "continue", "try", "catch", "finally", "throw", "async", "await", "get",
            "set", "override", "virtual", "abstract", "sealed", "partial", "string", "int",
            "bool", "double", "float", "decimal", "long", "object", "null", "true", "false",
            "in", "out", "ref", "params", "is", "as", "typeof", "default", "yield", "record",
        ],
        _ => &[
            "if", "else", "for", "while", "return", "function", "class", "def", "fn", "let",
            "const", "var", "true", "false", "null",
        ],
    }
}

fn comment_prefix(lang: &str) -> &'static [&'static str] {
    match lang.to_ascii_lowercase().as_str() {
        "python" | "py" | "sh" | "bash" | "yaml" | "toml" => &["#"],
        "sql" => &["--"],
        _ => &["//", "#"],
    }
}

/// Разбивает одну строку кода на токены псевдоподсветки.
pub fn highlight_line(lang: &str, line: &str) -> Vec<Token> {
    let mut tokens = Vec::new();

    // комментарий целиком до конца строки
    for cp in comment_prefix(lang) {
        if let Some(pos) = line.find(cp) {
            // грубая проверка: не внутри строки
            let before = &line[..pos];
            if before.matches('"').count().is_multiple_of(2)
                && before.matches('\'').count().is_multiple_of(2)
            {
                let mut head = highlight_segment(lang, before);
                tokens.append(&mut head);
                tokens.push(Token { text: line[pos..].to_string(), kind: TokenKind::Comment });
                return tokens;
            }
        }
    }

    highlight_segment(lang, line)
}

/// Цветовая палитра ручной подсветки для BBCode-вывода (HEX из 6 символов).
pub fn bbcode_color_for(kind: TokenKind) -> Option<&'static str> {
    match kind {
        TokenKind::Keyword => Some("#c678dd"),
        TokenKind::String => Some("#98c379"),
        TokenKind::Comment => Some("#7f848e"),
        TokenKind::Number => Some("#d19a66"),
        TokenKind::Plain => None,
    }
}

/// Ручная подсветка кода в BBCode **без** тега `[code]`:
/// каждая строка начинается с документированной цитаты `>>` (визуальное
/// отделение кода), затем табуляция в 4 пробела, а токены оборачиваются
/// в документированный `[color=#HEX]`.
///
/// Строки всегда разделяются реальным `\n`: `>>` работает только
/// в начале строки.
pub fn highlight_code_to_bbcode(lang: &str, code: &str) -> String {
    const PREFIX: &str = ">>    ";
    let mut lines_out: Vec<String> = Vec::new();
    for line in code.lines() {
        if line.is_empty() {
            lines_out.push(">>".to_string());
            continue;
        }
        let (line, _) = escape_bbcode_tags(line, &[]);
        let mut out = String::from(PREFIX);
        for tok in highlight_line(lang, &line) {
            match bbcode_color_for(tok.kind) {
                Some(hex) if !tok.text.trim().is_empty() => {
                    out.push_str(&format!("[color={hex}]{}[/color]", tok.text));
                }
                _ => out.push_str(&tok.text),
            }
        }
        lines_out.push(out);
    }
    lines_out.join("\n")
}

pub(crate) fn escape_bbcode_tags(source: &str, allowed_tags: &[&str]) -> (String, bool) {
    let mut output = String::with_capacity(source.len());
    let mut position = 0;
    let mut escaped = false;

    while let Some(relative_start) = source[position..].find('[') {
        let start = position + relative_start;
        output.push_str(&source[position..start]);
        let Some(relative_end) = source[start + 1..].find(']') else {
            output.push_str(&source[start..]);
            return (output, escaped);
        };
        let end = start + 1 + relative_end;
        let raw_tag = &source[start + 1..end];
        let body = raw_tag.strip_prefix('/').unwrap_or(raw_tag);
        let name_end = body.find('=').unwrap_or(body.len());
        let name = &body[..name_end];
        let valid_name = !name.is_empty()
            && name.chars().next().is_some_and(|character| character.is_ascii_alphabetic())
            && name.chars().all(|character| {
                character.is_ascii_alphanumeric() || character == '_' || character == '-'
            });
        let known_tag = [
            "b", "i", "u", "s", "url", "user", "color", "size", "icon", "code",
        ]
        .iter()
        .any(|known| known.eq_ignore_ascii_case(name));
        let excluded = EXCLUDED_TAGS
            .iter()
            .any(|excluded| excluded.eq_ignore_ascii_case(name));
        let allowed = allowed_tags
            .iter()
            .any(|allowed| allowed.eq_ignore_ascii_case(name));
        let has_attributes = body.as_bytes().get(name_end) == Some(&b'=');
        let is_closing = raw_tag.starts_with('/');
        let is_tag = valid_name && (known_tag || excluded || has_attributes || is_closing);

        if is_tag && (excluded || !allowed) {
            output.push('［');
            output.push_str(raw_tag);
            output.push('］');
            escaped = true;
        } else {
            output.push_str(&source[start..=end]);
        }
        position = end + 1;
    }

    output.push_str(&source[position..]);
    (output, escaped)
}

/// Есть ли в коде BBCode-подобные конструкции, которые Bitrix24 может
/// интерпретировать как теги при выводе без контейнера `[code]`.
pub fn code_has_bbcode_like_tokens(code: &str) -> bool {
    escape_bbcode_tags(code, &[]).1
}

fn highlight_segment(lang: &str, seg: &str) -> Vec<Token> {
    let keywords = keywords_for(lang);
    let mut tokens = Vec::new();
    let chars: Vec<char> = seg.chars().collect();
    let mut i = 0;
    let mut plain = String::new();

    let flush_plain = |plain: &mut String, tokens: &mut Vec<Token>| {
        if !plain.is_empty() {
            tokens.push(Token { text: std::mem::take(plain), kind: TokenKind::Plain });
        }
    };

    while i < chars.len() {
        let c = chars[i];
        // строковые литералы
        if c == '"' || c == '\'' {
            flush_plain(&mut plain, &mut tokens);
            let quote = c;
            let mut s = String::from(c);
            i += 1;
            while i < chars.len() {
                s.push(chars[i]);
                if chars[i] == quote && chars.get(i.wrapping_sub(1)) != Some(&'\\') {
                    i += 1;
                    break;
                }
                i += 1;
            }
            tokens.push(Token { text: s, kind: TokenKind::String });
            continue;
        }
        // числа
        if c.is_ascii_digit() && !plain.chars().last().map(|p| p.is_alphanumeric() || p == '_').unwrap_or(false) {
            flush_plain(&mut plain, &mut tokens);
            let mut n = String::new();
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '.' || chars[i] == '_') {
                n.push(chars[i]);
                i += 1;
            }
            tokens.push(Token { text: n, kind: TokenKind::Number });
            continue;
        }
        // идентификаторы / ключевые слова
        if c.is_alphabetic() || c == '_' {
            flush_plain(&mut plain, &mut tokens);
            let mut w = String::new();
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                w.push(chars[i]);
                i += 1;
            }
            let kind = if keywords.contains(&w.as_str()) { TokenKind::Keyword } else { TokenKind::Plain };
            tokens.push(Token { text: w, kind });
            continue;
        }
        plain.push(c);
        i += 1;
    }
    flush_plain(&mut plain, &mut tokens);
    tokens
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(lang: &str, line: &str) -> Vec<(String, TokenKind)> {
        highlight_line(lang, line).into_iter().map(|t| (t.text, t.kind)).collect()
    }

    #[test]
    fn rust_keywords_highlighted() {
        let toks = kinds("rust", "fn main() { let x = 1; }");
        assert!(toks.iter().any(|(t, k)| t == "fn" && *k == TokenKind::Keyword));
        assert!(toks.iter().any(|(t, k)| t == "let" && *k == TokenKind::Keyword));
        assert!(toks.iter().any(|(t, k)| t == "1" && *k == TokenKind::Number));
    }

    #[test]
    fn strings_highlighted() {
        let toks = kinds("python", "print(\"hello\")");
        assert!(toks.iter().any(|(t, k)| t == "\"hello\"" && *k == TokenKind::String));
    }

    #[test]
    fn csharp_keywords_highlighted() {
        let toks = kinds("csharp", "public class Foo { private readonly int x = 1; }");
        assert!(toks.iter().any(|(t, k)| t == "public" && *k == TokenKind::Keyword));
        assert!(toks.iter().any(|(t, k)| t == "class" && *k == TokenKind::Keyword));
        assert!(toks.iter().any(|(t, k)| t == "private" && *k == TokenKind::Keyword));
        assert!(toks.iter().any(|(t, k)| t == "readonly" && *k == TokenKind::Keyword));
        assert!(toks.iter().any(|(t, k)| t == "int" && *k == TokenKind::Keyword));
    }

    #[test]
    fn csharp_language_aliases_share_keywords() {
        for lang in ["csharp", "c#", "cs", "CSharp", "C#"] {
            let toks = kinds(lang, "namespace App { }");
            assert!(
                toks.iter().any(|(t, k)| t == "namespace" && *k == TokenKind::Keyword),
                "namespace should be a keyword for lang={lang}"
            );
        }
    }

    #[test]
    fn comments_highlighted() {
        let toks = kinds("rust", "let a = 1; // комментарий");
        assert!(toks
            .iter()
            .any(|(t, k)| t.contains("комментарий") && *k == TokenKind::Comment));
    }

    #[test]
    fn comment_inside_string_not_highlighted() {
        let toks = kinds("rust", "let s = \"http://a\";");
        assert!(!toks.iter().any(|(_, k)| *k == TokenKind::Comment));
    }

    #[test]
    fn unknown_language_still_tokenizes() {
        let toks = kinds("brainfuck", "if x return 5");
        assert!(toks.iter().any(|(t, k)| t == "if" && *k == TokenKind::Keyword));
    }

    #[test]
    fn bbcode_highlight_wraps_tokens_in_color() {
        let out = highlight_code_to_bbcode("rust", "let x = 1;");
        assert!(out.contains("[color=#c678dd]let[/color]"));
        assert!(out.contains("[color=#d19a66]1[/color]"));
        assert!(!out.contains("[code]"));
    }

    #[test]
    fn bbcode_highlight_prefixes_each_line_with_quote_and_indent() {
        let out = highlight_code_to_bbcode("rust", "let a = 1;\nlet b = 2;");
        for line in out.lines() {
            assert!(line.starts_with(">>    "), "строка без >> и отступа: {line:?}");
        }
    }

    #[test]
    fn bbcode_highlight_preserves_empty_lines_as_quote() {
        let out = highlight_code_to_bbcode("rust", "a\n\nb");
        assert_eq!(out.lines().count(), 3);
        // пустая строка остаётся внутри цитатного блока
        assert_eq!(out.lines().nth(1), Some(">>"));
    }

    #[test]
    fn bbcode_highlight_always_uses_real_newlines() {
        // `>>` работает только в начале строки — разделитель всегда \n
        let out = highlight_code_to_bbcode("rust", "a\nb");
        assert_eq!(out.lines().count(), 2);
        assert!(!out.contains("[br]"));
    }

    #[test]
    fn bbcode_highlight_plain_tokens_not_wrapped() {
        let out = highlight_code_to_bbcode("rust", "foo(bar)");
        assert_eq!(out, ">>    foo(bar)");
    }

    #[test]
    fn bbcode_highlight_colors_strings_and_comments() {
        let out = highlight_code_to_bbcode("python", "print(\"hi\")  # note");
        assert!(out.contains("[color=#98c379]\"hi\"[/color]"));
        assert!(out.contains("[color=#7f848e]# note[/color]"));
    }

    #[test]
    fn detects_bbcode_like_tokens_in_code() {
        assert!(code_has_bbcode_like_tokens("s = \"[b]bold[/b]\""));
        assert!(code_has_bbcode_like_tokens("tag = '[URL=x]'"));
        assert!(!code_has_bbcode_like_tokens("arr[0] = map[key]"));
    }

    #[test]
    fn escapes_bbcode_like_tokens_without_changing_array_indexes() {
        let (escaped, changed) = escape_bbcode_tags("arr[0] = [send=1]name[/send]", &[]);

        assert!(changed);
        assert_eq!(escaped, "arr[0] = ［send=1］name［/send］");
    }

    #[test]
    fn keeps_profile_allowed_tags() {
        let (text, changed) = escape_bbcode_tags("[b]bold[/b] [color=#fff]red[/color]", &["b"]);

        assert!(changed);
        assert_eq!(text, "[b]bold[/b] ［color=#fff］red［/color］");
    }

    #[test]
    fn escapes_bbcode_like_tokens_in_manual_highlight() {
        assert_eq!(
            highlight_code_to_bbcode("", "[b]literal[/b]"),
            ">>    ［b］literal［/b］"
        );
    }
}
