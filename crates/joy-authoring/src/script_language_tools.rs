// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! UI-free language tools behind the script editor: catalog-driven syntax highlighting, completion entries and
//! filtering, call insertion, symbol lookup
//! for hover documentation, and diagnostic positions.
//!
//! Positions are character indices (Unicode scalar values) into the script, as egui's text cursors count them.
//! Diagnostics from FORM and particle scripts carry one-based lines and UTF-16 columns, which
//! [`ScriptDiagnostic::character_range`] converts.

use std::collections::HashSet;
use std::ops::Range;

use joy_core::scripting::{
    ScriptLanguageCatalog,
    ScriptLanguageSymbol,
    ScriptSymbolKind,
};

/// What a highlighted span is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TokenKind {
    /// Spaces, tabs, and line breaks.
    Whitespace,
    /// `#` and `//` comments to the end of the line.
    Comment,
    /// Quoted text (FORM and JSON).
    String,
    /// Numeric literals.
    Number,
    /// Language keywords.
    Keyword,
    /// Built-in functions.
    Function,
    /// Declared or known parameters.
    Parameter,
    /// A component after `.` (FORM).
    Property,
    /// Any other identifier.
    Variable,
    /// `true`, `false`, and `null`.
    Boolean,
    /// Operators and punctuation.
    Operator,
    /// Text the language rejects outright (FORM `@` names).
    Invalid,
}

/// One highlighted span, as a byte range into the script.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Token {
    /// Byte range in the source.
    pub range: Range<usize>,
    /// Highlight category.
    pub kind: TokenKind,
}

/// Highlighting rules for one language, built from its catalog.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ScriptSyntax {
    keywords: HashSet<String>,
    functions: HashSet<String>,
    parameters: HashSet<String>,
    parameter_keyword: Option<String>,
    strings: bool,
    comments: bool,
    components: bool,
    invalid_sigil: Option<char>,
    json: bool,
}

impl ScriptSyntax {
    /// Highlight a catalog's keywords and functions, with `#` and `//` comments and numbers.
    pub fn from_catalog(catalog: &ScriptLanguageCatalog) -> Self {
        let names = |kind: ScriptSymbolKind| -> HashSet<String> {
            catalog.symbols.iter().filter(|symbol| symbol.kind == kind).map(|symbol| symbol.name.clone()).collect()
        };
        Self {
            keywords: names(ScriptSymbolKind::Keyword),
            functions: names(ScriptSymbolKind::Function),
            comments: true,
            ..Self::default()
        }
    }

    /// FORM: the FORM catalog plus attribute-domain keywords, quoted strings, `param` declarations highlighted as
    /// parameters wherever they are used afterwards, components after `.`, and `@` names marked invalid (the FORM LAB
    /// stream language).
    pub fn form() -> Self {
        let mut syntax = Self::from_catalog(&joy_form::form_language_catalog());
        syntax.keywords.extend(joy_form::FORM_ATTRIBUTE_DOMAINS.iter().map(|name| (*name).to_owned()));
        syntax.strings = true;
        syntax.components = true;
        syntax.parameter_keyword = Some("param".to_owned());
        syntax.invalid_sigil = Some('@');
        syntax
    }

    /// JOYFX particle scripts: the particle catalog's keywords and functions (the PARTICLE LAB stream language).
    pub fn particle() -> Self {
        Self::from_catalog(&joy_fx::particle_language_catalog())
    }

    /// Read-only JSON: strings, signed numbers, and `true`/`false`/`null`.
    pub fn json() -> Self {
        Self { strings: true, json: true, ..Self::default() }
    }

    /// Plain text without highlighting.
    pub fn plain() -> Self {
        Self::default()
    }

    /// Also highlight these names as parameters (for example an effect's named parameters).
    pub fn with_parameters<'a>(mut self, names: impl IntoIterator<Item = &'a str>) -> Self {
        self.parameters = names.into_iter().map(str::to_owned).collect();
        self
    }

    /// Split `source` into highlighted spans covering every byte, in order.
    pub fn tokenize(&self, source: &str) -> Vec<Token> {
        let bytes = source.as_bytes();
        let mut tokens = Vec::new();
        let mut parameters = self.parameters.clone();
        let mut declaring_parameter = false;
        let mut component = false;
        let mut position = 0;
        while position < bytes.len() {
            let start = position;
            let rest = &source[start..];
            let character = rest.chars().next().expect("position is on a character boundary");
            let kind = if character.is_whitespace() {
                position += rest.find(|next: char| !next.is_whitespace()).unwrap_or(rest.len());
                TokenKind::Whitespace
            } else if self.comments && (rest.starts_with('#') || rest.starts_with("//")) {
                position += rest.find('\n').unwrap_or(rest.len());
                TokenKind::Comment
            } else if self.strings && character == '"' {
                position += string_length(rest);
                TokenKind::String
            } else if let Some(length) = number_length(rest, self.json) {
                position += length;
                TokenKind::Number
            } else if self.invalid_sigil == Some(character) && rest[1..].starts_with(is_identifier_start) {
                position += 1 + identifier_length(&rest[1..]);
                TokenKind::Invalid
            } else if is_identifier_start(character) {
                let length = identifier_length(rest);
                position += length;
                let name = &rest[..length];
                if self.json {
                    if matches!(name, "true" | "false" | "null") { TokenKind::Boolean } else { TokenKind::Operator }
                } else if component && self.components {
                    component = false;
                    TokenKind::Property
                } else if declaring_parameter {
                    declaring_parameter = false;
                    parameters.insert(name.to_owned());
                    TokenKind::Parameter
                } else if self.strings && matches!(name, "true" | "false" | "null") {
                    TokenKind::Boolean
                } else if self.parameter_keyword.as_deref() == Some(name) {
                    declaring_parameter = true;
                    TokenKind::Keyword
                } else if self.keywords.contains(name) {
                    TokenKind::Keyword
                } else if parameters.contains(name) {
                    TokenKind::Parameter
                } else if self.functions.contains(name) {
                    TokenKind::Function
                } else {
                    TokenKind::Variable
                }
            } else {
                position += character.len_utf8();
                component = character == '.';
                TokenKind::Operator
            };
            tokens.push(Token { range: start..position, kind });
        }
        tokens
    }
}

fn is_identifier_start(character: char) -> bool {
    character.is_ascii_alphabetic() || character == '_'
}

fn is_identifier_character(character: char) -> bool {
    character.is_ascii_alphanumeric() || character == '_'
}

fn identifier_length(text: &str) -> usize {
    text.find(|character: char| !is_identifier_character(character)).unwrap_or(text.len())
}

/// Length of a quoted string with backslash escapes, running to the end of the line when unterminated.
fn string_length(text: &str) -> usize {
    let mut escaped = false;
    for (index, character) in text.char_indices().skip(1) {
        match character {
            '\n' => return index,
            '\\' if !escaped => escaped = true,
            '"' if !escaped => return index + 1,
            _ => escaped = false,
        }
    }
    text.len()
}

/// Length of a number at the start of `text`: `(\d+(\.\d*)?|\.\d+)([eE][+-]?\d+)?`, or JSON's `-?\d+(\.\d+)?(...)`.
fn number_length(text: &str, json: bool) -> Option<usize> {
    let bytes = text.as_bytes();
    let digits = |from: usize| bytes[from..].iter().take_while(|byte| byte.is_ascii_digit()).count();
    let mut length = 0;
    if json && bytes.first() == Some(&b'-') {
        length = 1;
    }
    let integer = digits(length);
    length += integer;
    if bytes.get(length) == Some(&b'.') {
        let fraction = digits(length + 1);
        if integer == 0 && fraction == 0 {
            return None;
        }
        if !json || fraction > 0 {
            length += 1 + fraction;
        }
    } else if integer == 0 {
        return None;
    }
    if matches!(bytes.get(length), Some(b'e' | b'E')) {
        let sign = usize::from(matches!(bytes.get(length + 1), Some(b'+' | b'-')));
        let exponent = digits(length + 1 + sign);
        if exponent > 0 {
            length += 1 + sign + exponent;
        }
    }
    Some(length)
}

// ---------------------------------------------------------------------------------------------------------------------
// Completion and hover
// ---------------------------------------------------------------------------------------------------------------------

/// One completion entry, from a catalog symbol.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompletionItem {
    /// The symbol name shown in the list.
    pub label: String,
    /// Symbol category.
    pub kind: ScriptSymbolKind,
    /// One-line summary shown beside the label.
    pub detail: String,
    /// Longer documentation for the selected entry.
    pub documentation: String,
    /// Text inserted in place of the typed word: `name()` for functions.
    pub insert: String,
    /// Where the cursor lands within `insert`, in characters (between the parentheses for functions).
    pub cursor: usize,
}

/// Completion entries for every catalog symbol, in catalog order; functions insert a call with the cursor between the
/// parentheses (the TypeScript `snippetCompletion(name + '(${})')`).
pub fn completion_items(catalog: &ScriptLanguageCatalog) -> Vec<CompletionItem> {
    catalog
        .symbols
        .iter()
        .map(|symbol| {
            let (insert, cursor) = if symbol.kind == ScriptSymbolKind::Function {
                (format!("{}()", symbol.name), symbol.name.chars().count() + 1)
            } else {
                (symbol.name.clone(), symbol.name.chars().count())
            };
            CompletionItem {
                label: symbol.name.clone(),
                kind: symbol.kind,
                detail: symbol.detail.clone(),
                documentation: symbol.documentation.clone(),
                insert,
                cursor,
            }
        })
        .collect()
}

/// Indices of `items` that match `prefix`, best first: case-sensitive prefix matches, then case-insensitive prefix
/// matches, then labels containing the typed characters in order. An empty prefix (an explicit Ctrl+Space request)
/// lists every item in catalog order.
pub fn filter_completions(items: &[CompletionItem], prefix: &str) -> Vec<usize> {
    if prefix.is_empty() {
        return (0..items.len()).collect();
    }
    let lower = prefix.to_lowercase();
    let mut ranked: Vec<(u8, usize)> = items
        .iter()
        .enumerate()
        .filter_map(|(index, item)| {
            let label = item.label.to_lowercase();
            let rank = if item.label.starts_with(prefix) {
                0
            } else if label.starts_with(&lower) {
                1
            } else if is_subsequence(&lower, &label) {
                2
            } else {
                return None;
            };
            Some((rank, index))
        })
        .collect();
    ranked.sort();
    ranked.into_iter().map(|(_, index)| index).collect()
}

fn is_subsequence(needle: &str, haystack: &str) -> bool {
    let mut remaining = haystack.chars();
    needle.chars().all(|character| remaining.any(|candidate| candidate == character))
}

/// The identifier ending at character `cursor`: its starting character index and text (empty when the cursor does
/// not follow an identifier).
pub fn word_before(text: &str, cursor: usize) -> (usize, String) {
    let characters: Vec<char> = text.chars().take(cursor).collect();
    let start =
        characters.iter().rposition(|character| !is_identifier_character(*character)).map_or(0, |index| index + 1);
    let word: String = characters[start..].iter().collect();
    // Identifiers cannot start with a digit; `12ab` is not a word to complete.
    if word.starts_with(|character: char| character.is_ascii_digit()) {
        return (cursor, String::new());
    }
    (start, word)
}

/// The identifier around character `index` (the hovered word), as a character range and text.
pub fn word_at(text: &str, index: usize) -> Option<(Range<usize>, String)> {
    let characters: Vec<char> = text.chars().collect();
    if characters.is_empty() {
        return None;
    }
    let index = index.min(characters.len());
    let mut start = index;
    while start > 0 && is_identifier_character(characters[start - 1]) {
        start -= 1;
    }
    let mut end = index;
    while end < characters.len() && is_identifier_character(characters[end]) {
        end += 1;
    }
    let word: String = characters[start..end].iter().collect();
    (!word.is_empty() && word.starts_with(is_identifier_start)).then_some((start..end, word))
}

/// The catalog symbol named exactly `word`, for hover documentation.
pub fn find_symbol<'a>(catalog: &'a ScriptLanguageCatalog, word: &str) -> Option<&'a ScriptLanguageSymbol> {
    catalog.symbol(word)
}

/// The hover heading for a symbol: its signatures joined with ` | `, or its name.
pub fn symbol_heading(symbol: &ScriptLanguageSymbol) -> String {
    if symbol.signatures.is_empty() { symbol.name.clone() } else { symbol.signatures.join(" | ") }
}

/// Replace the characters `start..end` of `text` with `item`'s insertion, returning the cursor's new character index.
pub fn apply_completion(text: &mut String, start: usize, end: usize, item: &CompletionItem) -> usize {
    let byte = |index: usize| text.char_indices().nth(index).map_or(text.len(), |(offset, _)| offset);
    let (from, to) = (byte(start), byte(end.max(start)));
    text.replace_range(from..to, &item.insert);
    start + item.cursor
}

// ---------------------------------------------------------------------------------------------------------------------
// Positions and diagnostics
// ---------------------------------------------------------------------------------------------------------------------

/// How serious a diagnostic is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DiagnosticSeverity {
    /// The script cannot run.
    #[default]
    Error,
    /// The script runs but something is suspicious.
    Warning,
}

/// A located message to mark in the editor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScriptDiagnostic {
    /// One-based line.
    pub line: usize,
    /// One-based column in UTF-16 code units, as FORM and particle diagnostics report it.
    pub column: usize,
    /// The message shown on hover and in status lines.
    pub message: String,
    /// Severity.
    pub severity: DiagnosticSeverity,
}

impl ScriptDiagnostic {
    /// An error at a one-based line and UTF-16 column.
    pub fn error(line: usize, column: usize, message: impl Into<String>) -> Self {
        Self { line, column, message: message.into(), severity: DiagnosticSeverity::Error }
    }

    /// The characters to underline: the word or number at the diagnostic, or one character (the line break at the end
    /// of a line), clamped into the text.
    pub fn character_range(&self, text: &str) -> Range<usize> {
        let start = character_at(text, self.line, self.column);
        let characters: Vec<char> = text.chars().collect();
        let mut end = start;
        while end < characters.len() && is_identifier_character(characters[end]) {
            end += 1;
        }
        if end == start {
            end = (start + 1).min(characters.len().max(start));
        }
        start..end
    }
}

impl From<&joy_form::Diagnostic> for ScriptDiagnostic {
    fn from(diagnostic: &joy_form::Diagnostic) -> Self {
        Self::error(diagnostic.line, diagnostic.column, diagnostic.message.clone())
    }
}

impl From<&joy_fx::ParticleScriptError> for ScriptDiagnostic {
    fn from(error: &joy_fx::ParticleScriptError) -> Self {
        Self::error(error.line as usize, error.column as usize, error.message.clone())
    }
}

/// The character index of one-based `line` and one-based UTF-16 `column`, clamped to the line and the text.
pub fn character_at(text: &str, line: usize, column: usize) -> usize {
    let mut index = 0;
    for (number, content) in text.split('\n').enumerate() {
        if number + 1 == line.max(1) {
            let mut units = 1;
            for character in content.chars() {
                if units >= column {
                    break;
                }
                units += character.len_utf16();
                index += 1;
            }
            return index;
        }
        index += content.chars().count() + 1;
    }
    text.chars().count()
}

/// The character index where one-based `line` starts (the end of the text past the last line).
pub fn line_start(text: &str, line: usize) -> usize {
    character_at(text, line, 1)
}

/// The one-based line and column (in characters) of character `index`.
pub fn line_and_column(text: &str, index: usize) -> (usize, usize) {
    let mut line = 1;
    let mut column = 1;
    for character in text.chars().take(index) {
        if character == '\n' {
            line += 1;
            column = 1;
        } else {
            column += 1;
        }
    }
    (line, column)
}

/// Number of lines (an empty script has one).
pub fn line_count(text: &str) -> usize {
    text.split('\n').count()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(syntax: &ScriptSyntax, source: &str) -> Vec<(String, TokenKind)> {
        syntax
            .tokenize(source)
            .into_iter()
            .filter(|token| token.kind != TokenKind::Whitespace)
            .map(|token| (source[token.range].to_owned(), token.kind))
            .collect()
    }

    #[test]
    fn form_highlighting_follows_the_lab_stream_language() {
        let source = "param width = 2 meta(min=1)\n# note\nlet p = box(width, 1e-3, .5).x\n@bad \"text\" true";
        let tokens = kinds(&ScriptSyntax::form(), source);
        let find = |text: &str| tokens.iter().find(|(token, _)| token == text).map(|(_, kind)| *kind);
        assert_eq!(find("param"), Some(TokenKind::Keyword));
        assert_eq!(tokens[1], ("width".to_owned(), TokenKind::Parameter), "declarations are parameters");
        assert_eq!(tokens.iter().filter(|(text, kind)| text == "width" && *kind == TokenKind::Parameter).count(), 2);
        assert_eq!(find("meta"), Some(TokenKind::Keyword));
        assert_eq!(find("# note"), Some(TokenKind::Comment));
        assert_eq!(find("box"), Some(TokenKind::Function));
        assert_eq!(find("1e-3"), Some(TokenKind::Number));
        assert_eq!(find(".5"), Some(TokenKind::Number));
        assert_eq!(find("x"), Some(TokenKind::Property), "components after a dot");
        assert_eq!(find("@bad"), Some(TokenKind::Invalid));
        assert_eq!(find("\"text\""), Some(TokenKind::String));
        assert_eq!(find("true"), Some(TokenKind::Boolean));
        assert_eq!(find("p"), Some(TokenKind::Variable));
        let covered: usize = ScriptSyntax::form().tokenize(source).iter().map(|token| token.range.len()).sum();
        assert_eq!(covered, source.len(), "tokens cover the whole source");
    }

    #[test]
    fn particle_and_json_highlighting() {
        let particle = ScriptSyntax::particle().with_parameters(["power"]);
        let tokens = kinds(&particle, "let a = rand(0, tau) * power; // c\ncolor.r *= 3;");
        assert_eq!(tokens[0], ("let".to_owned(), TokenKind::Keyword));
        assert!(tokens.contains(&("rand".to_owned(), TokenKind::Function)));
        assert!(tokens.contains(&("power".to_owned(), TokenKind::Parameter)));
        assert!(tokens.contains(&("// c".to_owned(), TokenKind::Comment)));
        assert!(tokens.contains(&("r".to_owned(), TokenKind::Variable)), "particle scripts color components plainly");
        let json = kinds(&ScriptSyntax::json(), "{\"rate\": -2.5e3, \"on\": true, \"x\": null}");
        assert!(json.contains(&("\"rate\"".to_owned(), TokenKind::String)));
        assert!(json.contains(&("-2.5e3".to_owned(), TokenKind::Number)));
        assert!(json.contains(&("true".to_owned(), TokenKind::Boolean)));
        assert!(json.contains(&("null".to_owned(), TokenKind::Boolean)));
    }

    #[test]
    fn completion_filters_and_inserts_calls_with_the_cursor_inside() {
        let catalog = joy_fx::particle_language_catalog();
        let items = completion_items(&catalog);
        let matches = filter_completions(&items, "sm");
        assert_eq!(items[matches[0]].label, "smoothstep");
        assert_eq!(filter_completions(&items, "").len(), items.len(), "explicit requests list everything");
        assert!(filter_completions(&items, "zzz").is_empty());
        let mut text = "size = smo + 1".to_owned();
        let (start, word) = word_before(&text, 10);
        assert_eq!((start, word.as_str()), (7, "smo"));
        let item = &items[matches[0]];
        let cursor = apply_completion(&mut text, start, 10, item);
        assert_eq!(text, "size = smoothstep() + 1");
        assert_eq!(cursor, 18, "between the parentheses");
        let keyword = items.iter().find(|item| item.label == "let").unwrap();
        assert_eq!((keyword.insert.as_str(), keyword.cursor), ("let", 3));
        assert_eq!(word_before("x = 12", 6), (6, String::new()));
    }

    #[test]
    fn hover_words_and_diagnostic_positions() {
        let catalog = joy_form::form_language_catalog();
        let text = "let a = sphere(1)\n  émoji😀 = box(1, 2, 3)";
        let (range, word) = word_at(text, 10).unwrap();
        assert_eq!((range, word.as_str()), (8..14, "sphere"));
        assert_eq!(symbol_heading(find_symbol(&catalog, &word).unwrap()), "sphere(radius[, segments])");
        assert!(word_at(text, 7).is_none(), "spaces and operators are not words");
        // Line 2, UTF-16 column 13 is `box`: the emoji takes two UTF-16 units but one character.
        let diagnostic = ScriptDiagnostic::error(2, 13, "Unknown call");
        let range = diagnostic.character_range(text);
        let characters: Vec<char> = text.chars().collect();
        assert_eq!(characters[range].iter().collect::<String>(), "box");
        assert_eq!(line_start(text, 2), 18);
        assert_eq!(line_and_column(text, 20), (2, 3));
        assert_eq!(line_count(text), 2);
        assert_eq!(character_at(text, 9, 1), text.chars().count(), "lines past the end clamp");
    }
}
