use anyhow::{Context, Result};
use clap::Args;
use futures_util::StreamExt;
use serde::Deserialize;

use crate::client::ApiClient;
use crate::config;
use crate::output::OutputFormat;
use crate::style;

#[derive(Debug, Args)]
pub struct DeployArgs {
    /// Function name (defaults to first function in zylora.toml).
    #[arg(long)]
    pub function: Option<String>,

    /// Override GPU type.
    #[arg(long)]
    pub gpu: Option<String>,

    /// Override timeout in seconds.
    #[arg(long)]
    pub timeout: Option<u32>,

    /// Override minimum instances.
    #[arg(long)]
    pub min_instances: Option<u32>,
}

#[derive(Debug, Deserialize, serde::Serialize)]
struct DeployResponse {
    deployment_id: String,
    function_id: String,
    version: u32,
    status: String,
    endpoint: Option<String>,
}

impl std::fmt::Display for DeployResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Deployment: {}", self.deployment_id)?;
        writeln!(f, "Function:   {}", self.function_id)?;
        writeln!(f, "Version:    v{}", self.version)?;
        writeln!(f, "Status:     {}", self.status)?;
        if let Some(ref ep) = self.endpoint {
            writeln!(f, "Endpoint:   {ep}")?;
        }
        Ok(())
    }
}

pub async fn run(args: DeployArgs, format: OutputFormat) -> Result<()> {
    let token = config::require_token()?;
    let project = config::load_project_config()?;
    let client = ApiClient::new(token)?;

    // Resolve function name
    let (fn_name, fn_config) = resolve_function(&project, args.function.as_deref())?;

    let gpu_type = args.gpu.as_deref().unwrap_or(&fn_config.gpu_type);
    let timeout = args.timeout.unwrap_or(
        fn_config.timeout_seconds.unwrap_or(300),
    );

    let spinner = style::spinner();
    spinner.set_message(format!("Bundling {fn_name}..."));

    // Bundle the project source
    let bundle = bundle_project()?;
    spinner.set_message("Uploading...");

    // Upload as multipart form
    let form = reqwest::multipart::Form::new()
        .text("name", fn_name.clone())
        .text("entry_point", fn_config.entry_point.clone())
        .text("gpu_type", gpu_type.to_string())
        .text("timeout_seconds", timeout.to_string())
        .text(
            "min_instances",
            args.min_instances
                .or(fn_config.min_instances)
                .unwrap_or(0)
                .to_string(),
        )
        .part(
            "bundle",
            reqwest::multipart::Part::bytes(bundle)
                .file_name("bundle.tar.gz")
                .mime_str("application/gzip")?,
        );

    let deploy_resp: serde_json::Value = client
        .upload(&format!("/v1/functions/{fn_name}/deploy"), form)
        .await?;

    let deployment_id = deploy_resp["deployment_id"]
        .as_str()
        .unwrap_or("unknown");

    spinner.finish_and_clear();
    println!("{}", style::dim("Build logs:"));

    // Stream build logs via SSE
    stream_build_logs(&client, deployment_id).await?;

    // Fetch final deployment state
    let result: DeployResponse = client
        .get(&format!("/v1/deployments/{deployment_id}"))
        .await?;

    match format {
        OutputFormat::Table => {
            if result.status == "active" {
                println!(
                    "{}",
                    style::success(&format!(
                        "Deployed {fn_name} v{} → {}",
                        result.version,
                        result.endpoint.as_deref().unwrap_or("(pending)")
                    ))
                );
            } else {
                println!(
                    "{}",
                    style::warning(&format!(
                        "Deployment {}: {}",
                        deployment_id, result.status
                    ))
                );
            }
        }
        _ => crate::output::print_item(&result, format)?,
    }

    Ok(())
}

/// Resolve which function to deploy from the project config.
fn resolve_function<'a>(
    project: &'a config::ProjectConfig,
    name: Option<&str>,
) -> Result<(String, &'a config::FunctionConfig)> {
    if let Some(name) = name {
        let config = project
            .functions
            .get(name)
            .with_context(|| format!("Function '{name}' not found in zylora.toml"))?;
        return Ok((name.to_string(), config));
    }

    // Default to the first (or only) function
    let (name, config) = project
        .functions
        .iter()
        .next()
        .context("No functions defined in zylora.toml")?;

    if project.functions.len() > 1 {
        println!(
            "{}",
            style::warning(&format!(
                "Multiple functions found. Deploying '{name}'. Use --function to specify."
            ))
        );
    }

    Ok((name.clone(), config))
}

/// Bundle the current project directory into a tar.gz byte vector.
fn bundle_project() -> Result<Vec<u8>> {
    use flate2::write::GzEncoder;
    use flate2::Compression;

    let cwd = std::env::current_dir()?;
    let ignore_patterns = load_ignore_patterns(&cwd);

    let mut buf = Vec::new();
    {
        let encoder = GzEncoder::new(&mut buf, Compression::fast());
        let mut archive = tar::Builder::new(encoder);

        add_files_to_tar(&cwd, &cwd, &ignore_patterns, &mut archive)?;

        let encoder = archive.into_inner()?;
        encoder.finish()?;
    }

    Ok(buf)
}

/// Recursively add files to the tar archive, respecting ignore patterns.
fn add_files_to_tar(
    base: &std::path::Path,
    dir: &std::path::Path,
    ignores: &[String],
    archive: &mut tar::Builder<flate2::write::GzEncoder<&mut Vec<u8>>>,
) -> Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let relative = path
            .strip_prefix(base)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");

        if should_ignore(&relative, ignores) {
            continue;
        }

        if path.is_dir() {
            add_files_to_tar(base, &path, ignores, archive)?;
        } else {
            archive
                .append_path_with_name(&path, &relative)
                .with_context(|| format!("Failed to add {relative} to archive"))?;
        }
    }
    Ok(())
}

fn load_ignore_patterns(dir: &std::path::Path) -> Vec<String> {
    let ignore_file = dir.join(".zyloraignore");
    if let Ok(content) = std::fs::read_to_string(ignore_file) {
        content
            .lines()
            .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
            .map(|l| l.trim().trim_end_matches('/').to_string())
            .collect()
    } else {
        // Default ignores
        vec![
            ".git".into(),
            "__pycache__".into(),
            ".venv".into(),
            "node_modules".into(),
        ]
    }
}

fn should_ignore(path: &str, patterns: &[String]) -> bool {
    for pattern in patterns {
        // Simple prefix/contains matching
        if path.starts_with(pattern) || path.contains(&format!("/{pattern}")) {
            return true;
        }
        // Glob-like extension matching (e.g., *.pyc)
        if let Some(ext) = pattern.strip_prefix("*.") {
            if path.ends_with(&format!(".{ext}")) {
                return true;
            }
        }
    }
    false
}

/// Stream SSE build logs from the engine.
async fn stream_build_logs(client: &ApiClient, deployment_id: &str) -> Result<()> {
    let url = format!(
        "{}/v1/deployments/{deployment_id}/logs/stream",
        client.base_url()
    );

    let resp = reqwest::Client::new()
        .get(&url)
        .bearer_auth(client.token())
        .header("Accept", "text/event-stream")
        .send()
        .await;

    let resp = match resp {
        Ok(r) if r.status().is_success() => r,
        _ => {
            // SSE not available — fall back to polling
            println!("{}", style::dim("  (build log streaming not available — waiting for completion)"));
            return Ok(());
        }
    };

    let mut stream = resp.bytes_stream();
    let mut buffer = String::new();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.context("Stream error")?;
        buffer.push_str(&String::from_utf8_lossy(&chunk));

        // Process complete SSE events
        while let Some(pos) = buffer.find("\n\n") {
            let event = buffer[..pos].to_string();
            buffer = buffer[pos + 2..].to_string();

            for line in event.lines() {
                if let Some(data) = line.strip_prefix("data: ") {
                    if data == "[DONE]" {
                        return Ok(());
                    }
                    println!("  {data}");
                }
            }
        }
    }

    Ok(())
}
