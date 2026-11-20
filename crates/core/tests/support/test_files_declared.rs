//! Guard that every top-level `tests/*.rs` file is compiled.
//!
//! Each crate builds its integration tests as one binary, `tests/main.rs`,
//! with `autotests = false` in its manifest. Cargo then ignores any file in
//! `tests/` that `main.rs` does not declare, so a new test file that is not
//! added there would never compile or run. This module is included by path
//! from every crate's `tests/main.rs` and fails when such a file exists.

use std::collections::BTreeSet;
use std::path::Path;

/// Stems of the test files in `files` that are neither declared as
/// `mod <stem>;` in `main_rs` nor listed as `path = "tests/<stem>.rs"` in a
/// `[[test]]` table of `manifest`, sorted. `main` itself is never reported.
fn undeclared_test_files(files: &[&str], main_rs: &str, manifest: &str) -> Vec<String> {
    let mut declared = declared_module_files(main_rs);
    declared.extend(test_target_files(manifest));
    let mut undeclared: Vec<String> = files
        .iter()
        .filter(|stem| **stem != "main" && !declared.contains(**stem))
        .map(|stem| stem.to_string())
        .collect();
    undeclared.sort();
    undeclared
}

/// Stems of the `tests/` files that `main_rs` compiles as modules. Each
/// top-level `mod <name>;`, optionally `pub` and after attributes, compiles
/// `<name>.rs`, or the file its `#[path = "..."]` attribute names. Comments,
/// literals, macro input and anything nested in `{ }`, `( )` or `[ ]`, such
/// as an inline `mod { }`, declare nothing.
fn declared_module_files(main_rs: &str) -> BTreeSet<String> {
    let tokens = rust_tokens(main_rs);
    let word = |i: usize, w: &str| matches!(tokens.get(i), Some(Token::Word(t)) if t == w);
    let punct = |i: usize, p: char| tokens.get(i) == Some(&Token::Punct(p));
    let mut declared = BTreeSet::new();
    let mut brace_depth = 0usize;
    // The file a `#[path]` attribute names for the item being read.
    let mut path_attr: Option<&str> = None;
    let mut i = 0;
    while i < tokens.len() {
        if punct(i, '#') && punct(i + 1, '[') {
            let close = group_end(&tokens, i + 1, ('[', ']'));
            if let [Token::Word(key), Token::Punct('='), Token::Str(path)] = &tokens[i + 2..close]
                && key == "path"
            {
                path_attr = Some(path);
            }
            i = close + 1;
            continue;
        }
        if word(i, "pub") {
            i += 1;
            if punct(i, '(') {
                i = group_end(&tokens, i, ('(', ')')) + 1;
            }
            continue;
        }
        if brace_depth == 0
            && word(i, "mod")
            && punct(i + 2, ';')
            && let Some(Token::Word(name)) = tokens.get(i + 1)
        {
            let file = match path_attr {
                Some(path) => path
                    .strip_prefix("./")
                    .unwrap_or(path)
                    .strip_suffix(".rs")
                    .filter(|stem| !stem.contains('/')),
                None => Some(name.as_str()),
            };
            declared.extend(file.map(str::to_string));
            i += 3;
        } else {
            match tokens[i] {
                Token::Punct('{') => brace_depth += 1,
                Token::Punct('}') => brace_depth = brace_depth.saturating_sub(1),
                // No item is declared inside parentheses or brackets, so
                // skip the group whole: macro input like `m!(mod x;)`.
                Token::Punct('(') => i = group_end(&tokens, i, ('(', ')')),
                Token::Punct('[') => i = group_end(&tokens, i, ('[', ']')),
                _ => {}
            }
            i += 1;
        }
        path_attr = None;
    }
    declared
}

/// A Rust token, as much of one as finding module declarations needs.
#[derive(Debug, PartialEq)]
enum Token {
    /// An identifier, keyword or number; a raw identifier loses its `r#`.
    Word(String),
    /// The contents of a string literal of any kind.
    Str(String),
    /// Any other single character.
    Punct(char),
}

/// Splits Rust source into tokens. Comments (line, doc and nested block
/// comments) are dropped, and string, raw string, byte string and char
/// literals are read whole, so text inside them is never taken for code.
fn rust_tokens(src: &str) -> Vec<Token> {
    let chars: Vec<char> = src.chars().collect();
    let at = |i: usize| chars.get(i).copied();
    let is_word_char = |c: char| c.is_alphanumeric() || c == '_';
    let mut tokens = Vec::new();
    let mut i = 0;
    while let Some(c) = at(i) {
        if c.is_whitespace() {
            i += 1;
        } else if c == '/' && at(i + 1) == Some('/') {
            while at(i).is_some_and(|c| c != '\n') {
                i += 1;
            }
        } else if c == '/' && at(i + 1) == Some('*') {
            let mut depth = 0;
            while i < chars.len() {
                if at(i) == Some('/') && at(i + 1) == Some('*') {
                    depth += 1;
                    i += 2;
                } else if at(i) == Some('*') && at(i + 1) == Some('/') {
                    depth -= 1;
                    i += 2;
                    if depth == 0 {
                        break;
                    }
                } else {
                    i += 1;
                }
            }
        } else if c == '"' {
            let (text, end) = quoted(&chars, i + 1);
            tokens.push(Token::Str(text));
            i = end;
        } else if c == '\'' {
            match char_literal_end(&chars, i) {
                Some(end) => i = end,
                // A lifetime or label.
                None => {
                    tokens.push(Token::Punct('\''));
                    i += 1;
                }
            }
        } else if is_word_char(c) {
            let start = i;
            while at(i).is_some_and(is_word_char) {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            let hashes = chars[i..].iter().take_while(|&&c| c == '#').count();
            match (word.as_str(), at(i)) {
                ("b" | "c", Some('"')) => {
                    let (text, end) = quoted(&chars, i + 1);
                    tokens.push(Token::Str(text));
                    i = end;
                }
                ("b", Some('\'')) => i = char_literal_end(&chars, i).unwrap_or(i + 1),
                ("r" | "br" | "cr", _) if at(i + hashes) == Some('"') => {
                    // Ends at a `"` followed by as many `#` as opened it.
                    let body = i + hashes + 1;
                    let mut end = body;
                    while end < chars.len()
                        && !(chars[end] == '"'
                            && chars[end + 1..].iter().take_while(|&&c| c == '#').count() >= hashes)
                    {
                        end += 1;
                    }
                    tokens.push(Token::Str(chars[body..end].iter().collect()));
                    i = (end + 1 + hashes).min(chars.len());
                }
                ("r", Some('#')) if hashes == 1 && at(i + 1).is_some_and(is_word_char) => {
                    let start = i + 1;
                    i = start;
                    while at(i).is_some_and(is_word_char) {
                        i += 1;
                    }
                    tokens.push(Token::Word(chars[start..i].iter().collect()));
                }
                _ => tokens.push(Token::Word(word)),
            }
        } else {
            tokens.push(Token::Punct(c));
            i += 1;
        }
    }
    tokens
}

/// Reads a quoted string body starting just after its opening `"`, taking
/// a backslash as escaping the next character. Returns the contents and the
/// index after the closing quote.
fn quoted(chars: &[char], start: usize) -> (String, usize) {
    let mut text = String::new();
    let mut i = start;
    while let Some(&c) = chars.get(i) {
        i += 1;
        match c {
            '"' => break,
            '\\' => {
                if let Some(&escaped) = chars.get(i) {
                    text.push(escaped);
                    i += 1;
                }
            }
            _ => text.push(c),
        }
    }
    (text, i)
}

/// If a char literal (`'x'`, `'\n'`, `'\u{..}'`) starts at `start`, the index
/// after it; `None` for a lifetime or label.
fn char_literal_end(chars: &[char], start: usize) -> Option<usize> {
    if chars.get(start + 1) == Some(&'\\') {
        // Skip the quote, the backslash and the escaped character.
        let close = chars.get(start + 3..)?.iter().position(|&c| c == '\'')?;
        return Some(start + 3 + close + 1);
    }
    (chars.get(start + 2) == Some(&'\'')).then_some(start + 3)
}

/// Index of the token that closes the group opened at `open`, or the token
/// count when the group is never closed.
fn group_end(tokens: &[Token], open: usize, (opener, closer): (char, char)) -> usize {
    let mut depth = 0usize;
    for (i, token) in tokens.iter().enumerate().skip(open) {
        if *token == Token::Punct(opener) {
            depth += 1;
        } else if *token == Token::Punct(closer) {
            depth -= 1;
            if depth == 0 {
                return i;
            }
        }
    }
    tokens.len()
}

/// Stems of the `tests/<stem>.rs` files that `manifest` names as the `path`
/// of a `[[test]]` target. Reads only what a Cargo manifest uses for that:
/// table headers, `key = "string"` lines and trailing comments.
fn test_target_files(manifest: &str) -> BTreeSet<String> {
    let mut targets = BTreeSet::new();
    let mut in_test_table = false;
    for line in manifest.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_test_table = line
                .strip_prefix("[[")
                .and_then(|rest| rest.split_once("]]"))
                .is_some_and(|(name, rest)| name.trim() == "test" && is_blank_or_comment(rest));
            continue;
        }
        if !in_test_table {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if key.trim().trim_matches('"') != "path" {
            continue;
        }
        let Some((path, rest)) = toml_string(value.trim_start()) else {
            continue;
        };
        if let Some(stem) = path
            .strip_prefix("tests/")
            .and_then(|p| p.strip_suffix(".rs"))
            && !stem.contains('/')
            && is_blank_or_comment(rest)
        {
            targets.insert(stem.to_string());
        }
    }
    targets
}

/// Whether what follows a TOML value on its line is nothing or a comment.
fn is_blank_or_comment(rest: &str) -> bool {
    let rest = rest.trim();
    rest.is_empty() || rest.starts_with('#')
}

/// Reads a TOML basic (`"..."`) or literal (`'...'`) string from the start of
/// `text`, returning its value and the text after it.
fn toml_string(text: &str) -> Option<(String, &str)> {
    if let Some(body) = text.strip_prefix('\'') {
        let (value, rest) = body.split_once('\'')?;
        return Some((value.to_string(), rest));
    }
    let body = text.strip_prefix('"')?;
    let chars: Vec<char> = body.chars().collect();
    let (value, end) = quoted(&chars, 0);
    let consumed: usize = chars[..end].iter().map(|c| c.len_utf8()).sum();
    Some((value, &body[consumed..]))
}

#[test]
fn every_test_file_is_compiled() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let tests_dir = root.join("tests");
    let mut stems = Vec::new();
    for entry in std::fs::read_dir(&tests_dir).expect("tests/ is readable") {
        let path = entry.expect("tests/ entry is readable").path();
        if path.is_file() && path.extension().is_some_and(|ext| ext == "rs") {
            let stem = path
                .file_stem()
                .and_then(|s| s.to_str())
                .expect("test file names are UTF-8");
            stems.push(stem.to_string());
        }
    }
    assert!(
        !stems.is_empty(),
        "found no test files in {}",
        tests_dir.display()
    );
    let main_rs =
        std::fs::read_to_string(tests_dir.join("main.rs")).expect("tests/main.rs is readable");
    let manifest =
        std::fs::read_to_string(root.join("Cargo.toml")).expect("Cargo.toml is readable");
    let files: Vec<&str> = stems.iter().map(String::as_str).collect();
    let undeclared = undeclared_test_files(&files, &main_rs, &manifest);
    assert!(
        undeclared.is_empty(),
        "test files that never compile, because autotests = false: {undeclared:?}; \
         add `mod <stem>;` to tests/main.rs for each"
    );
}

#[test]
fn undeclared_test_files_finds_only_the_stray() {
    let main_rs = "mod a_test;\n#[path = \"x.rs\"] mod helper;\n";
    let manifest = "[[test]]\nname = \"b_test\"\npath = \"tests/b_test.rs\"\n";
    let cases: &[(&[&str], &[&str])] = &[
        (&["a_test", "main"], &[]),
        (&["a_test", "b_test", "main"], &[]),
        (&["a_test", "stray_test", "main"], &["stray_test"]),
        (&["a_test_two", "main"], &["a_test_two"]),
    ];
    for (files, want) in cases {
        assert_eq!(
            undeclared_test_files(files, main_rs, manifest),
            *want,
            "{files:?}"
        );
    }
}

#[test]
fn undeclared_test_files_counts_only_real_declarations() {
    const STRAY: &[&str] = &["stray_test"];
    const DECLARED: &[&str] = &[];
    // (main_rs, manifest, want) for the files `stray_test` and `main`.
    let cases: &[(&str, &str, &[&str])] = &[
        // Text that mentions the module but declares nothing.
        ("// mod stray_test;\n", "", STRAY),
        ("/// mod stray_test;\n", "", STRAY),
        ("/* mod stray_test; */\n", "", STRAY),
        ("/* /* nested */ mod stray_test; */\n", "", STRAY),
        ("const NOTE: &str = \" mod stray_test; \";\n", "", STRAY),
        ("const NOTE: &str = r#\" mod stray_test; \"#;\n", "", STRAY),
        ("mod inner {\n    mod stray_test;\n}\n", "", STRAY),
        ("#[path = \"other.rs\"]\nmod stray_test;\n", "", STRAY),
        // Macro input is tokens, not items, whatever its delimiters.
        (
            "const NOTE: &str = stringify!(mod stray_test;);\n",
            "",
            STRAY,
        ),
        (
            "const NOTE: &str = stringify![mod stray_test;];\n",
            "",
            STRAY,
        ),
        (
            "const NOTE: &str = stringify!{mod stray_test;};\n",
            "",
            STRAY,
        ),
        // Real declarations, around literals that a naive scan misreads.
        ("mod stray_test;\n", "", DECLARED),
        ("pub mod stray_test;\n", "", DECLARED),
        ("pub(crate) mod stray_test;\n", "", DECLARED),
        ("#[cfg(test)]\nmod stray_test;\n", "", DECLARED),
        ("#[path = \"stray_test.rs\"]\nmod renamed;\n", "", DECLARED),
        ("const Q: char = '\"';\nmod stray_test;\n", "", DECLARED),
        (
            "#![allow(dead_code)]\nconst A: [u8; 1] = [0];\nfn f(_: (u8, u8)) {}\nmod stray_test;\n",
            "",
            DECLARED,
        ),
        (
            "fn f<'a>(s: &'a str) -> &'a str { s }\nmod stray_test;\n",
            "",
            DECLARED,
        ),
        // Manifest paths count only inside a `[[test]]` table.
        (
            "",
            "[package.metadata.note]\npath = \"tests/stray_test.rs\"\n",
            STRAY,
        ),
        (
            "",
            "[[test]]\nname = \"x\"\n\n[dependencies]\npath = \"tests/stray_test.rs\"\n",
            STRAY,
        ),
        (
            "",
            "[[test]]\nname = \"stray_test\"\npath = \"tests/stray_test.rs\" # kept separate\n",
            DECLARED,
        ),
        (
            "",
            "[[test]] # second target\nname = \"stray_test\"\npath = 'tests/stray_test.rs'\n",
            DECLARED,
        ),
    ];
    for (main_rs, manifest, want) in cases {
        assert_eq!(
            undeclared_test_files(&["stray_test", "main"], main_rs, manifest),
            *want,
            "main.rs {main_rs:?}, manifest {manifest:?}"
        );
    }
}
