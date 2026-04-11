use anyhow::{Context, Result};
use clap::Args;

use crate::style;

#[derive(Debug, Args)]
pub struct InitArgs {
    /// Project name (defaults to current directory name).
    #[arg(long)]
    pub name: Option<String>,

    /// GPU type for the default function.
    #[arg(long, default_value = "t4")]
    pub gpu: String,

    /// Python entry point (e.g., main:handler).
    #[arg(long, default_value = "main:handler")]
    pub entry_point: String,
}

pub async fn run(args: InitArgs) -> Result<()> {
    let cwd = std::env::current_dir().context("Cannot determine current directory")?;

    // Check if already initialized
    if cwd.join("zylora.toml").exists() {
        anyhow::bail!("zylora.toml already exists in this directory. Remove it first to re-init.");
    }

    let project_name = args.name.unwrap_or_else(|| {
        cwd.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("my-project")
            .to_string()
    });

    // Detect Python environment
    let python_version = detect_python_version();
    let runtime = python_version
        .as_ref()
        .map(|v| format!("python{v}"))
        .unwrap_or_else(|| "python312".into());

    // Generate zylora.toml
    let toml_content = format!(
        r#"[project]
name = "{project_name}"

[functions.{fn_name}]
entry_point = "{entry_point}"
gpu_type = "{gpu}"
runtime = "{runtime}"
timeout_seconds = 300
min_instances = 0
max_instances = 10
"#,
        fn_name = sanitize_name(&project_name),
        entry_point = args.entry_point,
        gpu = args.gpu,
    );

    std::fs::write(cwd.join("zylora.toml"), &toml_content)
        .context("Failed to write zylora.toml")?;

    // Generate .zyloraignore
    let ignore_content = r#"# Zylora ignore file (like .dockerignore)
.git/
.venv/
__pycache__/
*.pyc
.env
.mypy_cache/
.ruff_cache/
node_modules/
*.egg-info/
dist/
build/
.pytest_cache/
"#;

    if !cwd.join(".zyloraignore").exists() {
        std::fs::write(cwd.join(".zyloraignore"), ignore_content)
            .context("Failed to write .zyloraignore")?;
    }

    println!("{}", style::success(&format!("Project initialized: {project_name}")));
    println!();
    println!("Created:");
    println!("  zylora.toml     — project configuration");
    println!("  .zyloraignore   — files to exclude from deploy");
    println!();
    println!("Next steps:");
    println!("  1. Edit your function code");
    println!("  2. Run `zy deploy` to deploy");

    if let Some(version) = python_version {
        println!(
            "{}",
            style::dim(&format!(
                "  Detected Python {}",
                version
            ))
        );
    }

    Ok(())
}

/// Try to detect the Python version in the current environment.
fn detect_python_version() -> Option<String> {
    let output = std::process::Command::new("python3")
        .args(["--version"])
        .output()
        .or_else(|_| {
            std::process::Command::new("python")
                .args(["--version"])
                .output()
        })
        .ok()?;

    let version_str = String::from_utf8_lossy(&output.stdout);
    // "Python 3.12.4" → "3.12"
    let version = version_str.trim().strip_prefix("Python ")?;
    let parts: Vec<&str> = version.split('.').collect();
    if parts.len() >= 2 {
        Some(format!("{}.{}", parts[0], parts[1]))
    } else {
        None
    }
}

/// Sanitize a project name to a valid function name (lowercase, hyphens).
fn sanitize_name(name: &str) -> String {
    name.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' { c } else { '-' })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_project_name() {
        assert_eq!(sanitize_name("My Project"), "my-project");
        assert_eq!(sanitize_name("ml_pipeline_v2"), "ml-pipeline-v2");
        assert_eq!(sanitize_name("--test--"), "test");
    }
}
