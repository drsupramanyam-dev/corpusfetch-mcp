# CorpusFetch MCP Server

An MCP (Model Context Protocol) server for [CorpusFetch.com](https://corpusfetch.com), a machine-native repository of concluded operational experiences for AI agents.

CorpusFetch provides deterministic, empirically verified boundary decisions for distributed systems, AI infrastructure, and software engineering.

## Features

This server exposes five tools to MCP clients (like Claude Desktop, Cursor, or autonomous agents):

1. `corpusfetch_lookup_exception`: Route a Python exception class to candidate record addresses.
2. `corpusfetch_lookup_symptom`: Route a runtime symptom to candidate record addresses.
3. `corpusfetch_search`: Search the corpus by natural language keywords or concepts.
4. `corpusfetch_fetch_warrant`: Fetch the compact ~220-token precondition seal for a record.
5. `corpusfetch_fetch_record`: Fetch the full record body (`.md` for reasoning or `.py` for executable invariants).

## Usage

This is a remote MCP server accessible at:
`https://mcp.corpusfetch.com/mcp`

### Connecting via Claude Desktop / Cursor

Add the following to your MCP client configuration (e.g., `claude_desktop_config.json`):

```json
{
  "mcpServers": {
    "corpusfetch": {
      "transport": "streamable-http",
      "url": "https://mcp.corpusfetch.com/mcp"
    }
  }
}
```

## Architecture

The server is written in Rust using the `rmcp` SDK. It reads from a dynamically generated static file system containing over 700+ operational records, ensuring sub-millisecond lookup times and zero database overhead.

## License

MIT
