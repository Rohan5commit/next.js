/// Gets the current terminal width, or returns a default if unavailable.
///
/// Returns 140 as a sensible default when terminal width cannot be detected.
pub fn get_terminal_width() -> usize {
    // Try to detect terminal width from environment or system
    #[cfg(not(target_arch = "wasm32"))]
    {
        // Check COLUMNS environment variable first
        if let Ok(cols) = std::env::var("COLUMNS")
            && let Ok(width) = cols.parse::<usize>()
            && width > 0
        {
            return width;
        }

        // Try platform-specific terminal size detection
        // For now, we'll use a simple approach
        // TODO: Add terminfo/ioctl support if needed
    }

    // Default to 140 characters
    140
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_terminal_width() {
        let width = get_terminal_width();
        assert!(width > 0);
        assert!(width >= 80); // Should be at least 80 chars
    }
}
