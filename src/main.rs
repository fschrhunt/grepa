//! Executable entry point for the grep.app MCP search CLI.

use std::{
    fmt,
    io::{self, BufWriter, Write},
};

use grep_cli::{
    args::{self, ParseOutcome},
    parse::parse_search_texts,
    render,
    transport::McpClient,
};

/// A user-facing failure with the conventional CLI exit status it requires.
enum AppError {
    Usage(String),
    Runtime(String),
}

impl AppError {
    fn exit_code(&self) -> i32 {
        match self {
            Self::Usage(_) => 2,
            Self::Runtime(_) => 1,
        }
    }
}

impl fmt::Display for AppError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Usage(message) | Self::Runtime(message) => formatter.write_str(message),
        }
    }
}

fn main() {
    if let Err(error) = run() {
        eprintln!("grep-cli: {error}");
        std::process::exit(error.exit_code());
    }
}

/// Run one validated search, treating a closed stdout pipe as normal Unix completion.
fn run() -> Result<(), AppError> {
    let options = match args::parse(std::env::args_os().skip(1)).map_err(AppError::Usage)? {
        ParseOutcome::Help => return write_info(args::USAGE),
        ParseOutcome::Version => {
            return write_info(concat!("grep-cli ", env!("CARGO_PKG_VERSION")))
        }
        ParseOutcome::Run(options) => options,
    };
    let mut client = McpClient::new(options.timeout_seconds).map_err(AppError::Runtime)?;
    let texts = client.search(&options).map_err(AppError::Runtime)?;
    let results = parse_search_texts(&texts).map_err(AppError::Runtime)?;
    let stdout = io::stdout();
    let mut output = BufWriter::new(stdout.lock());
    let color = render::use_color(
        options.color,
        render::stdout_is_terminal(),
        std::env::var_os("NO_COLOR").is_some(),
    );
    match render::write_results(&mut output, &results, options.json, color) {
        Ok(()) => Ok(()),
        Err(error) if render::is_broken_pipe(&error) => Ok(()),
        Err(error) => Err(AppError::Runtime(format!(
            "could not write output: {error}"
        ))),
    }
}

fn write_info(message: &str) -> Result<(), AppError> {
    let mut output = BufWriter::new(io::stdout().lock());
    match writeln!(output, "{message}").and_then(|_| output.flush()) {
        Ok(()) => Ok(()),
        Err(error) if render::is_broken_pipe(&error) => Ok(()),
        Err(error) => Err(AppError::Runtime(format!(
            "could not write output: {error}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::AppError;

    #[test]
    fn uses_conventional_error_exit_codes() {
        assert_eq!(AppError::Usage("bad arguments".into()).exit_code(), 2);
        assert_eq!(AppError::Runtime("network failure".into()).exit_code(), 1);
    }
}
