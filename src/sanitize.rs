//! Terminal-safe string handling for untrusted server text.

/// Escape terminal controls and bidirectional formatting characters without altering normal Unicode.
pub fn sanitize(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    for character in input.chars() {
        let code = character as u32;
        if code <= 0x1f || (0x7f..=0x9f).contains(&code) || is_bidi_control(code) {
            use std::fmt::Write as _;
            let _ = write!(output, "\\u{{{code:04X}}}");
        } else {
            output.push(character);
        }
    }
    output
}

fn is_bidi_control(code: u32) -> bool {
    matches!(code, 0x061c | 0x200e | 0x200f | 0x202a..=0x202e | 0x2066..=0x2069)
}

#[cfg(test)]
mod tests {
    use super::sanitize;
    #[test]
    fn escapes_controls_and_bidi() {
        assert_eq!(
            sanitize("a\x1b[31m\u{202e}z\u{0085}"),
            "a\\u{001B}[31m\\u{202E}z\\u{0085}"
        );
    }
    #[test]
    fn keeps_normal_unicode() {
        assert_eq!(sanitize("東京 café"), "東京 café");
    }
}
