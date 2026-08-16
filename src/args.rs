//! Small, strict command-line parser and input limits.

use lexopt::prelude::*;

const MAX_QUERY_BYTES: usize = 4096;
const MAX_FILTER_BYTES: usize = 1024;
const MAX_LANGUAGES: usize = 32;
const MAX_TIMEOUT_SECONDS: u64 = 300;

/// CLI settings after syntax and safety validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Options {
    pub query: String,
    pub match_case: bool,
    pub match_whole_words: bool,
    pub use_regexp: bool,
    pub repo: Option<String>,
    pub path: Option<String>,
    pub languages: Vec<String>,
    pub json: bool,
    pub color: Color,
    pub timeout_seconds: u64,
}

/// Color behavior for human output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Color {
    Auto,
    Always,
    Never,
}

/// Parser outcome that lets the binary handle informational flags without errors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseOutcome {
    Run(Options),
    Help,
    Version,
}

/// Usage text kept compact enough for terminal help.
pub const USAGE: &str = "Usage: grep-cli [OPTIONS] QUERY\n\nSearch public GitHub code through mcp.grep.app.\n\nOptions:\n      --match-case             Match case exactly\n      --match-whole-words      Match whole words only\n      --use-regexp             Treat QUERY as a regular expression\n      --repo VALUE             Limit to a repository\n      --path VALUE             Limit to a file path\n      --language VALUE         Limit to a language (repeatable)\n      --json                   Emit typed JSON results\n      --color auto|always|never Color human output (default: auto)\n      --timeout SECONDS        Network timeout, 1–300 seconds (default: 15)\n  -h, --help                   Show this help\n  -V, --version                Show version\n\nUse `--` before a query that begins with a hyphen.";

/// Parse argv excluding the program name, returning clear user-facing errors.
pub fn parse<I, T>(args: I) -> Result<ParseOutcome, String>
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString>,
{
    let program = std::iter::once(std::ffi::OsString::from("grep-cli"));
    let mut parser = lexopt::Parser::from_iter(program.chain(args.into_iter().map(Into::into)));
    let mut query = None;
    let mut match_case = false;
    let mut match_whole_words = false;
    let mut use_regexp = false;
    let mut repo = None;
    let mut path = None;
    let mut languages = Vec::new();
    let mut json = false;
    let mut color = Color::Auto;
    let mut timeout_seconds = 15;

    while let Some(arg) = parser
        .next()
        .map_err(|e| format!("invalid arguments: {e}"))?
    {
        match arg {
            Long("match-case") => match_case = true,
            Long("match-whole-words") => match_whole_words = true,
            Long("use-regexp") => use_regexp = true,
            Long("repo") => repo = Some(value(&mut parser, "--repo")?),
            Long("path") => path = Some(value(&mut parser, "--path")?),
            Long("language") => {
                if languages.len() == MAX_LANGUAGES {
                    return Err(format!(
                        "at most {MAX_LANGUAGES} --language filters are allowed"
                    ));
                }
                languages.push(value(&mut parser, "--language")?);
            }
            Long("json") => json = true,
            Long("color") => {
                color = match value(&mut parser, "--color")?.as_str() {
                    "auto" => Color::Auto,
                    "always" => Color::Always,
                    "never" => Color::Never,
                    _ => return Err("--color must be auto, always, or never".into()),
                };
            }
            Long("timeout") => {
                let text = value(&mut parser, "--timeout")?;
                timeout_seconds = text
                    .parse()
                    .map_err(|_| "--timeout must be a positive integer")?;
                if timeout_seconds == 0 || timeout_seconds > MAX_TIMEOUT_SECONDS {
                    return Err(format!(
                        "--timeout must be between 1 and {MAX_TIMEOUT_SECONDS} seconds"
                    ));
                }
            }
            Short('h') | Long("help") => return Ok(ParseOutcome::Help),
            Short('V') | Long("version") => return Ok(ParseOutcome::Version),
            Value(v) => {
                let value = v
                    .into_string()
                    .map_err(|_| "arguments must be valid UTF-8")?;
                if query.replace(value).is_some() {
                    return Err("only one QUERY may be supplied".into());
                }
            }
            Long(other) => return Err(format!("unknown option --{other}")),
            Short(other) => return Err(format!("unknown option -{other}")),
        }
    }
    let query = query.ok_or_else(|| "missing QUERY (use --help for usage)".to_owned())?;
    check_nonempty("QUERY", &query)?;
    check_length("QUERY", &query, MAX_QUERY_BYTES)?;
    if let Some(value) = &repo {
        check_nonempty("--repo", value)?;
        check_length("--repo", value, MAX_FILTER_BYTES)?;
    }
    if let Some(value) = &path {
        check_nonempty("--path", value)?;
        check_length("--path", value, MAX_FILTER_BYTES)?;
    }
    for language in &languages {
        check_nonempty("--language", language)?;
        check_length("--language", language, MAX_FILTER_BYTES)?;
    }
    Ok(ParseOutcome::Run(Options {
        query,
        match_case,
        match_whole_words,
        use_regexp,
        repo,
        path,
        languages,
        json,
        color,
        timeout_seconds,
    }))
}

fn value(parser: &mut lexopt::Parser, name: &str) -> Result<String, String> {
    parser
        .value()
        .map_err(|_| format!("{name} requires a value"))?
        .into_string()
        .map_err(|_| format!("{name} must be valid UTF-8"))
}

fn check_nonempty(name: &str, value: &str) -> Result<(), String> {
    if value.is_empty() {
        Err(format!("{name} must not be empty"))
    } else {
        Ok(())
    }
}

fn check_length(name: &str, value: &str, maximum: usize) -> Result<(), String> {
    if value.len() > maximum {
        Err(format!("{name} must be at most {maximum} bytes"))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_double_dash_hyphen_query() {
        let got = parse(["--", "-literal"]).unwrap();
        assert!(matches!(got, ParseOutcome::Run(Options { query, .. }) if query == "-literal"));
    }
    #[test]
    fn rejects_missing_option_value() {
        assert_eq!(parse(["--repo"]).unwrap_err(), "--repo requires a value");
    }
    #[test]
    fn rejects_extra_query() {
        assert_eq!(
            parse(["one", "two"]).unwrap_err(),
            "only one QUERY may be supplied"
        );
    }
    #[test]
    fn rejects_bad_color() {
        assert_eq!(
            parse(["--color", "blue", "x"]).unwrap_err(),
            "--color must be auto, always, or never"
        );
    }
    #[test]
    fn rejects_empty_values() {
        for args in [
            vec![""],
            vec!["--repo", "", "query"],
            vec!["--path", "", "query"],
            vec!["--language", "", "query"],
        ] {
            assert!(parse(args).unwrap_err().contains("must not be empty"));
        }
    }
    #[test]
    fn caps_timeout_at_practical_limit() {
        assert_eq!(
            parse(["--timeout", "301", "query"]).unwrap_err(),
            "--timeout must be between 1 and 300 seconds"
        );
        assert!(matches!(
            parse(["--timeout", "300", "query"]),
            Ok(ParseOutcome::Run(Options {
                timeout_seconds: 300,
                ..
            }))
        ));
    }
}
