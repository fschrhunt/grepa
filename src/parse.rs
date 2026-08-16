//! Stateful parser for grep.app's human-formatted search tool text.

use crate::{sanitize::sanitize, SearchResult, Snippet};

const MAX_RESULTS: usize = 200;
const MAX_SNIPPETS_PER_RESULT: usize = 200;
const MAX_SNIPPET_BYTES: usize = 256 * 1024;

/// Parse all text content items from a successful tool call into typed, safe results.
pub fn parse_search_texts(texts: &[String]) -> Result<Vec<SearchResult>, String> {
    let mut results = Vec::new();
    for text in texts {
        if text.len() > MAX_SNIPPET_BYTES * 2 {
            return Err("server text item is too large".into());
        }
        let mut parsed = parse_one(text)?;
        if results.len() + parsed.len() > MAX_RESULTS {
            return Err("server returned too many results".into());
        }
        results.append(&mut parsed);
    }
    Ok(results)
}

fn parse_one(text: &str) -> Result<Vec<SearchResult>, String> {
    if text.trim() == "No results found for your query." || text.trim().is_empty() {
        return Ok(Vec::new());
    }
    let lines: Vec<&str> = text.lines().collect();
    let mut results = Vec::new();
    let mut current: Option<SearchResult> = None;
    let mut current_snippet: Option<PendingSnippet> = None;
    let mut index = 0;

    while index < lines.len() {
        if let Some((repository, path, url, license)) = result_header(&lines, index) {
            finish_snippet(&mut current, &mut current_snippet)?;
            if let Some(result) = current.take() {
                results.push(result);
            }
            current = Some(SearchResult {
                repository: sanitize(repository),
                path: sanitize(path),
                url: sanitize(url),
                license: sanitize(license),
                snippets: Vec::new(),
            });
            index += 6;
            continue;
        }
        let result = current.as_mut().ok_or_else(|| {
            "server search text does not begin with a complete result header".to_owned()
        })?;
        if lines[index] == "Snippets:" {
            index += 1;
            continue;
        }
        if let Some(line) = snippet_header(lines[index]) {
            finish_snippet(&mut current, &mut current_snippet)?;
            current_snippet = Some(PendingSnippet {
                snippet: Snippet {
                    line,
                    text: String::new(),
                },
                has_source_lines: false,
            });
        } else if let Some(snippet) = current_snippet.as_mut() {
            if snippet.has_source_lines {
                snippet.snippet.text.push('\n');
            }
            snippet.snippet.text.push_str(lines[index]);
            snippet.has_source_lines = true;
            if snippet.snippet.text.len() > MAX_SNIPPET_BYTES {
                return Err("server snippet is too large".into());
            }
        } else if !lines[index].is_empty() {
            return Err(format!(
                "unexpected server text before snippets: {}",
                sanitize(lines[index])
            ));
        } else if result.snippets.len() > MAX_SNIPPETS_PER_RESULT {
            return Err("server returned too many snippets".into());
        }
        index += 1;
    }
    finish_snippet(&mut current, &mut current_snippet)?;
    if let Some(result) = current {
        results.push(result);
    }
    if results.is_empty() {
        return Err("server search text contained no results".into());
    }
    Ok(results)
}

/// A snippet being assembled, retaining whether an empty first source line was received.
struct PendingSnippet {
    snippet: Snippet,
    has_source_lines: bool,
}

fn finish_snippet(
    result: &mut Option<SearchResult>,
    snippet: &mut Option<PendingSnippet>,
) -> Result<(), String> {
    if let Some(PendingSnippet { mut snippet, .. }) = snippet.take() {
        let result = result
            .as_mut()
            .ok_or_else(|| "snippet without result".to_owned())?;
        if result.snippets.len() == MAX_SNIPPETS_PER_RESULT {
            return Err("server returned too many snippets".into());
        }
        // Newlines are structural delimiters; every server-controlled line is escaped independently.
        snippet.text = snippet
            .text
            .split('\n')
            .map(sanitize)
            .collect::<Vec<_>>()
            .join("\n");
        result.snippets.push(snippet);
    }
    Ok(())
}

/// A result begins only with the entire six-line metadata sequence, not a metadata-looking line in code.
fn result_header<'a>(lines: &'a [&str], at: usize) -> Option<(&'a str, &'a str, &'a str, &'a str)> {
    let tail = lines.get(at..at + 6)?;
    Some((
        tail[0].strip_prefix("Repository: ")?,
        tail[1].strip_prefix("Path: ")?,
        tail[2].strip_prefix("URL: ")?,
        tail[3].strip_prefix("License: ")?,
    ))
    .filter(|(repository, path, url, license)| {
        !repository.is_empty()
            && !path.is_empty()
            && !url.is_empty()
            && !license.is_empty()
            && tail[4].is_empty()
            && tail[5] == "Snippets:"
    })
}

fn snippet_header(line: &str) -> Option<u64> {
    let middle = line.strip_prefix("--- Snippet ")?.strip_suffix(" ---")?;
    let (ordinal, line) = middle.rsplit_once(" (Line ")?;
    ordinal.parse::<u64>().ok().filter(|ordinal| *ordinal > 0)?;
    line.strip_suffix(')')?
        .parse()
        .ok()
        .filter(|line: &u64| *line > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn keeps_metadata_like_code_inside_a_snippet() {
        let text = "Repository: owner/repo\nPath: a.rs\nURL: https://example.test/a\nLicense: MIT\n\nSnippets:\n--- Snippet 1 (Line 4) ---\nRepository: not a header\nPath: code\nlet x = 1;";
        let results = parse_search_texts(&[text.into()]).unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].snippets[0]
            .text
            .contains("Repository: not a header"));
    }
    #[test]
    fn recognizes_complete_next_result_header() {
        let text = "Repository: a/r\nPath: a\nURL: https://a\nLicense: MIT\n\nSnippets:\n--- Snippet 1 (Line 1) ---\nx\nRepository: b/r\nPath: b\nURL: https://b\nLicense: MIT\n\nSnippets:\n--- Snippet 1 (Line 2) ---\ny";
        assert_eq!(parse_search_texts(&[text.into()]).unwrap().len(), 2);
    }
    #[test]
    fn parses_live_no_results_text() {
        assert_eq!(
            parse_search_texts(&["No results found for your query.".into()]).unwrap(),
            []
        );
    }
    #[test]
    fn requires_a_positive_numeric_snippet_ordinal() {
        for header in [
            "--- Snippet first (Line 1) ---",
            "--- Snippet 0 (Line 1) ---",
            "--- Snippet 1.5 (Line 1) ---",
        ] {
            assert_eq!(snippet_header(header), None);
        }
    }
    #[test]
    fn preserves_leading_empty_snippet_lines() {
        let text = "Repository: owner/repo\nPath: a.rs\nURL: https://example.test/a\nLicense: MIT\n\nSnippets:\n--- Snippet 1 (Line 4) ---\n\nfirst code line";
        let results = parse_search_texts(&[text.into()]).unwrap();
        assert_eq!(results[0].snippets[0].text, "\nfirst code line");
    }
}
