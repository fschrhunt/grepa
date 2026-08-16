//! Minimal blocking JSON-RPC-over-HTTP transport for the official grep.app MCP endpoint.

use std::{io::Read, time::Duration};

use serde_json::{json, Value};

use crate::{args::Options, sanitize::sanitize};

const ENDPOINT: &str = "https://mcp.grep.app/";
const PROTOCOL_VERSION: &str = "2025-06-18";
const MAX_BODY_BYTES: usize = 2 * 1024 * 1024;

/// A client that performs exactly the MCP initialization and one search tool call.
pub struct McpClient {
    agent: ureq::Agent,
    session_id: Option<String>,
}

impl McpClient {
    /// Create a bounded-time client that never follows redirects away from the fixed endpoint.
    pub fn new(timeout_seconds: u64) -> Result<Self, String> {
        let config = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(timeout_seconds)))
            .max_redirects(0)
            .user_agent(concat!("grepa/", env!("CARGO_PKG_VERSION")))
            .http_status_as_error(false)
            .build();
        Ok(Self {
            agent: config.new_agent(),
            session_id: None,
        })
    }

    /// Initialize the MCP session, send initialized, and invoke the official searchGitHub tool.
    pub fn search(&mut self, options: &Options) -> Result<Vec<String>, String> {
        let initialize = json!({
            "jsonrpc": "2.0", "id": 1, "method": "initialize",
            "params": {"protocolVersion": PROTOCOL_VERSION, "capabilities": {}, "clientInfo": {"name": "grepa", "version": env!("CARGO_PKG_VERSION")}}
        });
        let initialize_result = self.request(Some(1), &initialize)?;
        validate_initialize_result(&initialize_result)?;
        let initialized =
            json!({"jsonrpc":"2.0", "method":"notifications/initialized", "params": {}});
        self.request(None, &initialized)?;
        let mut arguments = serde_json::Map::new();
        arguments.insert("query".into(), Value::String(options.query.clone()));
        arguments.insert("matchCase".into(), Value::Bool(options.match_case));
        arguments.insert(
            "matchWholeWords".into(),
            Value::Bool(options.match_whole_words),
        );
        arguments.insert("useRegexp".into(), Value::Bool(options.use_regexp));
        if let Some(value) = &options.repo {
            arguments.insert("repo".into(), Value::String(value.clone()));
        }
        if let Some(value) = &options.path {
            arguments.insert("path".into(), Value::String(value.clone()));
        }
        if !options.languages.is_empty() {
            arguments.insert("language".into(), json!(options.languages));
        }
        let call = json!({"jsonrpc":"2.0", "id": 2, "method":"tools/call", "params":{"name":"searchGitHub", "arguments": arguments}});
        let result = self.request(Some(2), &call)?;
        tool_texts(&result)
    }

    fn request(&mut self, expected_id: Option<u64>, payload: &Value) -> Result<Value, String> {
        let mut request = self
            .agent
            .post(ENDPOINT)
            .header("Accept", "application/json, text/event-stream")
            .header("Content-Type", "application/json")
            .header("MCP-Protocol-Version", PROTOCOL_VERSION);
        if let Some(session_id) = &self.session_id {
            request = request.header("Mcp-Session-Id", session_id);
        }
        let response = request
            .send_json(payload)
            .map_err(|error| format!("MCP request failed: {error}"))?;
        if let Some(session_id) = response
            .headers()
            .get("mcp-session-id")
            .and_then(|value| value.to_str().ok())
        {
            if session_id.len() > 1024 {
                return Err("server supplied an oversized MCP session id".into());
            }
            self.session_id = Some(session_id.to_owned());
        }
        if !(200..300).contains(&response.status().as_u16()) {
            return Err(format!("MCP server returned HTTP {}", response.status()));
        }
        let is_sse = response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            .map(|value| {
                value
                    .split(';')
                    .next()
                    .unwrap_or("")
                    .trim()
                    .eq_ignore_ascii_case("text/event-stream")
            })
            .unwrap_or(false);
        let reader = response.into_body().into_reader();
        let mut body = Vec::new();
        reader
            .take((MAX_BODY_BYTES + 1) as u64)
            .read_to_end(&mut body)
            .map_err(|error| format!("could not read MCP response: {error}"))?;
        if body.len() > MAX_BODY_BYTES {
            return Err("MCP response exceeds 2 MiB limit".into());
        }
        let body = std::str::from_utf8(&body).map_err(|_| "MCP response is not UTF-8")?;
        // Notifications have no response by JSON-RPC contract; grep.app returns an empty body.
        if expected_id.is_none() {
            return Ok(Value::Null);
        }
        let messages = if is_sse {
            parse_sse(body)?
        } else {
            vec![serde_json::from_str(body).map_err(|error| format!("invalid MCP JSON: {error}"))?]
        };
        check_response(messages, expected_id)
    }
}

fn validate_initialize_result(result: &Value) -> Result<(), String> {
    let result = result
        .as_object()
        .ok_or_else(|| "initialize result must be an object".to_owned())?;
    if result.get("protocolVersion").and_then(Value::as_str) != Some(PROTOCOL_VERSION) {
        return Err(format!(
            "MCP server protocolVersion must be exactly {PROTOCOL_VERSION}"
        ));
    }
    let server_info = result
        .get("serverInfo")
        .and_then(Value::as_object)
        .ok_or_else(|| "initialize result serverInfo must be an object".to_owned())?;
    for field in ["name", "version"] {
        if server_info
            .get(field)
            .and_then(Value::as_str)
            .is_none_or(str::is_empty)
        {
            return Err(format!(
                "initialize result serverInfo.{field} must be a non-empty string"
            ));
        }
    }
    if !result.get("capabilities").is_some_and(Value::is_object) {
        return Err("initialize result capabilities must be an object".into());
    }
    Ok(())
}

fn tool_texts(result: &Value) -> Result<Vec<String>, String> {
    if let Some(is_error) = result.get("isError") {
        let is_error = is_error
            .as_bool()
            .ok_or_else(|| "tool result isError must be a boolean".to_owned())?;
        if is_error {
            return Err(format!(
                "search tool error: {}",
                result
                    .get("content")
                    .map(compact_json)
                    .unwrap_or_else(|| "no details".into())
            ));
        }
    }
    let content = result
        .get("content")
        .and_then(Value::as_array)
        .ok_or_else(|| "tool result has no content array".to_owned())?;
    if content.len() > 200 {
        return Err("tool result has too many content items".into());
    }
    content
        .iter()
        .map(|item| {
            if item.get("type").and_then(Value::as_str) != Some("text") {
                return Err("tool result contains a non-text item".into());
            }
            item.get("text")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .ok_or_else(|| "tool text item has no text".to_owned())
        })
        .collect()
}

fn compact_json(value: &Value) -> String {
    sanitize(&serde_json::to_string(value).unwrap_or_else(|_| "unprintable details".into()))
}

fn check_response(messages: Vec<Value>, expected_id: Option<u64>) -> Result<Value, String> {
    let expected_id = expected_id.ok_or_else(|| "response id was not expected".to_owned())?;
    let mut result = None;
    for message in messages {
        let object = message
            .as_object()
            .ok_or_else(|| "MCP message must be a JSON object".to_owned())?;
        if object.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
            return Err("invalid JSON-RPC version in response".into());
        }
        match (object.get("method"), object.get("id")) {
            (Some(method), None) => {
                if method
                    .as_str()
                    .filter(|method| !method.is_empty())
                    .is_none()
                {
                    return Err("invalid JSON-RPC notification method".into());
                }
                if object.contains_key("result") || object.contains_key("error") {
                    return Err("invalid JSON-RPC notification payload".into());
                }
            }
            (Some(_), Some(_)) => return Err("unsupported server JSON-RPC request".into()),
            (None, Some(id)) => {
                if id.as_u64() != Some(expected_id) {
                    return Err("JSON-RPC response id did not match request".into());
                }
                if let Some(error) = object.get("error") {
                    if object.contains_key("result") {
                        return Err("MCP response has both result and error".into());
                    }
                    return Err(format!("MCP JSON-RPC error: {}", compact_json(error)));
                }
                let response = object
                    .get("result")
                    .ok_or_else(|| "MCP response has neither result nor error".to_owned())?;
                if result.replace(response.clone()).is_some() {
                    return Err("MCP response contained multiple results for one request".into());
                }
            }
            (None, None) => return Err("MCP message has neither method nor id".into()),
        }
    }
    result.ok_or_else(|| "MCP response contained no response for request".into())
}

/// Decode SSE records, collecting each message's one or more `data:` lines.
pub fn parse_sse(input: &str) -> Result<Vec<Value>, String> {
    let normalized = input.replace("\r\n", "\n").replace('\r', "\n");
    let mut values = Vec::new();
    for record in normalized.split("\n\n") {
        let data: Vec<&str> = record
            .lines()
            .filter_map(|line| {
                line.strip_prefix("data:")
                    .map(|data| data.strip_prefix(' ').unwrap_or(data))
            })
            .collect();
        if data.is_empty() {
            continue;
        }
        let joined = data.join("\n");
        if joined == "[DONE]" {
            continue;
        }
        values.push(
            serde_json::from_str(&joined).map_err(|error| format!("invalid SSE JSON: {error}"))?,
        );
    }
    if values.is_empty() {
        return Err("SSE response contained no JSON messages".into());
    }
    Ok(values)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn decodes_sse_data_lines() {
        let values = parse_sse(
            "event: message\ndata: {\"jsonrpc\":\"2.0\",\ndata: \"id\":1,\"result\":{}}\n\n",
        )
        .unwrap();
        assert_eq!(values[0]["id"], 1);
    }
    #[test]
    fn rejects_rpc_error() {
        assert!(check_response(
            vec![json!({"jsonrpc":"2.0","id":2,"error":{"code":-1}})],
            Some(2)
        )
        .unwrap_err()
        .contains("JSON-RPC error"));
    }
    #[test]
    fn rejects_tool_error() {
        assert!(tool_texts(&json!({"isError": true, "content": []}))
            .unwrap_err()
            .contains("search tool error"));
    }
    #[test]
    fn rejects_wrong_id() {
        assert!(
            check_response(vec![json!({"jsonrpc":"2.0","id":3,"result":{}})], Some(2)).is_err()
        );
    }
    #[test]
    fn ignores_notifications_while_selecting_expected_response() {
        let result = check_response(
            vec![
                json!({"jsonrpc":"2.0", "method":"notifications/progress", "params": {}}),
                json!({"jsonrpc":"2.0", "id":2, "result":{"ok":true}}),
            ],
            Some(2),
        )
        .unwrap();
        assert_eq!(result, json!({"ok": true}));
    }
    #[test]
    fn rejects_server_requests_and_mismatched_responses() {
        let server_request = check_response(
            vec![json!({"jsonrpc":"2.0", "id":2, "method":"ping", "params": {}})],
            Some(2),
        )
        .unwrap_err();
        assert!(server_request.contains("unsupported server"));
        let mismatched = check_response(
            vec![
                json!({"jsonrpc":"2.0", "method":"notifications/progress"}),
                json!({"jsonrpc":"2.0", "id":3, "result":{}}),
            ],
            Some(2),
        )
        .unwrap_err();
        assert!(mismatched.contains("did not match"));
    }
    #[test]
    fn validates_initialize_result_shape_and_protocol() {
        let valid = json!({
            "protocolVersion": PROTOCOL_VERSION,
            "serverInfo": {"name":"grep.app", "version":"1"},
            "capabilities": {}
        });
        assert!(validate_initialize_result(&valid).is_ok());
        for invalid in [
            json!({"protocolVersion":"2024-11-05", "serverInfo":{"name":"x", "version":"1"}, "capabilities":{}}),
            json!({"protocolVersion":PROTOCOL_VERSION, "serverInfo":"x", "capabilities":{}}),
            json!({"protocolVersion":PROTOCOL_VERSION, "serverInfo":{"name":"", "version":"1"}, "capabilities":{}}),
            json!({"protocolVersion":PROTOCOL_VERSION, "serverInfo":{"name":"x", "version":"1"}, "capabilities":[]}),
        ] {
            assert!(validate_initialize_result(&invalid).is_err());
        }
    }
    #[test]
    fn rejects_non_boolean_tool_error_flag() {
        assert_eq!(
            tool_texts(&json!({"isError": "false", "content": []})).unwrap_err(),
            "tool result isError must be a boolean"
        );
    }
}
