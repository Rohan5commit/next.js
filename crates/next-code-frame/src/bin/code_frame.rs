use std::{fs, process::ExitCode};

use next_code_frame::{CodeFrameLocation, CodeFrameOptions, Location, render_code_frame};

fn usage() -> ExitCode {
    eprintln!("Usage: code_frame [OPTIONS] <file> <start_line:start_col> [end_line:end_col]");
    eprintln!();
    eprintln!("Render a code frame for the given file and position.");
    eprintln!("Lines and columns are 1-indexed.");
    eprintln!();
    eprintln!("Options:");
    eprintln!("  -m, --message <MSG>  Message to display with the error");
    eprintln!();
    eprintln!("Examples:");
    eprintln!("  code_frame src/app.tsx 10:5");
    eprintln!("  code_frame src/app.tsx 10:5 10:20");
    eprintln!("  code_frame -m \"Unexpected token\" src/app.tsx 10:5 10:20");
    ExitCode::FAILURE
}

fn parse_position(s: &str) -> Option<Location> {
    let (line, col) = s.split_once(':')?;
    Some(Location {
        line: line.parse().ok()?,
        column: Some(col.parse().ok()?),
    })
}

fn main() -> ExitCode {
    let raw_args: Vec<String> = std::env::args().skip(1).collect();

    // Parse optional -m/--message flag
    let mut message: Option<String> = None;
    let mut positional = Vec::new();
    let mut iter = raw_args.iter();
    while let Some(arg) = iter.next() {
        if arg == "-m" || arg == "--message" {
            match iter.next() {
                Some(val) => message = Some(val.clone()),
                None => {
                    eprintln!("Missing value for {arg}");
                    return usage();
                }
            }
        } else {
            positional.push(arg.as_str());
        }
    }

    if positional.len() < 2 || positional.len() > 3 {
        return usage();
    }

    let file = positional[0];
    let source = match fs::read_to_string(file) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error reading {file}: {e}");
            return ExitCode::FAILURE;
        }
    };

    let start = match parse_position(positional[1]) {
        Some(loc) => loc,
        None => {
            eprintln!("Invalid start position: {}", positional[1]);
            return usage();
        }
    };

    let end = if positional.len() == 3 {
        match parse_position(positional[2]) {
            Some(loc) => Some(loc),
            None => {
                eprintln!("Invalid end position: {}", positional[2]);
                return usage();
            }
        }
    } else {
        None
    };

    let location = CodeFrameLocation { start, end };
    let options = CodeFrameOptions {
        message,
        ..Default::default()
    };

    match render_code_frame(&source, &location, &options) {
        Ok(frame) => {
            println!("{frame}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("Error rendering code frame: {e}");
            ExitCode::FAILURE
        }
    }
}
