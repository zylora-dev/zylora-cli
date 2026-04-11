use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::error::CliError;

/// Global CLI configuration stored at `~/.zylora/config.toml`.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct CliConfig {
    #[serde(default)]
    pub auth: AuthConfig,

    #[serde(default)]
    pub defaults: DefaultsConfig,

    #[serde(default)]
    pub preferences: PreferencesConfig,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct AuthConfig {
    /// API token stored after `zy login`.
    pub token: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct DefaultsConfig {
    /// Default organization slug.
    pub org: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PreferencesConfig {
    /// Output format: table, json, yaml.
    #[serde(default = "default_output")]
    pub output: String,

    /// Enable colored output.
    #[serde(default = "default_true")]
    pub color: bool,
}

impl Default for PreferencesConfig {
    fn default() -> Self {
        Self {
            output: default_output(),
            color: default_true(),
        }
    }
}

fn default_output() -> String {
    "table".to_string()
}

fn default_true() -> bool {
    true
}

/// Per-project configuration from `zylora.toml`.
#[derive(Debug, Serialize, Deserialize)]
pub struct ProjectConfig {
    pub project: ProjectMeta,

    #[serde(default)]
    pub defaults: Option<FunctionDefaults>,

    /// Named function configurations.
    #[serde(default)]
    pub functions: std::collections::HashMap<String, FunctionConfig>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ProjectMeta {
    pub name: String,
    pub org: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct FunctionDefaults {
    pub gpu_type: Option<String>,
    pub runtime: Option<String>,
    pub timeout_seconds: Option<u32>,
    pub min_instances: Option<u32>,
    pub max_instances: Option<u32>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FunctionConfig {
    pub entry_point: String,
    pub gpu_type: String,

    #[serde(default)]
    pub gpu_count: Option<u32>,
    #[serde(default)]
    pub runtime: Option<String>,
    #[serde(default)]
    pub python_version: Option<String>,
    #[serde(default)]
    pub timeout_seconds: Option<u32>,
    #[serde(default)]
    pub min_instances: Option<u32>,
    #[serde(default)]
    pub max_instances: Option<u32>,
    #[serde(default)]
    pub concurrency: Option<u32>,
    #[serde(default)]
    pub env: std::collections::HashMap<String, String>,
    #[serde(default)]
    pub secrets: Vec<String>,
    #[serde(default)]
    pub requirements_file: Option<String>,
}

// ── Path helpers ─────────────────────────────────────────────────────────────

/// `~/.zylora/`
pub fn config_dir() -> Result<PathBuf> {
    let home = dirs::home_dir().context("Could not determine home directory")?;
    Ok(home.join(".zylora"))
}

/// `~/.zylora/config.toml`
pub fn config_path() -> Result<PathBuf> {
    Ok(config_dir()?.join("config.toml"))
}

// ── Load / Save ──────────────────────────────────────────────────────────────

/// Load CLI config. Returns default if file doesn't exist.
pub fn load_config() -> Result<CliConfig> {
    let path = config_path()?;
    if !path.exists() {
        return Ok(CliConfig::default());
    }
    let contents = std::fs::read_to_string(&path)
        .with_context(|| format!("Failed to read {}", path.display()))?;
    let config: CliConfig =
        toml::from_str(&contents).with_context(|| format!("Invalid TOML in {}", path.display()))?;
    Ok(config)
}

/// Save CLI config to `~/.zylora/config.toml`.
pub fn save_config(config: &CliConfig) -> Result<()> {
    let dir = config_dir()?;
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("Failed to create {}", dir.display()))?;

    let path = config_path()?;
    let contents = toml::to_string_pretty(config).context("Failed to serialize config")?;
    std::fs::write(&path, contents)
        .with_context(|| format!("Failed to write {}", path.display()))?;
    Ok(())
}

/// Get the stored auth token, or error if not logged in.
pub fn require_token() -> Result<String> {
    // Check env var first (for CI/CD)
    if let Ok(token) = std::env::var("ZYLORA_TOKEN") {
        if !token.is_empty() {
            return Ok(token);
        }
    }

    let config = load_config()?;
    config
        .auth
        .token
        .filter(|t| !t.is_empty())
        .ok_or_else(|| CliError::NotAuthenticated.into())
}

/// Load `zylora.toml` from current directory (or parent directories).
pub fn load_project_config() -> Result<ProjectConfig> {
    let path = find_project_file()?;
    let contents = std::fs::read_to_string(&path)
        .with_context(|| format!("Failed to read {}", path.display()))?;
    let config: ProjectConfig = toml::from_str(&contents)
        .with_context(|| format!("Invalid zylora.toml at {}", path.display()))?;
    Ok(config)
}

/// Walk up directories to find `zylora.toml`.
fn find_project_file() -> Result<PathBuf> {
    let mut dir = std::env::current_dir().context("Cannot determine current directory")?;
    loop {
        let candidate = dir.join("zylora.toml");
        if candidate.exists() {
            return Ok(candidate);
        }
        if !dir.pop() {
            break;
        }
    }
    Err(CliError::ProjectNotInitialized.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_has_table_output() {
        let cfg = CliConfig::default();
        assert_eq!(cfg.preferences.output, "table");
        assert!(cfg.preferences.color);
    }

    #[test]
    fn parse_minimal_project_config() {
        let toml_str = r#"
[project]
name = "my-project"

[functions.predict]
entry_point = "main:handler"
gpu_type = "h100"
"#;
        let cfg: ProjectConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.project.name, "my-project");
        assert!(cfg.functions.contains_key("predict"));
        assert_eq!(cfg.functions["predict"].gpu_type, "h100");
    }

    #[test]
    fn parse_full_project_config() {
        let toml_str = r#"
[project]
name = "ml-pipeline"
org = "acme"

[defaults]
gpu_type = "a100_80gb"
timeout_seconds = 300

[functions.embed]
entry_point = "embed:handler"
gpu_type = "t4"
min_instances = 1
max_instances = 10
secrets = ["HF_TOKEN"]

[functions.generate]
entry_point = "gen:predict"
gpu_type = "h100"
timeout_seconds = 600
"#;
        let cfg: ProjectConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.project.org, Some("acme".to_string()));
        assert_eq!(cfg.functions.len(), 2);
        assert_eq!(cfg.functions["embed"].secrets, vec!["HF_TOKEN"]);
    }
}
