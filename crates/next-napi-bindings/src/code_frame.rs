use napi::bindgen_prelude::*;
use napi_derive::napi;
use next_code_frame::{CodeFrameLocation, CodeFrameOptions, Language, Location, render_code_frame};

#[napi(object)]
pub struct NapiLocation {
    pub line: u32,
    pub column: Option<u32>,
}

impl From<NapiLocation> for Location {
    fn from(loc: NapiLocation) -> Self {
        Location {
            line: loc.line as usize,
            column: loc.column.map(|c| c as usize),
        }
    }
}

#[napi(object)]
pub struct NapiCodeFrameLocation {
    pub start: NapiLocation,
    pub end: Option<NapiLocation>,
}

impl From<NapiCodeFrameLocation> for CodeFrameLocation {
    fn from(loc: NapiCodeFrameLocation) -> Self {
        CodeFrameLocation {
            start: loc.start.into(),
            end: loc.end.map(Into::into),
        }
    }
}

#[napi(object)]
#[derive(Default)]
pub struct NapiCodeFrameOptions {
    /// Number of lines to show above the error (default: 2)
    pub lines_above: Option<u32>,
    /// Number of lines to show below the error (default: 3)
    pub lines_below: Option<u32>,
    /// Maximum width of the output (default: terminal width)
    pub max_width: Option<u32>,
    /// Whether to use ANSI colors (default: true)
    pub force_color: Option<bool>,
    /// Whether to highlight code syntax (default: true)
    pub highlight_code: Option<bool>,
    /// Optional message to display with the code frame
    pub message: Option<String>,
    /// Language hint for keyword highlighting: "javascript" (default) or "css"
    pub language: Option<String>,
}

fn parse_language(s: &Option<String>) -> Language {
    match s.as_deref() {
        Some("css") => Language::Css,
        _ => Language::JavaScript,
    }
}

impl From<NapiCodeFrameOptions> for CodeFrameOptions {
    fn from(opts: NapiCodeFrameOptions) -> Self {
        CodeFrameOptions {
            lines_above: opts.lines_above.unwrap_or(2) as usize,
            lines_below: opts.lines_below.unwrap_or(3) as usize,
            max_width: opts.max_width.map(|w| w as usize),
            force_color: opts.force_color.unwrap_or(true),
            highlight_code: opts.highlight_code.unwrap_or(true),
            message: opts.message,
            language: parse_language(&opts.language),
        }
    }
}

/// Renders a code frame showing the location of an error in source code
///
/// This is a Rust implementation that replaces Babel's code-frame for better:
/// - Performance on large files
/// - Handling of long lines
/// - Memory efficiency
///
/// # Arguments
/// * `source` - The source code to render
/// * `location` - The location to highlight (line and column numbers are 1-indexed)
/// * `options` - Optional configuration
///
/// # Returns
/// The formatted code frame string, or empty string if the location is invalid
#[napi]
pub fn code_frame_columns(
    source: String,
    location: NapiCodeFrameLocation,
    options: Option<NapiCodeFrameOptions>,
) -> Result<String> {
    let code_frame_location: CodeFrameLocation = location.into();
    let code_frame_options: CodeFrameOptions = options.unwrap_or_default().into();

    render_code_frame(&source, &code_frame_location, &code_frame_options)
        .map_err(|e| Error::from_reason(format!("Failed to render code frame: {e:?}")))
}
