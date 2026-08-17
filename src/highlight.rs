//! Модуль ручной обработки кода: псевдоподсветка для preview.
//!
//! Подсветка выполняется исключительно на стороне приложения (в GUI-preview)
//! и никогда не переносится в итоговый BBCode: `[code]` в Bitrix24 — только
//! контейнер кода без документированной подсветки синтаксиса.

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
            if before.matches('"').count() % 2 == 0 && before.matches('\'').count() % 2 == 0 {
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
        let mut out = String::from(PREFIX);
        for tok in highlight_line(lang, line) {
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

/// Есть ли в коде BBCode-подобные конструкции, которые Bitrix24 может
/// интерпретировать как теги при выводе без контейнера `[code]`.
pub fn code_has_bbcode_like_tokens(code: &str) -> bool {
    const RISKY: [&str; 12] = [
        "[b]", "[/b]", "[i]", "[/i]", "[u]", "[/u]", "[s]", "[/s]", "[url", "[code", "[color",
        "[size",
    ];
    let lower = code.to_ascii_lowercase();
    RISKY.iter().any(|t| lower.contains(t))
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
}
