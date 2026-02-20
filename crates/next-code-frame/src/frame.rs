use anyhow::{Result, bail};
use serde::Deserialize;

use crate::{
    highlight::{
        ColorScheme, Language, LineHighlight, adjust_highlights_for_truncation,
        apply_line_highlights, extract_highlights,
    },
    terminal::get_terminal_width,
};

/// A source location with line and column
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Location {
    /// 1-indexed line number
    pub line: usize,
    /// 1-indexed column number as byte offset
    #[serde(default)]
    pub column: Option<usize>,
}

/// Location information for the error in the source code
///
/// # Column Semantics
///
/// Columns are 1-indexed **byte offsets** within the line.
///
/// - `start.column`: **Inclusive** - points to the first byte to mark
/// - `end.column`: **EXCLUSIVE** - points one past the last byte to mark
///
/// This follows standard programming range conventions `[start, end)`.
///
/// Example: To mark "123" at byte columns 11-13, use:
/// ```ignore
/// CodeFrameLocation {
///     start: Location { line: 1, column: Some(11) },
///     end: Some(Location { line: 1, column: Some(14) }),  // Exclusive
/// }
/// ```
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodeFrameLocation {
    /// Starting location
    pub start: Location,
    /// Optional ending location
    /// Line is treated inclusively but column is treated exclusively
    pub end: Option<Location>,
}

/// Options for rendering the code frame
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CodeFrameOptions {
    /// Number of lines to show before the error
    pub lines_above: usize,
    /// Number of lines to show after the error
    pub lines_below: usize,
    /// Whether to use color output (named forceColor in the JS API)
    pub force_color: bool,
    /// Whether to attempt syntax highlighting
    pub highlight_code: bool,
    /// Optional message to display with the error
    pub message: Option<String>,
    /// Maximum width for the output (None = terminal width)
    pub max_width: Option<usize>,
    /// Language hint for keyword highlighting
    #[serde(default)]
    pub language: Language,
}

impl Default for CodeFrameOptions {
    fn default() -> Self {
        Self {
            lines_above: 2,
            lines_below: 3,
            force_color: true,
            highlight_code: true,
            message: None,
            max_width: None,
            language: Language::default(),
        }
    }
}

/// Result of applying line truncation.
/// All offsets are in byte space.
struct TruncationResult {
    /// The visible content after truncation (may include "..." prefix/suffix)
    visible_content: String,
    /// The byte offset in the original line where visible source content starts
    byte_offset: usize,
    /// The byte length of any prefix prepended before source content (e.g., "..." = 3)
    prefix_len: usize,
}

fn calculate_marker_position(
    location_start_column: usize,
    end_column: usize,
    line_length: usize,
    line_idx: usize,
    start_line: usize,
    end_line: usize,
    column_offset: usize,
    available_code_width: usize,
) -> (usize, usize) {
    // Allow columns to go one past line length (pointing after last char)
    let max_col = line_length + 1;

    // Determine the column range to mark on this line:
    // We use exclusive ranges [start, end) internally
    //
    // API contract: end.column is ALWAYS exclusive (follows [start, end) convention)
    //
    // For rendering:
    // - Single-line: Mark from start.column to end.column (exclusive)
    // - First line of multiline: Mark from start.column to end of line
    // - Last line of multiline: Mark from column 1 to end.column (exclusive)
    let is_single_line_error = start_line == end_line;

    let (range_start, range_end) = if is_single_line_error {
        // Single-line: end.column is exclusive, use directly
        (location_start_column, end_column)
    } else if line_idx == start_line {
        // First line of multiline: mark from start to end of line
        // line_length already represents the last column position
        (location_start_column, line_length)
    } else {
        // Last line of multiline: mark from column 1 to end.column (exclusive)
        (1, end_column)
    };

    // Clamp to reasonable bounds
    let range_start = range_start.min(max_col);
    // Allow small extension past line for off-by-one, but prevent excessive spans
    let reasonable_max = max_col + 1;
    let range_end = range_end.min(reasonable_max);

    // Calculate marker position accounting for truncation
    // When column_offset > 0, visible content is "...XXXXX" where X starts at column_offset
    // Display positions: columns 1-3 are "...", column 4 corresponds to original column_offset
    let marker_col = if column_offset > 0 {
        // Convert original column to display column
        // formula: display_col = (original_col - offset) + 4
        // where 4 accounts for the "..." prefix (3 chars) plus 1-indexing
        if range_start < column_offset {
            // Error starts before visible window, mark from column 4 (after "...")
            4
        } else {
            // Error starts in visible window
            (range_start - column_offset) + 4
        }
    } else {
        // No truncation, use column as-is
        range_start.max(1)
    };

    // If range is invalid (end <= start), show single marker at start
    let marker_length = if range_end > range_start {
        range_end - range_start
    } else {
        1
    };

    // Adjust marker_length if it would extend past available width
    let marker_length = marker_length.min(available_code_width.saturating_sub(marker_col - 1));

    (marker_col, marker_length)
}

/// Renders a code frame showing the location of an error in source code
pub fn render_code_frame(
    source: &str,
    location: &CodeFrameLocation,
    options: &CodeFrameOptions,
) -> Result<String> {
    if source.is_empty() {
        return Ok(String::new());
    }

    // Split source into lines, preserving trailing empty line.
    // `str::lines()` strips a trailing newline — "foo\n" yields ["foo"] not ["foo", ""].
    // Use `split('\n')` so that error locations pointing at the last line after a
    // trailing newline still resolve correctly. Strip trailing '\r' for Windows line endings.
    let lines: Vec<&str> = source
        .split('\n')
        .map(|l| l.strip_suffix('\r').unwrap_or(l))
        .collect();

    // Validate location
    let start_line = location.start.line.saturating_sub(1); // Convert to 0-indexed
    if start_line >= lines.len() {
        return Ok(String::new());
    }

    let end_line = location
        .end
        .map(|l| l.line.saturating_sub(1).min(lines.len() - 1))
        .unwrap_or(start_line);
    if end_line < start_line {
        bail!("source location invalid end_line {end_line} < {start_line}");
    }

    // Extract the start column (None means no column highlighting)
    let start_column = location.start.column;

    // Normalize end_column:
    // API contract: end.column is always EXCLUSIVE (follows [start, end) convention)
    // - If no end location, default to start.column + 1 (marks 1 char)
    // - Otherwise use the provided end.column as-is (falling back to start + 1 if missing)
    let end_column = match location.end {
        Some(l) => l.column.or(start_column.map(|c| c + 1)),
        None => start_column.map(|c| c + 1),
    };

    // For rendering, we'll clamp columns to valid ranges per-line

    // Calculate window of lines to show
    let first_line = start_line.saturating_sub(options.lines_above);
    let last_line = (end_line + options.lines_below + 1).min(lines.len());

    // Extract syntax highlights if enabled, only for the visible line range
    // first_line and last_line are already 0-indexed, perfect for the API
    let line_highlights: Vec<LineHighlight> = if options.highlight_code {
        extract_highlights(source, first_line..last_line, options.language)
    } else {
        Vec::new()
    };

    // Calculate gutter width (space needed for line numbers)
    let max_line_num = last_line;
    let gutter_width = format!("{}", max_line_num).len();

    let max_width = options.max_width.unwrap_or_else(get_terminal_width);

    // Calculate available width for code (accounting for gutter, markers, and padding)
    // Format: "> N | code" or "  N | code"
    // That's: 1 (marker) + 1 (space) + gutter_width + 3 (" | ")
    let gutter_total_width = 1 + 1 + gutter_width + 3;
    let available_code_width = max_width.saturating_sub(gutter_total_width);
    if available_code_width == 0 {
        bail!("max_width {max_width} too small to render a code frame")
    }

    // Calculate truncation offset (in bytes) for long lines
    // Center the error range if any line in the window needs truncation
    let truncation_offset = calculate_truncation_offset(
        &lines[first_line..last_line],
        start_column.unwrap_or(0),
        end_column.unwrap_or(0),
        available_code_width,
    );

    let color_scheme = if options.force_color {
        ColorScheme::colored()
    } else {
        ColorScheme::plain()
    };
    let mut output = String::new();
    // Track whether we need a newline before the next section.
    // By prepending newlines instead of appending them we avoid a
    // trailing newline that callers would have to strip.
    let mut needs_newline = false;

    // Add message if provided and no column specified
    if let Some(ref message) = options.message
        && start_column.is_none()
    {
        output.push_str(&" ".repeat(gutter_total_width));
        output.push_str(color_scheme.marker);
        output.push_str(message);
        output.push_str(color_scheme.reset);
        needs_newline = true;
    }

    // Render each line
    for (line_idx, line_content) in lines.iter().enumerate().take(last_line).skip(first_line) {
        let is_error_line = line_idx >= start_line && line_idx <= end_line;
        let line_num = line_idx + 1;

        // Apply consistent truncation to all lines (all offsets in bytes)
        let truncation = truncate_line(line_content, truncation_offset, available_code_width);

        // Apply syntax highlighting if enabled
        // line_highlights is indexed relative to first_line (0-indexed within the visible range)
        let highlight_idx = line_idx.saturating_sub(first_line);
        let visible_content = if options.highlight_code && highlight_idx < line_highlights.len() {
            // Adjust highlights for truncation (byte offset, markers are byte-based)
            let adjusted_highlight = adjust_highlights_for_truncation(
                &line_highlights[highlight_idx],
                truncation.byte_offset,
                truncation.visible_content.len(),
                truncation.prefix_len,
            );
            apply_line_highlights(
                &truncation.visible_content,
                &adjusted_highlight,
                &color_scheme,
            )
        } else {
            truncation.visible_content
        };

        // Separate from previous line/section
        if needs_newline {
            output.push('\n');
        }
        needs_newline = true;

        // Line prefix with number
        if is_error_line {
            output.push_str(color_scheme.marker);
            output.push('>');
            output.push_str(color_scheme.reset);
        } else {
            output.push(' ');
        }
        output.push(' ');
        output.push_str(color_scheme.gutter);
        output.push_str(&format!("{:>width$}", line_num, width = gutter_width));
        output.push_str(color_scheme.reset);
        output.push_str(color_scheme.gutter);
        output.push_str(" |");
        output.push_str(color_scheme.reset);

        // Line content (with space separator if not empty)
        if !visible_content.is_empty() {
            output.push(' ');
            output.push_str(&visible_content);
        }

        // Add marker line if this is an error line with column info
        if is_error_line && let Some(start_col) = start_column {
            let (marker_col, marker_length) = calculate_marker_position(
                start_col,
                end_column.unwrap_or(start_col + 1),
                line_content.len(),
                line_idx,
                start_line,
                end_line,
                truncation.byte_offset,
                available_code_width,
            );

            output.push('\n');
            output.push(' ');
            output.push(' ');
            output.push_str(color_scheme.gutter);
            output.push_str(&format!("{:>width$} |", "", width = gutter_width));
            output.push_str(color_scheme.reset);
            output.push(' ');
            output.push_str(&" ".repeat(marker_col - 1));
            output.push_str(color_scheme.marker);
            output.push_str(&"^".repeat(marker_length));
            output.push_str(color_scheme.reset);

            // Add message only on the last error line's marker
            if line_idx == end_line
                && let Some(ref message) = options.message
            {
                output.push(' ');
                output.push_str(color_scheme.marker);
                output.push_str(message);
                output.push_str(color_scheme.reset);
            }
        }
    }

    Ok(output)
}
const ELLIPSIS: &str = "...";
const ELLIPSIS_LEN: usize = 3;

/// Calculate the truncation offset (in bytes) for all lines in the window.
/// This ensures all lines are "scrolled" to the same horizontal position, centering the error
/// range. All column values are byte offsets.
///
/// NOTE: We use byte length (`line.len()`) as a proxy for display width. This is exact for
/// ASCII but overapproximates for multi-byte UTF-8 (e.g., CJK characters are 3 bytes but
/// 2 display columns, emoji are 4 bytes but 1-2 columns). Properly computing display widths is high
/// complexity and requires unicode tables and font information.
fn calculate_truncation_offset(
    lines: &[&str],
    start_column: usize,
    end_column: usize,
    available_width: usize,
) -> usize {
    // Check if any line in the window needs truncation
    let needs_truncation = lines.iter().any(|line| line.len() > available_width);

    // All lines are short enough or we don't have an error column so start at beginning
    if !needs_truncation || start_column == 0 {
        return 0;
    }

    // If we need truncation, center the error range
    // We need to account for the "..." ellipsis (3 chars) on each side
    let available_with_ellipsis = available_width.saturating_sub(2 * ELLIPSIS_LEN);

    // Calculate the midpoint of the error range
    // end_column is exclusive, so the range is [start_column, end_column)
    let start_0idx = start_column.saturating_sub(1);
    let end_0idx = end_column.saturating_sub(1);
    let error_midpoint = (start_0idx + end_0idx) / 2;

    // Try to center the error range in the window
    let half_width = available_with_ellipsis / 2;

    error_midpoint.saturating_sub(half_width)
}

/// Truncate a line at a specific byte offset, adding ellipsis as needed.
/// The `offset` is snapped forward to the nearest UTF-8 character boundary
/// to avoid splitting multi-byte characters.
fn truncate_line(line: &str, offset: usize, max_width: usize) -> TruncationResult {
    // If no offset and line fits, return as-is
    if offset == 0 && line.len() <= max_width {
        return TruncationResult {
            visible_content: line.to_string(),
            byte_offset: 0,
            prefix_len: 0,
        };
    }

    // Snap offset to nearest char boundary (forward)
    let byte_offset = snap_to_char_boundary(line, offset);

    let mut result = String::with_capacity(max_width);

    // Add leading ellipsis if we're starting mid-line
    let prefix_len = if byte_offset > 0 {
        result.push_str(ELLIPSIS);
        ELLIPSIS_LEN
    } else {
        0
    };

    // Calculate how much content we can show (in bytes, approximate)
    let available_content_width = if byte_offset > 0 {
        max_width.saturating_sub(ELLIPSIS_LEN)
    } else {
        max_width
    };

    // Check if offset is past line length
    let remaining_line = if byte_offset < line.len() {
        &line[byte_offset..]
    } else {
        // Offset is past line length - show just ellipsis
        return TruncationResult {
            visible_content: ELLIPSIS.to_string(),
            byte_offset,
            prefix_len: ELLIPSIS_LEN,
        };
    };

    let needs_trailing_ellipsis = remaining_line.len() > available_content_width;
    let content_width = if needs_trailing_ellipsis {
        available_content_width.saturating_sub(ELLIPSIS_LEN)
    } else {
        available_content_width.min(remaining_line.len())
    };

    // Find the largest byte offset <= content_width that is on a char boundary
    let visible_end = snap_to_char_boundary_back(remaining_line, content_width);

    result.push_str(&remaining_line[..visible_end]);

    if needs_trailing_ellipsis {
        result.push_str(ELLIPSIS);
    }

    TruncationResult {
        visible_content: result,
        byte_offset,
        prefix_len,
    }
}

/// Snap a byte offset forward to the nearest UTF-8 character boundary.
/// If `offset` is already on a boundary, returns it unchanged.
/// If `offset >= line.len()`, returns `line.len()`.
fn snap_to_char_boundary(line: &str, offset: usize) -> usize {
    if offset >= line.len() {
        return line.len();
    }
    // Find the first char boundary at or after offset
    let mut pos = offset;
    while pos < line.len() && !line.is_char_boundary(pos) {
        pos += 1;
    }
    pos
}

/// Snap a byte offset backward to the nearest UTF-8 character boundary.
/// Returns the largest value <= `offset` that is a valid char boundary.
fn snap_to_char_boundary_back(line: &str, offset: usize) -> usize {
    let offset = offset.min(line.len());
    let mut pos = offset;
    while pos > 0 && !line.is_char_boundary(pos) {
        pos -= 1;
    }
    pos
}
