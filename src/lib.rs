//! Core parsing, transport, rendering, and argument handling for grep-app-lite.

pub mod args;
pub mod parse;
pub mod render;
pub mod sanitize;
pub mod transport;

use serde::Serialize;

/// A search result returned by grep.app's formatted MCP tool response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SearchResult {
    pub repository: String,
    pub path: String,
    pub url: String,
    pub license: String,
    pub snippets: Vec<Snippet>,
}

/// One source excerpt and the source line at which it starts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Snippet {
    pub line: u64,
    pub text: String,
}
