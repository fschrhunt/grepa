//! Plain, terminal-safe rendering for typed search results.

use std::io::{self, IsTerminal, Write};

use crate::{args::Color, sanitize::sanitize, SearchResult};

/// Determine whether ANSI decoration is permitted by the requested policy.
pub fn use_color(policy: Color, stdout_is_terminal: bool, no_color_set: bool) -> bool {
    match policy {
        Color::Always => true,
        Color::Never => false,
        Color::Auto => stdout_is_terminal && !no_color_set,
    }
}

/// Write either typed JSON or plain human results, preserving underlying writer error kinds.
pub fn write_results<W: Write>(
    writer: &mut W,
    results: &[SearchResult],
    json: bool,
    color: bool,
) -> io::Result<()> {
    if json {
        serde_json::to_writer(&mut *writer, results).map_err(json_write_error)?;
        writeln!(writer)?;
    } else {
        render_human(writer, results, color)?;
    }
    writer.flush()
}

fn json_write_error(error: serde_json::Error) -> io::Error {
    io::Error::new(error.io_error_kind().unwrap_or(io::ErrorKind::Other), error)
}

/// Render results to any buffered writer; callers may treat BrokenPipe as successful completion.
pub fn render_human<W: Write>(
    writer: &mut W,
    results: &[SearchResult],
    color: bool,
) -> io::Result<()> {
    if results.is_empty() {
        return writeln!(writer, "No results found.");
    }
    for (result_index, result) in results.iter().enumerate() {
        if result_index != 0 {
            writeln!(writer)?;
        }
        label(writer, "Repository", &result.repository, color)?;
        label(writer, "Path", &result.path, color)?;
        label(writer, "URL", &result.url, color)?;
        label(writer, "License", &result.license, color)?;
        for snippet in &result.snippets {
            if color {
                write!(writer, "\x1b[1;36m")?;
            }
            writeln!(writer, "Snippet (line {}):", snippet.line)?;
            if color {
                write!(writer, "\x1b[0m")?;
            }
            for line in snippet.text.split('\n') {
                writeln!(writer, "  {}", sanitize(line))?;
            }
        }
    }
    Ok(())
}

fn label<W: Write>(writer: &mut W, name: &str, value: &str, color: bool) -> io::Result<()> {
    if color {
        write!(writer, "\x1b[1m{name}:\x1b[0m ")?;
    } else {
        write!(writer, "{name}: ")?;
    }
    writeln!(writer, "{}", sanitize(value))
}

/// Whether an output error means a downstream consumer deliberately closed its pipe.
pub fn is_broken_pipe(error: &io::Error) -> bool {
    error.kind() == io::ErrorKind::BrokenPipe
}

/// Check the current stdout terminal state for auto-color callers.
pub fn stdout_is_terminal() -> bool {
    io::stdout().is_terminal()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Snippet;
    #[test]
    fn renders_unicode_without_byte_offsets() {
        let result = SearchResult {
            repository: "東京/r".into(),
            path: "café.rs".into(),
            url: "https://x".into(),
            license: "MIT".into(),
            snippets: vec![Snippet {
                line: 1,
                text: "héllo 世界".into(),
            }],
        };
        let mut output = Vec::new();
        render_human(&mut output, &[result], false).unwrap();
        assert!(String::from_utf8(output).unwrap().contains("héllo 世界"));
    }
    #[test]
    fn color_policy_honors_no_color() {
        assert!(!use_color(Color::Auto, true, true));
        assert!(use_color(Color::Always, false, true));
    }
    struct Broken;
    impl Write for Broken {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::Error::from(io::ErrorKind::BrokenPipe))
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    #[test]
    fn identifies_broken_pipe() {
        let result = SearchResult {
            repository: "r".into(),
            path: "p".into(),
            url: "u".into(),
            license: "l".into(),
            snippets: Vec::new(),
        };
        let error = render_human(&mut Broken, &[result], false).unwrap_err();
        assert!(is_broken_pipe(&error));
    }
    #[test]
    fn writes_live_no_results_message() {
        let mut output = Vec::new();
        render_human(&mut output, &[], false).unwrap();
        assert_eq!(output, b"No results found.\n");
    }
    #[test]
    fn writes_no_results_as_json_array() {
        let mut output = Vec::new();
        write_results(&mut output, &[], true, false).unwrap();
        assert_eq!(output, b"[]\n");
    }
    #[test]
    fn json_output_preserves_broken_pipe() {
        let error = write_results(&mut Broken, &[], true, false).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
    }
    #[test]
    fn human_output_preserves_broken_pipe() {
        let error = write_results(&mut Broken, &[], false, false).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
    }
}
