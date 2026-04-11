use anyhow::{Context, Result};
use clap::Subcommand;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::client::ApiClient;
use crate::config;
use crate::output::OutputFormat;
use crate::style;

#[derive(Debug, Subcommand)]
pub enum ModelsCommand {
    /// Upload model weights to Zylora.
    Push {
        /// Path to model directory.
        path: String,

        /// Model name.
        #[arg(long)]
        name: String,
    },

    /// List models.
    List,

    /// List model versions.
    Versions {
        /// Model name.
        name: String,
    },

    /// Set traffic percentage for a model version.
    Promote {
        /// Model name:version (e.g., my-model:v3).
        target: String,

        /// Traffic percentage (0-100).
        #[arg(long)]
        traffic: u8,
    },

    /// Download model weights locally.
    Download {
        /// Model name:version.
        target: String,

        /// Output directory.
        #[arg(long, short)]
        output: Option<String>,
    },
}

#[derive(Debug, Serialize, Deserialize, tabled::Tabled)]
pub struct ModelRow {
    pub name: String,
    pub latest_version: String,
    pub size: String,
    pub created_at: String,
}

#[derive(Debug, Serialize, Deserialize, tabled::Tabled)]
pub struct ModelVersionRow {
    pub version: String,
    pub size: String,
    pub files: String,
    pub traffic: String,
    pub created_at: String,
}

/// Known model file extensions to include in push.
const MODEL_EXTENSIONS: &[&str] = &[
    ".safetensors",
    ".gguf",
    ".bin",
    ".onnx",
    ".pt",
    ".pth",
    ".h5",
    ".pb",
];

/// Config/metadata files to always include.
const META_FILES: &[&str] = &[
    "config.json",
    "tokenizer.json",
    "tokenizer_config.json",
    "special_tokens_map.json",
    "generation_config.json",
    "preprocessor_config.json",
    "vocab.json",
    "merges.txt",
    "vocab.txt",
];

pub async fn run(cmd: ModelsCommand, format: OutputFormat, yes: bool) -> Result<()> {
    let token = config::require_token()?;
    let client = ApiClient::new(token)?;

    match cmd {
        ModelsCommand::Push { path, name } => push(&client, &path, &name).await,
        ModelsCommand::List => list(&client, format).await,
        ModelsCommand::Versions { name } => versions(&client, &name, format).await,
        ModelsCommand::Promote { target, traffic } => {
            promote(&client, &target, traffic, yes).await
        }
        ModelsCommand::Download { target, output } => {
            download(&client, &target, output.as_deref()).await
        }
    }
}

/// Push model weights to the registry.
async fn push(client: &ApiClient, path: &str, name: &str) -> Result<()> {
    let model_dir = std::path::Path::new(path);
    if !model_dir.is_dir() {
        anyhow::bail!("Not a directory: {path}");
    }

    // Scan for model files
    let files = scan_model_files(model_dir)?;
    if files.is_empty() {
        anyhow::bail!(
            "No model files found in {path}. Looking for: {}",
            MODEL_EXTENSIONS.join(", ")
        );
    }

    let total_size: u64 = files.iter().map(|f| f.size).sum();
    println!(
        "Found {} files ({:.1} GB)",
        files.len(),
        total_size as f64 / 1_073_741_824.0
    );

    // Compute SHA-256 for each file
    let spinner = style::spinner();
    spinner.set_message("Computing checksums...");

    let mut manifest: Vec<serde_json::Value> = Vec::new();
    for file in &files {
        let hash = sha256_file(&file.path)?;
        manifest.push(serde_json::json!({
            "path": file.relative_path,
            "size": file.size,
            "sha256": hash,
        }));
    }
    spinner.finish_and_clear();

    // Check which blobs already exist on server
    let check_resp: serde_json::Value = client
        .post(
            &format!("/v1/models/{name}/check-blobs"),
            &serde_json::json!({ "files": manifest }),
        )
        .await?;

    let missing: Vec<&str> = check_resp["missing"]
        .as_array()
        .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default();

    let upload_count = missing.len();
    let upload_size: u64 = files
        .iter()
        .filter(|f| missing.contains(&f.relative_path.as_str()))
        .map(|f| f.size)
        .sum();

    if upload_count == 0 {
        println!("{}", style::dim("All blobs already uploaded — creating version."));
    } else {
        println!(
            "Uploading {} new files ({:.1} GB)...",
            upload_count,
            upload_size as f64 / 1_073_741_824.0
        );

        let pb = style::progress_bar(upload_size);

        for file in &files {
            if !missing.contains(&file.relative_path.as_str()) {
                continue;
            }

            pb.set_message(file.relative_path.clone());
            let bytes = std::fs::read(&file.path)
                .with_context(|| format!("Failed to read {}", file.path.display()))?;

            let form = reqwest::multipart::Form::new().part(
                "file",
                reqwest::multipart::Part::bytes(bytes)
                    .file_name(file.relative_path.clone())
                    .mime_str("application/octet-stream")?,
            );

            client
                .upload(&format!("/v1/models/{name}/upload"), form)
                .await?;
            pb.inc(file.size);
        }

        pb.finish_and_clear();
    }

    // Create version
    let version_resp: serde_json::Value = client
        .post(
            &format!("/v1/models/{name}/versions"),
            &serde_json::json!({ "files": manifest }),
        )
        .await?;

    let version = version_resp["version"].as_str().unwrap_or("?");
    let total_files = files.len();
    println!(
        "{}",
        style::success(&format!(
            "Pushed {name} {version} ({total_files} files, {:.1} GB)",
            total_size as f64 / 1_073_741_824.0
        ))
    );

    Ok(())
}

/// List all models.
async fn list(client: &ApiClient, format: OutputFormat) -> Result<()> {
    let models: Vec<ModelRow> = client.get("/v1/models").await?;
    crate::output::print_list(&models, format)
}

/// List model versions.
async fn versions(client: &ApiClient, name: &str, format: OutputFormat) -> Result<()> {
    let versions: Vec<ModelVersionRow> = client.get(&format!("/v1/models/{name}/versions")).await?;
    crate::output::print_list(&versions, format)
}

/// Promote a model version to a traffic percentage.
async fn promote(client: &ApiClient, target: &str, traffic: u8, yes: bool) -> Result<()> {
    let (name, version) = parse_target(target)?;

    if !yes {
        let confirm = dialoguer::Confirm::new()
            .with_prompt(format!(
                "Set {name}:{version} to {traffic}% traffic?"
            ))
            .default(true)
            .interact()
            .unwrap_or(false);

        if !confirm {
            println!("Cancelled.");
            return Ok(());
        }
    }

    client
        .post_no_response(
            &format!("/v1/models/{name}/versions/{version}/promote"),
            &serde_json::json!({ "traffic_percentage": traffic }),
        )
        .await?;

    println!(
        "{}",
        style::success(&format!("{name}:{version} → {traffic}% traffic"))
    );
    Ok(())
}

/// Download model weights locally.
async fn download(client: &ApiClient, target: &str, output: Option<&str>) -> Result<()> {
    let (name, version) = parse_target(target)?;
    let out_dir = output.unwrap_or(".");
    std::fs::create_dir_all(out_dir)?;

    let info: serde_json::Value = client
        .get(&format!("/v1/models/{name}/versions/{version}"))
        .await?;

    let files = info["files"]
        .as_array()
        .context("No files in version info")?;

    let total_size: u64 = files.iter().filter_map(|f| f["size"].as_u64()).sum();
    let pb = style::progress_bar(total_size);

    for file in files {
        let path = file["path"].as_str().context("Missing file path")?;
        let download_url = file["download_url"]
            .as_str()
            .context("Missing download URL")?;

        pb.set_message(path.to_string());

        let resp = reqwest::get(download_url)
            .await
            .context("Download failed")?;
        let bytes = resp.bytes().await?;

        let dest = std::path::Path::new(out_dir).join(path);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&dest, &bytes)?;
        pb.inc(bytes.len() as u64);
    }

    pb.finish_and_clear();
    println!(
        "{}",
        style::success(&format!(
            "Downloaded {name}:{version} to {out_dir} ({} files)",
            files.len()
        ))
    );
    Ok(())
}

// ── Helpers ──────────────────────────────────────────────────────────────────

struct ModelFile {
    path: std::path::PathBuf,
    relative_path: String,
    size: u64,
}

fn scan_model_files(dir: &std::path::Path) -> Result<Vec<ModelFile>> {
    let mut files = Vec::new();
    scan_dir_recursive(dir, dir, &mut files)?;
    Ok(files)
}

fn scan_dir_recursive(
    base: &std::path::Path,
    dir: &std::path::Path,
    files: &mut Vec<ModelFile>,
) -> Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();

        if path.is_dir() {
            scan_dir_recursive(base, &path, files)?;
            continue;
        }

        let relative = path
            .strip_prefix(base)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");

        let file_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("");

        let is_model = MODEL_EXTENSIONS.iter().any(|ext| file_name.ends_with(ext));
        let is_meta = META_FILES.contains(&file_name);

        if is_model || is_meta {
            let size = path.metadata()?.len();
            files.push(ModelFile {
                path,
                relative_path: relative,
                size,
            });
        }
    }
    Ok(())
}

fn sha256_file(path: &std::path::Path) -> Result<String> {
    use std::io::Read;

    let mut file = std::fs::File::open(path)
        .with_context(|| format!("Cannot open {}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 8192];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }
    let hash = hasher.finalize();
    let hex: String = hash.iter().map(|b| format!("{b:02x}")).collect();
    Ok(hex)
}

/// Parse "model:version" target string.
fn parse_target(target: &str) -> Result<(&str, &str)> {
    let parts: Vec<&str> = target.splitn(2, ':').collect();
    if parts.len() != 2 {
        anyhow::bail!("Invalid target format. Expected name:version (e.g., my-model:v3)");
    }
    Ok((parts[0], parts[1]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_model_target() {
        let (name, ver) = parse_target("my-model:v3").unwrap();
        assert_eq!(name, "my-model");
        assert_eq!(ver, "v3");
    }

    #[test]
    fn parse_target_invalid() {
        assert!(parse_target("no-version").is_err());
    }
}
