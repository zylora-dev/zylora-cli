use anyhow::Result;
use clap::Subcommand;
use serde::{Deserialize, Serialize};

use crate::client::ApiClient;
use crate::config;
use crate::output::OutputFormat;
use crate::style;

#[derive(Debug, Subcommand)]
pub enum FunctionsCommand {
    /// List all functions.
    List,

    /// Show function details.
    Info {
        /// Function name or ID.
        name: String,
    },

    /// Delete a function (soft delete).
    Delete {
        /// Function name or ID.
        name: String,
    },
}

#[derive(Debug, Serialize, Deserialize, tabled::Tabled)]
pub struct FunctionRow {
    pub name: String,
    pub status: String,
    pub gpu_type: String,
    pub version: String,
    pub endpoint: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FunctionDetail {
    pub id: String,
    pub name: String,
    pub status: String,
    pub gpu_type: String,
    pub runtime: String,
    pub current_version: Option<u32>,
    pub endpoint: Option<String>,
    pub min_instances: u32,
    pub max_instances: u32,
    pub timeout_seconds: u32,
    pub invocations_24h: Option<u64>,
    pub avg_latency_ms: Option<f64>,
    pub created_at: String,
}

impl std::fmt::Display for FunctionDetail {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Name:       {}", self.name)?;
        writeln!(f, "ID:         {}", self.id)?;
        writeln!(f, "Status:     {}", self.status)?;
        writeln!(f, "GPU:        {}", self.gpu_type)?;
        writeln!(f, "Runtime:    {}", self.runtime)?;
        if let Some(v) = self.current_version {
            writeln!(f, "Version:    v{v}")?;
        }
        if let Some(ref ep) = self.endpoint {
            writeln!(f, "Endpoint:   {ep}")?;
        }
        writeln!(
            f,
            "Scaling:    {} → {} instances",
            self.min_instances, self.max_instances
        )?;
        writeln!(f, "Timeout:    {}s", self.timeout_seconds)?;
        if let Some(n) = self.invocations_24h {
            writeln!(f, "24h calls:  {n}")?;
        }
        if let Some(lat) = self.avg_latency_ms {
            writeln!(f, "Avg latency: {lat:.0}ms")?;
        }
        writeln!(f, "Created:    {}", self.created_at)?;
        Ok(())
    }
}

pub async fn run(cmd: FunctionsCommand, format: OutputFormat, yes: bool) -> Result<()> {
    let token = config::require_token()?;
    let client = ApiClient::new(token)?;

    match cmd {
        FunctionsCommand::List => {
            let functions: Vec<FunctionRow> = client.get("/v1/functions").await?;
            crate::output::print_list(&functions, format)?;
        }

        FunctionsCommand::Info { name } => {
            let detail: FunctionDetail = client.get(&format!("/v1/functions/{name}")).await?;
            crate::output::print_item(&detail, format)?;
        }

        FunctionsCommand::Delete { name } => {
            if !yes {
                let confirm = dialoguer::Confirm::new()
                    .with_prompt(format!("Delete function '{name}'? This cannot be undone."))
                    .default(false)
                    .interact()
                    .unwrap_or(false);

                if !confirm {
                    println!("Cancelled.");
                    return Ok(());
                }
            }

            client.delete(&format!("/v1/functions/{name}")).await?;
            println!("{}", style::success(&format!("Function '{name}' deleted.")));
        }
    }

    Ok(())
}
