use std::{ops::Range, sync::LazyLock};

use regex::Regex;
use serde::Deserialize;

/// A style marker at a specific byte offset in the source
/// Represents either the start or end of a styled region
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct StyleMarker {
    /// Byte offset in the source (0-indexed)
    pub offset: usize,
    /// Whether this is a start (true) or end (false) marker
    pub is_start: bool,
    /// The token type being styled
    pub token_type: TokenType,
}

/// Highlighting information for a single line
/// Contains sorted style markers that should be applied when rendering the line
#[derive(Debug, Clone)]
pub struct LineHighlight {
    /// Line number (1-indexed)
    pub line: usize,
    /// Byte offset where this line starts in the source
    pub line_start_offset: usize,
    /// Byte offset where this line ends (exclusive) in the source
    pub line_end_offset: usize,
    /// Style markers for this line, sorted by offset
    /// Offsets are relative to line_start_offset
    pub markers: Vec<StyleMarker>,
}

/// Token types for syntax highlighting
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum TokenType {
    Keyword,
    Identifier,
    String,
    Number,
    Regex,
    Comment,
}

/// Language hint for keyword highlighting.
///
/// Determines which set of keywords are recognized as `TokenType::Keyword`.
/// Non-keyword tokens (strings, comments, numbers, etc.) are language-agnostic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Language {
    /// JavaScript/TypeScript keywords
    #[default]
    JavaScript,
    /// CSS keywords (currently empty — CSS has no keyword highlighting)
    Css,
}

impl Language {
    /// Returns true if the given identifier is a keyword in this language.
    pub fn is_keyword(self, ident: &str) -> bool {
        match self {
            Language::JavaScript => JS_KEYWORDS.binary_search(&ident).is_ok(),
            Language::Css => false,
        }
    }
}

/// JavaScript/TypeScript keywords (sorted for binary search)
static JS_KEYWORDS: &[&str] = &[
    "as",
    "async",
    "await",
    "break",
    "case",
    "catch",
    "class",
    "const",
    "continue",
    "debugger",
    "default",
    "delete",
    "do",
    "else",
    "enum",
    "export",
    "extends",
    "false",
    "finally",
    "for",
    "from",
    "function",
    "if",
    "implements",
    "import",
    "in",
    "instanceof",
    "interface",
    "let",
    "new",
    "null",
    "of",
    "package",
    "private",
    "protected",
    "public",
    "return",
    "static",
    "super",
    "switch",
    "this",
    "throw",
    "true",
    "try",
    "type",
    "typeof",
    "undefined",
    "var",
    "void",
    "while",
    "with",
    "yield",
];

/// ANSI color codes for token types
#[derive(Debug, Clone, Copy)]
pub struct ColorScheme {
    pub reset: &'static str,
    pub keyword: &'static str,
    pub identifier: &'static str,
    pub string: &'static str,
    pub number: &'static str,
    pub regex: &'static str,
    pub comment: &'static str,
    pub gutter: &'static str,
    pub marker: &'static str,
}

impl ColorScheme {
    /// Get a color scheme with ANSI colors (matching babel-code-frame)
    pub const fn colored() -> Self {
        Self {
            reset: "\x1b[0m",
            keyword: "\x1b[36m",       // cyan
            identifier: "\x1b[33m",    // yellow
            string: "\x1b[32m",        // green
            number: "\x1b[35m",        // magenta
            regex: "\x1b[35m",         // magenta
            comment: "\x1b[90m",       // gray
            gutter: "\x1b[90m",        // gray
            marker: "\x1b[31m\x1b[1m", // red + bold
        }
    }

    /// Get a plain color scheme with no ANSI codes (all empty strings)
    pub const fn plain() -> Self {
        Self {
            reset: "",
            keyword: "",
            identifier: "",
            string: "",
            number: "",
            regex: "",
            comment: "",
            gutter: "",
            marker: "",
        }
    }

    /// Get the color for a token type
    pub fn color_for_token(&self, token_type: TokenType) -> &'static str {
        match token_type {
            TokenType::Keyword => self.keyword,
            TokenType::Identifier => self.identifier,
            TokenType::String => self.string,
            TokenType::Number => self.number,
            TokenType::Regex => self.regex,
            TokenType::Comment => self.comment,
        }
    }
}

// ---------------------------------------------------------------------------
// Shared line-boundary helpers
// ---------------------------------------------------------------------------

/// Compute the byte offset of each line start (0-indexed) in the source.
/// The first entry is always 0 (the start of the first line).
fn compute_line_starts(source: &str) -> Vec<usize> {
    let mut starts = vec![0usize];
    for (i, b) in source.bytes().enumerate() {
        if b == b'\n' {
            starts.push(i + 1);
        }
    }
    starts
}

/// Look up which line (0-indexed) a byte offset falls on via binary search.
fn lookup_line(line_starts: &[usize], byte_offset: usize) -> usize {
    match line_starts.binary_search(&byte_offset) {
        Ok(idx) => idx,
        Err(idx) => idx.saturating_sub(1),
    }
}

/// Get the byte range [start, end) for a given line index (0-indexed).
fn line_bounds(line_starts: &[usize], source_len: usize, line_idx: usize) -> (usize, usize) {
    let start = line_starts.get(line_idx).copied().unwrap_or(source_len);
    let end = line_starts.get(line_idx + 1).copied().unwrap_or(source_len);
    (start, end)
}

/// Push a start+end marker pair for a byte range, splitting at line boundaries
/// for multiline spans. Skips markers outside `byte_range`.
fn add_marker_pair(
    markers: &mut Vec<StyleMarker>,
    line_starts: &[usize],
    source_len: usize,
    start: usize,
    end: usize,
    token_type: TokenType,
    byte_range: (usize, usize),
) {
    if start >= end {
        return;
    }

    let (range_start, range_end) = byte_range;
    if end <= range_start || start >= range_end {
        return;
    }

    let start_line = lookup_line(line_starts, start);
    let end_line = lookup_line(line_starts, end.saturating_sub(1));

    if start_line != end_line {
        for line_idx in start_line..=end_line {
            let (line_start, line_end) = line_bounds(line_starts, source_len, line_idx);
            let marker_start = start.max(line_start);
            let marker_end = end.min(line_end);
            if marker_start < marker_end
                && !(marker_end <= range_start || marker_start >= range_end)
            {
                markers.push(StyleMarker {
                    offset: marker_start,
                    is_start: true,
                    token_type,
                });
                markers.push(StyleMarker {
                    offset: marker_end,
                    is_start: false,
                    token_type,
                });
            }
        }
        return;
    }

    markers.push(StyleMarker {
        offset: start,
        is_start: true,
        token_type,
    });
    markers.push(StyleMarker {
        offset: end,
        is_start: false,
        token_type,
    });
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Extract syntax highlighting markers for source code.
///
/// Uses a language-agnostic byte-scanning tokenizer inspired by the `js-tokens`
/// regex approach. It never fails and produces best-effort highlighting for any
/// input — recognizing quoted strings, comments, numbers, regex literals, and
/// capitalized identifiers.
///
/// # Parameters
/// - `source`: The source code to highlight
/// - `line_range`: Range of line indices (0-indexed, start inclusive, end exclusive). Style markers
///   are only produced for lines within this range. Pass `0..usize::MAX` to produce markers for all
///   lines.
pub fn extract_highlights(
    source: &str,
    line_range: Range<usize>,
    language: Language,
) -> Vec<LineHighlight> {
    let line_starts = compute_line_starts(source);
    let line_count = line_starts.len();

    let byte_range = {
        let start_byte = if line_range.start < line_count {
            line_starts[line_range.start]
        } else {
            usize::MAX
        };

        let end_byte = if line_range.end < line_count {
            line_bounds(&line_starts, source.len(), line_range.end).0
        } else {
            source.len()
        };

        (start_byte, end_byte)
    };

    let mut all_markers = extract_markers(source, &line_starts, byte_range, language);

    all_markers.sort();
    group_markers_by_line(&all_markers, &line_starts, source, line_range)
}

// ---------------------------------------------------------------------------
// Template literal helpers
// ---------------------------------------------------------------------------

/// Scan the content of a template literal (between the backticks), emitting
/// String markers for the quasis (string parts) and recursively tokenizing
/// the expressions inside `${...}` holes using the main tokenizer.
///
/// `tpl_start` is the byte offset of the opening backtick, `tpl_end` is one
/// past the closing backtick (or the end of the regex match for unclosed
/// templates).
fn scan_template_content(
    markers: &mut Vec<StyleMarker>,
    line_starts: &[usize],
    source: &str,
    tpl_start: usize,
    tpl_end: usize,
    byte_range: (usize, usize),
    language: Language,
) {
    let bytes = source.as_bytes();
    // Start after the opening backtick
    let mut i = tpl_start + 1;
    let content_end = if tpl_end > tpl_start && bytes.get(tpl_end - 1) == Some(&b'`') {
        tpl_end - 1
    } else {
        tpl_end
    };
    let len = source.len();

    // Track start of current string segment (includes the backtick/closing brace)
    let mut seg_start = tpl_start;

    while i < content_end {
        if bytes[i] == b'\\' {
            // Skip escape sequence
            i += 2;
            continue;
        }
        if bytes[i] == b'$' && i + 1 < content_end && bytes[i + 1] == b'{' {
            // End the current string segment at the `$` (include everything up to it)
            if i > seg_start {
                add_marker_pair(
                    markers,
                    line_starts,
                    len,
                    seg_start,
                    i,
                    TokenType::String,
                    byte_range,
                );
            }

            // Tokenize the expression using the main scanner with brace_depth=1.
            // It will stop when it sees the matching `}` and return the position
            // after the `}`.
            let expr_start = i + 2;
            let expr_end = scan_tokens(
                markers,
                line_starts,
                source,
                byte_range,
                language,
                expr_start,
                content_end,
                1, // brace_depth: we're inside one `{`
            );

            // The next string segment starts at the closing `}`
            // (expr_end points past the `}`, so the segment starts at expr_end - 1
            // to include the `}` in the string coloring if the expression was closed)
            if expr_end > expr_start && expr_end <= content_end && bytes[expr_end - 1] == b'}' {
                seg_start = expr_end - 1;
            } else {
                // Unclosed expression — no more string segments
                seg_start = expr_end;
            }
            i = expr_end;
            continue;
        }
        i += 1;
    }

    // Emit the final string segment (includes the closing backtick)
    if tpl_end > seg_start {
        add_marker_pair(
            markers,
            line_starts,
            len,
            seg_start,
            tpl_end,
            TokenType::String,
            byte_range,
        );
    }
}

// ---------------------------------------------------------------------------
// Tokenizer (language-agnostic, js-tokens style)
// ---------------------------------------------------------------------------

/// A single compiled regex that matches the next token at each position.
/// Named capture groups identify the token type. The regex is ordered so that
/// earlier alternatives take priority (e.g., `//` and `/*` before `/`).
///
/// Groups:
/// - `string`:  single/double quoted strings with escape handling
/// - `template`: backtick template literals with escape handling
/// - `line_comment`: `// ...` through end of line
/// - `block_comment`: `/* ... */` (non-greedy, spans lines)
/// - `number`:  hex, octal, binary, decimal (with optional exponent)
/// - `ident`:   identifiers (letter/$/_ start, includes non-ASCII)
/// - `close`:   `)` or `]`
/// - `open`:    `(`, `[`, `{`, `}`
/// - `postfix`: `++` or `--`
/// - `slash`:   a lone `/` (regex-or-division — disambiguated by context)
static TOKEN_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r#"(?P<string>"(?:[^"\\]|\\.)*"?|'(?:[^'\\]|\\.)*'?)"#,
        r"|",
        r"(?P<template>`(?:[^`\\]|\\.)*`?)",
        r"|",
        r"(?P<line_comment>//[^\n]*)",
        r"|",
        r"(?P<block_comment>/\*[\s\S]*?\*/)",
        r"|",
        // Numbers: hex, octal, binary, then decimal (with optional fraction/exponent)
        r"(?P<number>0[xX][\da-fA-F]+|0[oO][0-7]+|0[bB][01]+|(?:\d*\.\d+|\d+\.?)(?:[eE][+-]?\d+)?)",
        r"|",
        // Identifiers: letter/$/_/non-ASCII start, continue with alphanumeric/$/_/non-ASCII
        r"(?P<ident>[A-Za-z_$\x80-\xff][\w$\x80-\xff]*)",
        r"|",
        r"(?P<close>[)\]])",
        r"|",
        r"(?P<open>[(\[{}])",
        r"|",
        r"(?P<postfix>\+\+|--)",
        r"|",
        r"(?P<slash>/)",
        r"|",
        // Operators and punctuation: =, +, -, *, %, <, >, &, |, ^, !, ~, ?, :, ;, ,, .
        // Catch-all so we track last_token correctly for regex disambiguation
        r"(?P<op>[=+\-*%<>&|^!~?:;,.])",
    ))
    .expect("token regex must compile")
});

/// Regex that matches a regex literal starting at the opening `/`.
/// Handles character classes `[...]` (where `/` is literal), escape sequences,
/// and flags. Does not match across newlines (regex literals are single-line).
///
/// Structure: `/` then body then `/` then optional flags:
/// - `[^\\/\[\n\r]` — normal chars (not `\`, `/`, `[`, newline)
/// - `\\.`          — escape sequences
/// - `\[(?:[^\]\\\n\r]|\\.)*\]` — character classes with their own escapes
static REGEX_LITERAL_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"/(?:[^\\/\[\n\r]|\\.|\[(?:[^\]\\\n\r]|\\.)*\])+/[A-Za-z]*"#)
        .expect("regex literal regex must compile")
});

/// Group indices in TOKEN_RE, corresponding to the order of named groups.
/// Used for efficient match dispatch without repeated string lookups.
const GRP_STRING: usize = 1;
const GRP_TEMPLATE: usize = 2;
const GRP_LINE_COMMENT: usize = 3;
const GRP_BLOCK_COMMENT: usize = 4;
const GRP_NUMBER: usize = 5;
const GRP_IDENT: usize = 6;
const GRP_CLOSE: usize = 7;
const GRP_OPEN: usize = 8;
const GRP_POSTFIX: usize = 9;
const GRP_SLASH: usize = 10;
const GRP_OP: usize = 11;

/// Find which capture group matched (returns the group index, 1-based).
/// Uses `CaptureLocations` which is O(1) per group check (direct array index).
fn matched_group(locs: &regex::CaptureLocations) -> usize {
    for i in 1..locs.len() {
        if locs.get(i).is_some() {
            return i;
        }
    }
    0
}

/// Extract style markers by scanning source with a single regex.
///
/// Most tokens are handled directly from regex matches. Regex literals are
/// matched by a separate regex at `/` positions where the expression context
/// expects a regex (not division).
fn extract_markers(
    source: &str,
    line_starts: &[usize],
    byte_range: (usize, usize),
    language: Language,
) -> Vec<StyleMarker> {
    let mut markers = Vec::new();
    scan_tokens(
        &mut markers,
        line_starts,
        source,
        byte_range,
        language,
        0,
        source.len(),
        0,
    );
    markers
}

/// Core tokenizer loop. Scans `source[start_pos..scan_end]` and appends style
/// markers.
///
/// When `brace_depth > 0` we are inside a template expression `${...}`. The
/// scanner tracks `{` / `}` tokens and returns as soon as the matching `}`
/// brings the depth back to 0, returning the byte position just past the `}`.
/// Otherwise it returns `scan_end`.
fn scan_tokens(
    markers: &mut Vec<StyleMarker>,
    line_starts: &[usize],
    source: &str,
    byte_range: (usize, usize),
    language: Language,
    start_pos: usize,
    scan_end: usize,
    mut brace_depth: u32,
) -> usize {
    let len = source.len();
    let mut pos = start_pos;
    let mut locs = TOKEN_RE.capture_locations();

    // Track the last non-whitespace, non-comment token kind for regex disambiguation.
    // A `/` following a value or close bracket is division;
    // following an operator or at start of input it's a regex.
    let mut last_token = LastToken::None;

    while TOKEN_RE.captures_read_at(&mut locs, source, pos).is_some() {
        let (start, raw_end) = locs.get(0).unwrap();

        // Stop if the match starts at or past our scan boundary
        if start >= scan_end {
            break;
        }

        // Clamp the match end to scan_end
        let end = raw_end.min(scan_end);

        match matched_group(&locs) {
            GRP_STRING => {
                add_marker_pair(
                    markers,
                    line_starts,
                    len,
                    start,
                    end,
                    TokenType::String,
                    byte_range,
                );
                last_token = LastToken::Value;
            }
            GRP_TEMPLATE => {
                // Split template literal into string parts and expression holes.
                // The quasis are marked as String; expression contents are
                // recursively tokenized via scan_tokens (called from
                // scan_template_content).
                scan_template_content(
                    markers,
                    line_starts,
                    source,
                    start,
                    end,
                    byte_range,
                    language,
                );
                last_token = LastToken::Value;
            }
            GRP_LINE_COMMENT | GRP_BLOCK_COMMENT => {
                add_marker_pair(
                    markers,
                    line_starts,
                    len,
                    start,
                    end,
                    TokenType::Comment,
                    byte_range,
                );
                // Comments don't update last_token
            }
            GRP_POSTFIX => {
                last_token = LastToken::PostfixOp;
            }
            GRP_SLASH => {
                if last_token.expects_regex() {
                    if let Some(re_match) = REGEX_LITERAL_RE.find_at(source, start) {
                        if re_match.start() == start {
                            let re_end = re_match.end().min(scan_end);
                            add_marker_pair(
                                markers,
                                line_starts,
                                len,
                                start,
                                re_end,
                                TokenType::Regex,
                                byte_range,
                            );
                            last_token = LastToken::Value;
                            pos = re_end;
                            continue;
                        }
                    }
                }
                last_token = LastToken::Operator;
            }
            GRP_CLOSE => {
                last_token = LastToken::CloseBracket;
            }
            GRP_OPEN => {
                let ch = source.as_bytes()[start];
                if ch == b'{' {
                    if brace_depth > 0 {
                        brace_depth += 1;
                    }
                } else if ch == b'}' {
                    if brace_depth > 0 {
                        brace_depth -= 1;
                        if brace_depth == 0 {
                            // Found the closing `}` of a template expression.
                            // Return position past the `}` so the caller knows
                            // where the expression ended.
                            return end;
                        }
                    }
                }
                last_token = LastToken::Operator;
            }
            GRP_OP => {
                last_token = LastToken::Operator;
            }
            GRP_NUMBER => {
                add_marker_pair(
                    markers,
                    line_starts,
                    len,
                    start,
                    end,
                    TokenType::Number,
                    byte_range,
                );
                last_token = LastToken::Value;
            }
            GRP_IDENT => {
                let ident = &source[start..end];
                let token_type = if language.is_keyword(ident) {
                    Some(TokenType::Keyword)
                } else if ident.as_bytes()[0].is_ascii_uppercase() {
                    // Highlight capitalized identifiers (matching Babel behavior)
                    Some(TokenType::Identifier)
                } else {
                    None
                };
                if let Some(tt) = token_type {
                    add_marker_pair(markers, line_starts, len, start, end, tt, byte_range);
                }
                last_token = LastToken::Value;
            }
            _ => {}
        }

        debug_assert!(
            raw_end > pos,
            "TOKEN_RE produced a zero-width match at byte {pos}"
        );
        pos = raw_end;
    }

    scan_end
}

/// Tracks the kind of the last non-whitespace, non-comment token for regex
/// disambiguation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LastToken {
    /// Start of input
    None,
    /// Identifier, number, string, regex — values that end expressions
    Value,
    /// `)` or `]` — could end an expression
    CloseBracket,
    /// `++` or `--` — postfix operators end expressions
    PostfixOp,
    /// Operators, open brackets, commas, semicolons, `{`, `}` — regex follows
    Operator,
}

impl LastToken {
    /// Returns true if a `/` at this position should be treated as starting a regex literal.
    fn expects_regex(self) -> bool {
        match self {
            LastToken::None | LastToken::Operator => true,
            LastToken::Value | LastToken::CloseBracket | LastToken::PostfixOp => false,
        }
    }
}

// ---------------------------------------------------------------------------
// Marker → LineHighlight grouping
// ---------------------------------------------------------------------------

/// Group markers by line using line_starts lookup.
/// Complexity: O(markers + lines) using a single pass with marker index.
/// Only returns LineHighlight entries for lines within the specified range.
fn group_markers_by_line(
    markers: &[StyleMarker],
    line_starts: &[usize],
    source: &str,
    line_range: Range<usize>,
) -> Vec<LineHighlight> {
    if source.is_empty() {
        return Vec::new();
    }

    let line_count = line_starts.len();

    let start_line_idx = line_range.start.min(line_count);
    let end_line_idx = line_range.end.min(line_count);

    let output_line_count = end_line_idx.saturating_sub(start_line_idx);
    let mut line_highlights: Vec<LineHighlight> = Vec::with_capacity(output_line_count);

    let mut marker_idx = 0;

    for line_idx in start_line_idx..end_line_idx {
        if line_idx >= line_count {
            break;
        }

        let (line_start, line_end) = line_bounds(line_starts, source.len(), line_idx);

        let mut line_markers = Vec::new();

        while marker_idx < markers.len() {
            let marker = &markers[marker_idx];
            let abs_offset = marker.offset;

            if abs_offset >= line_end {
                break;
            }
            if abs_offset < line_start {
                marker_idx += 1;
                continue;
            }

            let rel_offset = abs_offset - line_start;
            line_markers.push(StyleMarker {
                offset: rel_offset,
                is_start: marker.is_start,
                token_type: marker.token_type,
            });

            marker_idx += 1;
        }

        line_highlights.push(LineHighlight {
            line: line_idx + 1,
            line_start_offset: line_start,
            line_end_offset: line_end,
            markers: line_markers,
        });
    }

    line_highlights
}

// ---------------------------------------------------------------------------
// Truncation adjustment + line rendering
// ---------------------------------------------------------------------------

/// Adjust line highlights for a truncated view of the line.
/// Returns a new LineHighlight with markers adjusted for the truncation offset.
///
/// - `truncation_offset`: byte offset in the original line where visible source content starts
/// - `visible_length`: byte length of the visible content string (including any "..." prefix)
/// - `prefix_len`: byte length of any prefix added before source content (e.g., "..." = 3)
///
/// Markers are shifted so that offset 0 in the adjusted markers corresponds to
/// `prefix_len` in the visible content string.
pub fn adjust_highlights_for_truncation(
    line_highlight: &LineHighlight,
    truncation_offset: usize,
    visible_length: usize,
    prefix_len: usize,
) -> LineHighlight {
    let visible_end = truncation_offset + visible_length.saturating_sub(prefix_len);
    let mut adjusted_markers = Vec::new();

    let start_idx = line_highlight
        .markers
        .partition_point(|m| m.offset < truncation_offset);

    for marker in &line_highlight.markers[start_idx..] {
        if marker.offset > visible_end {
            break;
        }

        adjusted_markers.push(StyleMarker {
            offset: (marker.offset - truncation_offset) + prefix_len,
            is_start: marker.is_start,
            token_type: marker.token_type,
        });
    }

    LineHighlight {
        line: line_highlight.line,
        line_start_offset: line_highlight.line_start_offset + truncation_offset,
        line_end_offset: (line_highlight.line_start_offset + visible_end)
            .min(line_highlight.line_end_offset),
        markers: adjusted_markers,
    }
}

/// Apply highlights to a line of text
/// Returns the styled text with ANSI codes inserted
pub fn apply_line_highlights(
    line: &str,
    line_highlight: &LineHighlight,
    color_scheme: &ColorScheme,
) -> String {
    if line_highlight.markers.is_empty() {
        return line.to_string();
    }

    let mut result = String::with_capacity(line.len() + line_highlight.markers.len() * 10);
    let mut last_offset = 0;
    let mut active_style: Option<TokenType> = None;

    for marker in &line_highlight.markers {
        if marker.offset > last_offset {
            let end = marker.offset.min(line.len());
            result.push_str(&line[last_offset..end]);
            last_offset = end;
        }

        if marker.is_start {
            result.push_str(color_scheme.color_for_token(marker.token_type));
            active_style = Some(marker.token_type);
        } else if active_style == Some(marker.token_type) {
            result.push_str(color_scheme.reset);
            active_style = None;
        }
    }

    if last_offset < line.len() {
        result.push_str(&line[last_offset..]);
    }

    if active_style.is_some() {
        result.push_str(color_scheme.reset);
    }

    result
}

#[cfg(test)]
pub mod tests {
    use super::*;

    /// Default language for tests
    const JS: Language = Language::JavaScript;

    /// Strip ANSI escape codes from a string
    pub fn strip_ansi_codes(s: &str) -> String {
        let mut result = String::with_capacity(s.len());
        let mut chars = s.chars();

        while let Some(ch) = chars.next() {
            if ch == '\x1b' {
                if chars.next() == Some('[') {
                    for ch in chars.by_ref() {
                        if ch.is_alphabetic() {
                            break;
                        }
                    }
                }
            } else {
                result.push(ch);
            }
        }

        result
    }

    // -----------------------------------------------------------------------
    // Basic highlighting tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_apply_line_highlights_basic() {
        let source = "const Foo = 123";
        let highlights = extract_highlights(source, 0..usize::MAX, JS);
        let color_scheme = ColorScheme::colored();

        let result = apply_line_highlights(source, &highlights[0], &color_scheme);

        assert!(result.contains("\x1b["), "Result should contain ANSI codes");
        assert!(result.contains("const"), "Result should contain 'const'");
        assert!(result.contains("Foo"), "Result should contain 'Foo'");
        assert!(result.contains("123"), "Result should contain '123'");
    }

    #[test]
    fn test_apply_line_highlights_plain() {
        let source = "const foo = 123";
        let highlights = extract_highlights(source, 0..usize::MAX, JS);
        let color_scheme = ColorScheme::plain();

        let result = apply_line_highlights(source, &highlights[0], &color_scheme);
        assert_eq!(result, source);
    }

    #[test]
    fn test_only_capitalized_identifiers_highlighted() {
        let source = "const foo = Bar";
        let highlights = extract_highlights(source, 0..usize::MAX, JS);

        let has_identifier = highlights[0]
            .markers
            .iter()
            .any(|m| m.token_type == TokenType::Identifier);
        assert!(has_identifier, "Capitalized 'Bar' should be highlighted");

        let ident_starts: Vec<usize> = highlights[0]
            .markers
            .iter()
            .filter(|m| m.token_type == TokenType::Identifier && m.is_start)
            .map(|m| m.offset)
            .collect();
        assert_eq!(
            ident_starts,
            vec![12],
            "Only 'Bar' at offset 12 should be highlighted"
        );
    }

    #[test]
    fn test_strip_ansi_codes() {
        let input = "\x1b[36mconst\x1b[0m foo = \x1b[35m123\x1b[0m";
        let result = strip_ansi_codes(input);
        assert_eq!(result, "const foo = 123");
    }

    #[test]
    fn test_strip_ansi_codes_preserves_plain_text() {
        let input = "const foo = 123";
        let result = strip_ansi_codes(input);
        assert_eq!(result, input);
    }

    #[test]
    fn test_adjust_highlights_for_truncation() {
        let source = "const Foo = 123";
        let highlights = extract_highlights(source, 0..usize::MAX, JS);

        let adjusted = adjust_highlights_for_truncation(&highlights[0], 6, 9, 0);
        assert!(!adjusted.markers.is_empty());
        assert!(adjusted.markers.iter().any(|m| m.is_start));
    }

    #[test]
    fn test_comments_and_numbers() {
        let source = "const x = 42; // comment\nobj.foo = 10;";
        let highlights = extract_highlights(source, 0..usize::MAX, JS);

        assert_eq!(highlights.len(), 2);

        let line1_has_comment = highlights[0]
            .markers
            .iter()
            .any(|m| m.token_type == TokenType::Comment);
        assert!(line1_has_comment, "First line should have comment markers");

        let line1_has_number = highlights[0]
            .markers
            .iter()
            .any(|m| m.token_type == TokenType::Number);
        let line2_has_number = highlights[1]
            .markers
            .iter()
            .any(|m| m.token_type == TokenType::Number);
        assert!(line1_has_number);
        assert!(line2_has_number);
    }

    #[test]
    fn test_multiline_comment() {
        let source = "const x = 1;\n/* multi\n   line */\nconst y = 2;";
        let highlights = extract_highlights(source, 0..usize::MAX, JS);

        assert_eq!(highlights.len(), 4);

        let line2_has_comment = highlights[1]
            .markers
            .iter()
            .any(|m| m.token_type == TokenType::Comment);
        let line3_has_comment = highlights[2]
            .markers
            .iter()
            .any(|m| m.token_type == TokenType::Comment);

        assert!(line2_has_comment, "Line 2 should have comment marker");
        assert!(line3_has_comment, "Line 3 should have comment marker");
    }

    #[test]
    fn test_multiline_template_literal() {
        let source = "const x = `line1\nline2\nline3`;";
        let highlights = extract_highlights(source, 0..usize::MAX, JS);

        assert_eq!(highlights.len(), 3);

        for (i, highlight) in highlights.iter().enumerate() {
            let has_string = highlight
                .markers
                .iter()
                .any(|m| m.token_type == TokenType::String);
            assert!(
                has_string,
                "Line {} should have string markers for the template literal",
                i + 1
            );
        }
    }

    #[test]
    fn test_template_literal_with_expression() {
        // `hello ${name}!` should mark `hello ` and `!` as string,
        // but NOT mark `name` as string.
        let source = "const x = `hello ${name}!`;";
        let highlights = extract_highlights(source, 0..usize::MAX, JS);

        let string_ranges: Vec<(usize, bool)> = highlights[0]
            .markers
            .iter()
            .filter(|m| m.token_type == TokenType::String)
            .map(|m| (m.offset, m.is_start))
            .collect();

        // Should have two string segments: `hello ${ and }!`
        // The `name` between ${ and } should NOT be in any string range
        assert!(
            string_ranges.len() >= 4,
            "Should have at least 2 string segments (4 markers): got {:?}",
            string_ranges
        );

        // Verify "name" at offset 18..22 is NOT inside any string marker range
        let name_offset = source.find("name").unwrap();
        let name_in_string = highlights[0]
            .markers
            .iter()
            .any(|m| m.token_type == TokenType::String && m.is_start && m.offset == name_offset);
        assert!(
            !name_in_string,
            "'name' should not be marked as part of a string"
        );
    }

    #[test]
    fn test_template_literal_nested() {
        // Nested template literal: `a ${`b ${c}`} d`
        let source = r#"const x = `a ${`b ${c}`} d`;"#;
        let highlights = extract_highlights(source, 0..usize::MAX, JS);

        // Should not panic and should produce some markers
        assert!(!highlights.is_empty());
        let has_string = highlights[0]
            .markers
            .iter()
            .any(|m| m.token_type == TokenType::String);
        assert!(has_string, "Should have string markers");
    }

    #[test]
    fn test_template_literal_no_expressions() {
        // Simple template with no expressions should be one string segment
        let source = "const x = `hello world`;";
        let highlights = extract_highlights(source, 0..usize::MAX, JS);

        let string_starts: Vec<usize> = highlights[0]
            .markers
            .iter()
            .filter(|m| m.token_type == TokenType::String && m.is_start)
            .map(|m| m.offset)
            .collect();

        // Should be a single string segment
        assert_eq!(
            string_starts.len(),
            1,
            "Simple template should be one string segment"
        );
    }

    // -----------------------------------------------------------------------
    // Unbalanced template literal tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_template_unclosed_expression() {
        // `hello ${name` — the `${` is never closed with `}`
        // Should not panic; the string part before `${` should still be marked.
        let source = "const x = `hello ${name";
        let highlights = extract_highlights(source, 0..usize::MAX, JS);
        assert!(!highlights.is_empty(), "Should produce highlights");

        // Should have at least one string marker for the "`hello " part
        let has_string = highlights[0]
            .markers
            .iter()
            .any(|m| m.token_type == TokenType::String);
        assert!(has_string, "Should still mark the string part before ${{");

        // "name" should NOT be marked as string since it's inside an expression hole
        let name_offset = source.find("name").unwrap();
        let name_in_string = highlights[0]
            .markers
            .iter()
            .any(|m| m.token_type == TokenType::String && m.is_start && m.offset == name_offset);
        assert!(
            !name_in_string,
            "'name' inside unclosed expression should not be a string"
        );
    }

    #[test]
    fn test_template_unclosed_backtick() {
        // `hello world  — no closing backtick (regex matches `? at end)
        // Should not panic and should produce some string markers.
        let source = "const x = `hello world";
        let highlights = extract_highlights(source, 0..usize::MAX, JS);
        assert!(!highlights.is_empty(), "Should produce highlights");

        let has_string = highlights[0]
            .markers
            .iter()
            .any(|m| m.token_type == TokenType::String);
        assert!(
            has_string,
            "Unclosed template should still be marked as string"
        );
    }

    #[test]
    fn test_template_unclosed_expression_with_nested_braces() {
        // `value: ${ {a: 1}` — unclosed expression with nested brace inside
        // The inner `{a: 1}` brings depth back to 1, but there's no final `}`
        let source = "const x = `value: ${ {a: 1}`;";
        let highlights = extract_highlights(source, 0..usize::MAX, JS);
        assert!(!highlights.is_empty(), "Should produce highlights");

        // Should have string markers for "`value: " part
        let string_starts: Vec<usize> = highlights[0]
            .markers
            .iter()
            .filter(|m| m.token_type == TokenType::String && m.is_start)
            .map(|m| m.offset)
            .collect();
        assert!(
            !string_starts.is_empty(),
            "Should have string markers for the template"
        );
    }

    #[test]
    fn test_template_brace_in_string_inside_expression() {
        // `${ "}" }` — the `}` inside the string should not close the expression
        let source = r#"const x = `${  "}" } end`;"#;
        let highlights = extract_highlights(source, 0..usize::MAX, JS);
        assert!(!highlights.is_empty());

        // The " end" part after the real closing } should be marked as string
        let end_offset = source.find(" end").unwrap();
        let has_end_string = highlights[0]
            .markers
            .iter()
            .any(|m| m.token_type == TokenType::String && m.is_start && m.offset < end_offset);
        assert!(
            has_end_string,
            "String part after expression should be marked"
        );
    }

    #[test]
    fn test_template_multiple_unclosed_expressions() {
        // `a ${b c ${d` — two unclosed expressions in sequence
        // The first `${` never closes, so the second `${` is inside the first expression
        let source = "const x = `a ${b c ${d";
        let highlights = extract_highlights(source, 0..usize::MAX, JS);
        assert!(
            !highlights.is_empty(),
            "Should not panic on multiple unclosed expressions"
        );
    }

    #[test]
    fn test_template_empty_expression() {
        // `hello ${}world` — empty expression hole
        let source = "const x = `hello ${}world`;";
        let highlights = extract_highlights(source, 0..usize::MAX, JS);
        assert!(!highlights.is_empty());

        // Both "hello " and "world" parts should be string-marked
        let string_starts: Vec<usize> = highlights[0]
            .markers
            .iter()
            .filter(|m| m.token_type == TokenType::String && m.is_start)
            .map(|m| m.offset)
            .collect();
        assert!(
            string_starts.len() >= 2,
            "Empty expression should still split into two string segments, got {:?}",
            string_starts
        );
    }

    #[test]
    fn test_line_range_filtering() {
        let source = "const a = 1;\nconst b = 2;\nconst c = 3;\nconst d = 4;\nconst e = 5;";

        let highlights = extract_highlights(source, 1..4, JS);

        assert_eq!(highlights.len(), 3);
        assert_eq!(highlights[0].line, 2);
        assert_eq!(highlights[1].line, 3);
        assert_eq!(highlights[2].line, 4);

        assert!(!highlights[0].markers.is_empty());
        assert!(!highlights[1].markers.is_empty());
        assert!(!highlights[2].markers.is_empty());
    }

    #[test]
    fn test_line_range_reduces_marker_count() {
        let source = "const a = 1;\nconst b = 2;\nconst c = 3;\nconst d = 4;\nconst e = 5;";

        let all_highlights = extract_highlights(source, 0..usize::MAX, JS);
        let all_marker_count: usize = all_highlights.iter().map(|h| h.markers.len()).sum();

        let filtered_highlights = extract_highlights(source, 2..3, JS);
        let filtered_marker_count: usize =
            filtered_highlights.iter().map(|h| h.markers.len()).sum();

        assert_eq!(filtered_highlights.len(), 1);
        assert!(filtered_marker_count < all_marker_count);
        assert!(filtered_marker_count > 0, "Should still have some markers");
    }

    // -----------------------------------------------------------------------
    // Regex literal tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_regex_after_equals() {
        let source = "const re = /foo/gi;";
        let highlights = extract_highlights(source, 0..usize::MAX, JS);

        let has_regex = highlights[0]
            .markers
            .iter()
            .any(|m| m.token_type == TokenType::Regex);
        assert!(has_regex, "/foo/gi should be highlighted as regex");
    }

    #[test]
    fn test_regex_after_open_paren() {
        let source = "if (/test/.test(x)) {}";
        let highlights = extract_highlights(source, 0..usize::MAX, JS);

        let has_regex = highlights[0]
            .markers
            .iter()
            .any(|m| m.token_type == TokenType::Regex);
        assert!(has_regex, "/test/ should be highlighted as regex");
    }

    #[test]
    fn test_division_not_regex() {
        // After an identifier, `/` is division not regex
        let source = "const x = a / b / c;";
        let highlights = extract_highlights(source, 0..usize::MAX, JS);

        let has_regex = highlights[0]
            .markers
            .iter()
            .any(|m| m.token_type == TokenType::Regex);
        assert!(!has_regex, "a / b / c should not have regex markers");
    }

    #[test]
    fn test_regex_with_char_class() {
        let source = "const re = /[a-z]+/;";
        let highlights = extract_highlights(source, 0..usize::MAX, JS);

        let has_regex = highlights[0]
            .markers
            .iter()
            .any(|m| m.token_type == TokenType::Regex);
        assert!(has_regex, "/[a-z]+/ should be highlighted as regex");
    }

    #[test]
    fn test_regex_at_start_of_line() {
        let source = "/pattern/g.test(str)";
        let highlights = extract_highlights(source, 0..usize::MAX, JS);

        let has_regex = highlights[0]
            .markers
            .iter()
            .any(|m| m.token_type == TokenType::Regex);
        assert!(has_regex, "/pattern/g at start of input should be regex");
    }

    // -----------------------------------------------------------------------
    // Keyword highlighting tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_js_keywords_highlighted() {
        let source = "const foo = function() { return true; }";
        let highlights = extract_highlights(source, 0..usize::MAX, JS);

        let keyword_offsets: Vec<(usize, bool)> = highlights[0]
            .markers
            .iter()
            .filter(|m| m.token_type == TokenType::Keyword)
            .map(|m| (m.offset, m.is_start))
            .collect();

        // "const" at 0..5, "function" at 12..20, "return" at 25..31, "true" at 32..36
        assert!(
            keyword_offsets.contains(&(0, true)),
            "'const' should start at offset 0"
        );
        assert!(
            keyword_offsets.contains(&(12, true)),
            "'function' should start at offset 12"
        );
        assert!(
            keyword_offsets.contains(&(25, true)),
            "'return' should start at offset 25"
        );
        assert!(
            keyword_offsets.contains(&(32, true)),
            "'true' should start at offset 32"
        );
    }

    #[test]
    fn test_js_non_keywords_not_highlighted_as_keyword() {
        let source = "const myVar = something";
        let highlights = extract_highlights(source, 0..usize::MAX, JS);

        // "myVar" and "something" should NOT be highlighted as keywords
        let keyword_starts: Vec<usize> = highlights[0]
            .markers
            .iter()
            .filter(|m| m.token_type == TokenType::Keyword && m.is_start)
            .map(|m| m.offset)
            .collect();

        // Only "const" at offset 0 should be a keyword
        assert_eq!(keyword_starts, vec![0], "Only 'const' should be a keyword");
    }

    #[test]
    fn test_css_no_keywords() {
        let source = "const foo = function() { return true; }";
        let highlights = extract_highlights(source, 0..usize::MAX, Language::Css);

        let has_keyword = highlights[0]
            .markers
            .iter()
            .any(|m| m.token_type == TokenType::Keyword);
        assert!(
            !has_keyword,
            "CSS language should not produce keyword markers"
        );
    }

    // -----------------------------------------------------------------------
    // CSS-like content tests (works via generic tokenizer)
    // -----------------------------------------------------------------------

    #[test]
    fn test_css_basic_highlighting() {
        let source = "div { color: red; }";
        let highlights = extract_highlights(source, 0..usize::MAX, JS);
        let color_scheme = ColorScheme::colored();

        assert_eq!(highlights.len(), 1);
        let stripped = strip_ansi_codes(&apply_line_highlights(
            source,
            &highlights[0],
            &color_scheme,
        ));
        assert_eq!(stripped, source, "Content should be preserved");
    }

    #[test]
    fn test_css_strings_and_comments() {
        let source = "/* a comment */\n.foo { content: 'hello'; }";
        let highlights = extract_highlights(source, 0..usize::MAX, JS);

        assert_eq!(highlights.len(), 2);

        let has_comment = highlights[0]
            .markers
            .iter()
            .any(|m| m.token_type == TokenType::Comment);
        assert!(has_comment, "Should have comment markers");

        let has_string = highlights[1]
            .markers
            .iter()
            .any(|m| m.token_type == TokenType::String);
        assert!(has_string, "Should have string markers");
    }

    #[test]
    fn test_block_comment() {
        let source = "x = 1 /* block\ncomment */ y = 2";
        let highlights = extract_highlights(source, 0..usize::MAX, JS);

        assert_eq!(highlights.len(), 2);

        let line1_has_comment = highlights[0]
            .markers
            .iter()
            .any(|m| m.token_type == TokenType::Comment);
        let line2_has_comment = highlights[1]
            .markers
            .iter()
            .any(|m| m.token_type == TokenType::Comment);

        assert!(line1_has_comment, "Line 1 should have comment marker");
        assert!(line2_has_comment, "Line 2 should have comment marker");
    }
}
