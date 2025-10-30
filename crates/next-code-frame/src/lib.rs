#![doc = include_str!("../README.md")]

mod frame;
mod highlight;
mod terminal;

pub use frame::{CodeFrameLocation, CodeFrameOptions, Location, render_code_frame};
pub use highlight::Language;
pub use terminal::get_terminal_width;

#[cfg(test)]
mod tests;
