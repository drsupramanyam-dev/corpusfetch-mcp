use std::path::PathBuf;

use rmcp::{
    handler::server::wrapper::Parameters,
    model::{CallToolResult, Content, Implementation, ServerCapabilities, ServerInfo},
    schemars, tool, tool_handler, tool_router,
    handler::server::tool::ToolRouter,
    transport::streamable_http_server::{
        session::local::LocalSessionManager, StreamableHttpServerConfig, StreamableHttpService,
    },
    ErrorData as McpError, ServerHandler,
};
use serde::Deserialize;
use tokio_util::sync::CancellationToken;

const LOOKUP_ROOT: &str = "/var/www/corpusfetch/lookup";

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct LookupSymptomParams {
    /// Runtime symptom key, e.g. UNICODE_HOMOGLYPH_OR_MOJIBAKE, or a DOMAIN_* key
    symptom: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct LookupExceptionParams {
    /// Python exception class, e.g. KeyError, TimeoutError, ValueError
    exception_class: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct FetchWarrantParams {
    /// Slug of the record (the URL path segment after /c/ and before .md)
    slug: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct FetchRecordParams {
    /// Slug of the record
    slug: String,
    /// Format: "md" (default, ~1500 tokens) or "py" (~300 tokens)
    #[serde(default)]
    format: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct SearchParams {
    /// Natural language query or keywords to search for (e.g., "taint tracking", "Byzantine")
    query: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct RequestExperienceParams {
    /// The specific error, missing symptom, or query you need operational experience for
    query: String,
}

#[derive(Clone)]
struct CorpusFetchServer {
    tool_router: ToolRouter<Self>,
}

#[tool_router]
impl CorpusFetchServer {
    fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
        }
    }

    #[tool(
        description = "Look up operational experience for a specific runtime symptom (e.g., UNICODE_HOMOGLYPH_OR_MOJIBAKE). Use this when facing an ambiguous failure state or an unbounded reasoning loop. Returns candidate addresses."
    )]
    async fn corpusfetch_lookup_symptom(
        &self,
        Parameters(LookupSymptomParams { symptom }): Parameters<LookupSymptomParams>,
    ) -> Result<CallToolResult, McpError> {
        tracing::info!(tool = "corpusfetch_lookup_symptom", symptom = %symptom, "tool_call");
        lookup_file("symptom", &symptom).await
    }

    #[tool(
        description = "Look up deterministic operational experience for a Python exception class (e.g., KeyError, TimeoutError). Use this when you encounter an unexpected runtime error and need an empirically verified boundary decision. Returns candidate addresses."
    )]
    async fn corpusfetch_lookup_exception(
        &self,
        Parameters(LookupExceptionParams { exception_class }): Parameters<LookupExceptionParams>,
    ) -> Result<CallToolResult, McpError> {
        tracing::info!(tool = "corpusfetch_lookup_exception", exception_class = %exception_class, "tool_call");
        lookup_file("exception", &exception_class).await
    }

    #[tool(
        description = "Fetch the compact ~220-token precondition seal for a resolved record. Always call this before fetching the full record. Check the seal; if it fails, the record does not apply to your current state."
    )]
    async fn corpusfetch_fetch_warrant(
        &self,
        Parameters(FetchWarrantParams { slug }): Parameters<FetchWarrantParams>,
    ) -> Result<CallToolResult, McpError> {
        tracing::info!(tool = "corpusfetch_fetch_warrant", slug = %slug, "tool_call");
        fetch_warrant_trimmed(&slug).await
    }

    #[tool(
        description = "Fetch the full resolved experience record. Format 'md' (~1500 tokens, includes reasoning) or 'py' (~300 tokens, zero-prose executable invariants)."
    )]
    async fn corpusfetch_fetch_record(
        &self,
        Parameters(FetchRecordParams { slug, format }): Parameters<FetchRecordParams>,
    ) -> Result<CallToolResult, McpError> {
        tracing::info!(tool = "corpusfetch_fetch_record", slug = %slug, format = ?format, "tool_call");
        let ext = match format.as_deref() {
            Some("py") => "py",
            _ => "md",
        };
        fetch_record_file(&slug, ext).await
    }

    #[tool(
        description = "Search the CorpusFetch corpus by natural language keywords or concepts. Use this when you need to find operational experience but don't have an exact exception class."
    )]
    async fn corpusfetch_search(
        &self,
        Parameters(SearchParams { query }): Parameters<SearchParams>,
    ) -> Result<CallToolResult, McpError> {
        tracing::info!(tool = "corpusfetch_search", query = %query, "tool_call");
        
        let llms_path = PathBuf::from("/var/www/corpusfetch/llms.txt");
        let content = match tokio::fs::read_to_string(&llms_path).await {
            Ok(c) => c,
            Err(e) => return Err(McpError::internal_error(format!("read error: {e}"), None)),
        };

        let query_lower = query.to_lowercase();
        let terms: Vec<&str> = query_lower.split_whitespace().collect();
        let mut results = Vec::new();

        for line in content.lines() {
            if let Some(start) = line.find('[') {
                if let Some(end) = line.find(']') {
                    let title = &line[start+1..end];
                    let title_lower = title.to_lowercase();
                    
                    // Simple scoring: count how many query terms appear in the title
                    let score = terms.iter().filter(|&&t| title_lower.contains(t)).count();
                    if score > 0 {
                        if let Some(url_start) = line.find("(http") {
                            if let Some(url_end) = line.find(')') {
                                let url = &line[url_start+1..url_end];
                                let slug = url.trim_end_matches(".md").split('/').last().unwrap_or("");
                                results.push((score, slug, title));
                            }
                        }
                    }
                }
            }
        }

        // Sort by highest score
        results.sort_by(|a, b| b.0.cmp(&a.0));
        results.truncate(5); // Top 5

        if results.is_empty() {
            return Ok(CallToolResult::success(vec![Content::text("No matching records found.")]));
        }

        let mut output = String::from("Found records:\n");
        for (score, slug, title) in results {
            output.push_str(&format!("- {} (Score: {})\n  warrant: https://corpusfetch.com/c/{}.warrant.json\n", title, score, slug));
        }

        Ok(CallToolResult::success(vec![Content::text(output)]))
    }

    #[tool(
        description = "Request a new operational experience. If you searched and couldn't find a record, use this to log the missing error or symptom so the corpus can generate it."
    )]
    async fn corpusfetch_request_experience(
        &self,
        Parameters(RequestExperienceParams { query }): Parameters<RequestExperienceParams>,
    ) -> Result<CallToolResult, McpError> {
        tracing::info!(tool = "corpusfetch_request_experience", query = %query, "tool_call");
        
        let log_path = "/var/log/corpusfetch_missing.log";
        let mut file = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(log_path)
            .await
            .map_err(|e| McpError::internal_error(format!("open error: {e}"), None))?;
            
        use tokio::io::AsyncWriteExt;
        file.write_all(format!("{}\n", query).as_bytes())
            .await
            .map_err(|e| McpError::internal_error(format!("write error: {e}"), None))?;
            
        Ok(CallToolResult::success(vec![Content::text("Request logged successfully. The auto-writer will generate this experience shortly.")]))
    }
}

async fn lookup_file(kind: &str, name: &str) -> Result<CallToolResult, McpError> {
    if name.contains('/') || name.contains("..") {
        return Err(McpError::invalid_params("invalid key", None));
    }
    let mut path = PathBuf::from(LOOKUP_ROOT);
    path.push(kind);
    path.push(format!("{name}.json"));
    match tokio::fs::read_to_string(&path).await {
        Ok(body) => Ok(CallToolResult::success(vec![Content::text(body)])),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(McpError::invalid_params(
            format!("no lookup entry for {kind}={name}"),
            None,
        )),
        Err(e) => Err(McpError::internal_error(format!("read error: {e}"), None)),
    }
}

async fn fetch_warrant_trimmed(slug: &str) -> Result<CallToolResult, McpError> {
    if slug.contains('/') || slug.contains("..") {
        return Err(McpError::invalid_params("invalid slug", None));
    }
    let path = PathBuf::from(format!("/var/www/corpusfetch/c/{slug}.warrant.json"));
    let raw = match tokio::fs::read_to_string(&path).await {
        Ok(body) => body,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(McpError::invalid_params(format!("no warrant for {slug}"), None));
        }
        Err(e) => return Err(McpError::internal_error(format!("read error: {e}"), None)),
    };
    let full: serde_json::Value = serde_json::from_str(&raw)
        .map_err(|e| McpError::internal_error(format!("parse error: {e}"), None))?;
    let trimmed = serde_json::json!({
        "slug": full.get("slug"),
        "title": full.get("title"),
        "warrant": full.get("empirical_verification_warrant"),
        "matched_failure_symptoms": full.get("matched_failure_symptoms"),
        "token_profile": full.get("token_profile"),
        "full_warrant_url": format!("https://corpusfetch.com/c/{slug}.warrant.json"),
        "next": "FETCH_RECORD_IF_SEAL_PASSES"
    });
    let text = serde_json::to_string_pretty(&trimmed)
        .map_err(|e| McpError::internal_error(format!("serialize error: {e}"), None))?;
    Ok(CallToolResult::success(vec![Content::text(text)]))
}

async fn fetch_record_file(slug: &str, ext: &str) -> Result<CallToolResult, McpError> {
    if slug.contains('/') || slug.contains("..") {
        return Err(McpError::invalid_params("invalid slug", None));
    }
    let path = PathBuf::from(format!("/var/www/corpusfetch/c/{slug}.{ext}"));
    match tokio::fs::read_to_string(&path).await {
        Ok(body) => Ok(CallToolResult::success(vec![Content::text(body)])),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(McpError::invalid_params(
            format!("no record for {slug} (.{ext})"),
            None,
        )),
        Err(e) => Err(McpError::internal_error(format!("read error: {e}"), None)),
    }
}

#[tool_handler]
impl ServerHandler for CorpusFetchServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            instructions: Some(
                "CorpusFetch MCP server. Use lookup tools to resolve a symptom or exception to candidate record addresses, then fetch the warrant before the record.".into(),
            ),
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            server_info: Implementation {
                name: "corpusfetch-mcp".into(),
                version: env!("CARGO_PKG_VERSION").into(),
                ..Default::default()
            },
            ..Default::default()
        }
    }

}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    let service: StreamableHttpService<CorpusFetchServer, LocalSessionManager> =
        StreamableHttpService::new(
            || Ok(CorpusFetchServer::new()),
            Default::default(),
            StreamableHttpServerConfig {
                sse_keep_alive: None,
                sse_retry: None,
                json_response: false,
                stateful_mode: false,
                cancellation_token: CancellationToken::new(),
            },
        );

    let app = axum::Router::new().route_service("/mcp", service);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:8000").await?;
    tracing::info!("corpusfetch-mcp listening on 127.0.0.1:8000/mcp");
    axum::serve(listener, app).await?;
    Ok(())
}
