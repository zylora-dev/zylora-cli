use anyhow::{Context, Result};
use clap::Args;
use futures_util::StreamExt;
use serde::Deserialize;

use crate::client::ApiClient;
use crate::config;
use crate::output::OutputFormat;
use crate::style;

#[derive(Debug, Args)]
pub struct InvokeArgs {
    /// Function name or endpoint slug.
    pub function: String,

    /// JSON input (or pipe from stdin).
    #[arg(long)]
    pub input: Option<String>,

    /// Stream response tokens via SSE.
    #[arg(long)]
    pub stream: bool,

    /// Async invocation — returns job ID immediately.
    #[arg(long, name = "async")]
    pub is_async: bool,
}

#[derive(Debug, Args)]
pub struct LogsArgs {
    /// Function name.
    pub function: String,

    /// Show logs since duration (e.g., 1h, 30m, 1d).
    #[arg(long, default_value = "1h")]
    pub since: String,

    /// Filter by log level.
    #[arg(long)]
    pub level: Option<String>,

    /// Filter by deployment version.
    #[arg(long)]
    pub deployment: Option<String>,
}

#[derive(Debug, Deserialize, serde::Serialize)]
struct InvokeResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    invocation_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    status: Option<String>,
    output: serde_json::Value,
    duration_ms: Option<u64>,
    cost_cents: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    gpu_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cold_start: Option<bool>,
}

#[derive(Debug, Deserialize, serde::Serialize)]
struct AsyncInvokeResponse {
    job_id: String,
}

pub async fn run(args: InvokeArgs, format: OutputFormat) -> Result<()> {
    let token = config::require_token()?;
    let client = ApiClient::new(token)?;

    // Read input from --input flag or stdin
    let input = resolve_input(args.input)?;

    if args.is_async {
        return invoke_async(&client, &args.function, &input, format).await;
    }

    if args.stream {
        return invoke_stream(&client, &args.function, &input).await;
    }

    // Sync invocation
    let spinner = style::spinner();
    spinner.set_message(format!("Invoking {}...", args.function));

    let resp: InvokeResponse = client
        .post(
            &format!("/v1/functions/{}/invoke", args.function),
            &input,
        )
        .await?;

    spinner.finish_and_clear();

    match format {
        OutputFormat::Table => {
            println!("{}", serde_json::to_string_pretty(&resp.output)?);
            if let Some(ms) = resp.duration_ms {
                let cold = if resp.cold_start.unwrap_or(false) { " | cold start" } else { "" };
                println!(
                    "{}",
                    style::dim(&format!(
                        "Duration: {ms}ms | Cost: {}{}",
                        resp.cost_cents
                            .map(style::format_credits)
                            .unwrap_or_else(|| "—".into()),
                        cold,
                    ))
                );
            }
        }
        _ => {
            crate::output::print_json(&serde_json::to_value(&resp)?, format)?;
        }
    }

    Ok(())
}

/// Async invocation — fire and forget, return job ID.
async fn invoke_async(
    client: &ApiClient,
    function: &str,
    input: &serde_json::Value,
    format: OutputFormat,
) -> Result<()> {
    let resp: AsyncInvokeResponse = client
        .post(&format!("/v1/functions/{function}/invoke/async"), input)
        .await?;

    match format {
        OutputFormat::Table => {
            println!("{}", style::success(&format!("Job submitted: {}", resp.job_id)));
        }
        _ => {
            crate::output::print_json(&serde_json::to_value(&resp)?, format)?;
        }
    }

    Ok(())
}

/// Streaming invocation — print tokens as they arrive via SSE.
async fn invoke_stream(
    client: &ApiClient,
    function: &str,
    input: &serde_json::Value,
) -> Result<()> {
    let resp = client
        .post_raw(
            &format!("/v1/functions/{function}/invoke/stream"),
            input,
        )
        .await?;

    if !resp.status().is_success() {
        anyhow::bail!("Stream invocation failed: HTTP {}", resp.status());
    }

    let mut stream = resp.bytes_stream();
    let mut buffer = String::new();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.context("Stream read error")?;
        buffer.push_str(&String::from_utf8_lossy(&chunk));

        while let Some(pos) = buffer.find("\n\n") {
            let event = buffer[..pos].to_string();
            buffer = buffer[pos + 2..].to_string();

            for line in event.lines() {
                if let Some(data) = line.strip_prefix("data: ") {
                    if data == "[DONE]" {
                        println!();
                        return Ok(());
                    }
                    // Parse SSE data — may be JSON with a "token" field
                    if let Ok(val) = serde_json::from_str::<serde_json::Value>(data) {
                        if let Some(token) = val["token"].as_str() {
                            print!("{token}");
                        } else if let Some(text) = val["text"].as_str() {
                            print!("{text}");
                        } else {
                            print!("{data}");
                        }
                    } else {
                        print!("{data}");
                    }
                }
            }
        }
    }

    println!();
    Ok(())
}

/// Resolve input from --input flag or stdin.
fn resolve_input(input: Option<String>) -> Result<serde_json::Value> {
    if let Some(ref input_str) = input {
        return serde_json::from_str(input_str).context("Invalid JSON input");
    }

    // Check if stdin has data (not a terminal)
    if atty_is_not_terminal() {
        let mut buf = String::new();
        std::io::Read::read_to_string(&mut std::io::stdin(), &mut buf)?;
        if !buf.trim().is_empty() {
            return serde_json::from_str(&buf).context("Invalid JSON from stdin");
        }
    }

    // No input provided — send empty object
    Ok(serde_json::json!({}))
}

/// Check if stdin is piped (not a terminal).
fn atty_is_not_terminal() -> bool {
    !std::io::IsTerminal::is_terminal(&std::io::stdin())
}

// ── zy logs ──────────────────────────────────────────────────────────────────

pub async fn logs(args: LogsArgs) -> Result<()> {
    let token = config::require_token()?;
    let client = ApiClient::new(token)?;

    let url = format!(
        "{}/v1/functions/{}/logs/stream",
        client.base_url(),
        args.function
    );

    println!(
        "{}",
        style::dim(&format!(
            "Tailing logs for {} (since {})...",
            args.function, args.since
        ))
    );

    let mut query_parts = vec![format!("since={}", args.since)];
    if let Some(ref level) = args.level {
        query_parts.push(format!("level={level}"));
    }
    if let Some(ref dep) = args.deployment {
        query_parts.push(format!("deployment={dep}"));
    }
    let query_string = query_parts.join("&");
    let full_url = format!("{url}?{query_string}");

    let resp = reqwest::Client::new()
        .get(&full_url)
        .bearer_auth(client.token())
        .header("Accept", "text/event-stream")
        .send()
        .await
        .context("Failed to connect to log stream")?;

    if !resp.status().is_success() {
        anyhow::bail!("Log stream failed: HTTP {}", resp.status());
    }

    let mut stream = resp.bytes_stream();
    let mut buffer = String::new();

    while let Some(chunk_result) = stream.next().await {
        let chunk = chunk_result.context("Stream read error")?;
        buffer.push_str(&String::from_utf8_lossy(&chunk));

        while let Some(pos) = buffer.find("\n\n") {
            let event = buffer[..pos].to_string();
            buffer = buffer[pos + 2..].to_string();

            for line in event.lines() {
                if let Some(data) = line.strip_prefix("data: ") {
                    print_log_line(data);
                }
            }
        }
    }

    Ok(())
}

/// Format and print a log line from the server.
fn print_log_line(data: &str) {
    #[derive(Deserialize)]
    struct LogEntry {
        timestamp: Option<String>,
        level: Option<String>,
        message: String,
        duration_ms: Option<u64>,
        status: Option<String>,
    }

    if let Ok(entry) = serde_json::from_str::<LogEntry>(data) {
        let ts = entry.timestamp.as_deref().unwrap_or("");
        let level = entry.level.as_deref().unwrap_or("INFO");
        let level_styled = match level {
            "ERROR" => console::style(level).red().to_string(),
            "WARN" => console::style(level).yellow().to_string(),
            "DEBUG" => console::style(level).dim().to_string(),
            _ => console::style(level).cyan().to_string(),
        };

        let mut line = format!("{ts} [{level_styled}] {}", entry.message);
        if let Some(ms) = entry.duration_ms {
            line.push_str(&format!(" ({ms}ms)"));
        }
        if let Some(ref status) = entry.status {
            line.push_str(&format!(" [{status}]"));
        }
        println!("{line}");
    } else {
        // Raw log line
        println!("{data}");
    }
}
