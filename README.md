# grep-cli

A small, safe blocking CLI for searching public GitHub code through the official [`https://mcp.grep.app`](https://mcp.grep.app) MCP service. It deliberately has no terminal UI, syntax highlighter, async runtime, or MCP framework.

## Install

Install the latest native release:

```sh
curl -fsSL https://raw.githubusercontent.com/fschrhunt/grep-cli/main/scripts/install.sh | sh
```

The installer downloads the release archive and its `SHA256SUMS` file, checks the archive integrity against that published checksum before extracting the expected binary, and installs `grep-cli` to `~/.local/bin`. Set `GREP_CLI_VERSION` to install a specific version or `GREP_CLI_BIN_DIR` to choose another binary directory. Native releases support macOS and Linux on x86_64/amd64 and arm64/aarch64; Linux release binaries require glibc.

Checksums help detect corruption or tampering of the downloaded archive, but they do not authenticate the release by themselves. For musl-based Linux systems, Cargo remains the fallback:

```sh
cargo install --locked --git https://github.com/fschrhunt/grep-cli
```

From a source checkout:

```sh
cargo build --locked --release
```

## Usage

```text
grep-cli [OPTIONS] QUERY

      --match-case
      --match-whole-words
      --use-regexp
      --repo VALUE
      --path VALUE
      --language VALUE       repeatable
      --json
      --color auto|always|never
      --timeout SECONDS      1-300; default: 15
```

Examples:

```sh
grep-cli 'useState(' --language TypeScript --language TSX
grep-cli --repo rust-lang/rust --path compiler --match-case 'struct Foo'
grep-cli --json --use-regexp '(?s)try {.*await'
grep-cli -- -a-leading-query
```

Use `--` before a query that begins with `-`. `--help` and `--version` are available.

## Development

Run the offline installer tests with:

```sh
sh tests/install.sh
```

## Security and operational behavior

* The endpoint is fixed to HTTPS `mcp.grep.app`; this program does not accept arbitrary URLs.
* It uses blocking `ureq`, a 15-second default timeout per HTTP request (capped at 300 seconds), no redirects, an explicit User-Agent, and a 2 MiB response-body cap. There are no broad automatic retries. Redirects are disabled so the fixed endpoint, protocol header, and session ID cannot cross origins.
* It validates non-empty query/filter values, query length (4096 bytes), filter length (1024 bytes), and language count (32). Unexpected, oversized, or malformed MCP/tool/search structures fail rather than being guessed at.
* MCP is initialized using protocol version `2025-06-18`, sent on every request as `MCP-Protocol-Version`, followed by `notifications/initialized` and `tools/call` for `searchGitHub`. The initialize result protocol and object shapes, JSON-RPC IDs/errors, and `CallToolResult.isError` are checked; an optional `Mcp-Session-Id` is retained.
* Server strings are escaped for C0/C1/escape controls and Unicode bidirectional controls before human or JSON output. `--color auto` enables ANSI only for a stdout TTY when `NO_COLOR` is absent; no source highlighting is performed.

## Architecture and tradeoffs

The binary has five direct pieces: strict argument parsing, a tiny MCP JSON-RPC/SSE transport, a stateful parser for the service's formatted text, terminal sanitization, and buffered plain rendering. Keeping these separate makes untrusted text handling testable while avoiding `rmcp`, `tokio`, `syntect`, `crossterm`, and `terminal-light`.

The server currently returns formatted text rather than structured matches, so parsing is intentionally conservative. A new result is recognized only by a complete repository/path/URL/license/snippets header sequence, and snippet ordinals and line numbers must be positive integers. The text protocol remains inherently ambiguous: a complete six-line fake result header inside source code is indistinguishable from a real next result header. JSON mode emits the resulting typed `SearchResult` and `Snippet` objects, not the raw MCP envelope. A live no-match response becomes `[]` in JSON mode and `No results found.` in human mode.

## License

MIT. See [LICENSE](LICENSE).
