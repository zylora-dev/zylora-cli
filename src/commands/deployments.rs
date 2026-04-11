use anyhow::Result;
use clap::Subcommand;
use serde::{Deserialize, Serialize};

use crate::client::ApiClient;
use crate::config;
use crate::output::OutputFormat;
use crate::style;

#[derive(Debug, Subcommand)]
pub enum DeploymentsCommand {
    /// List deployment history for a function.
    List {
        /// Function name.
        function: String,
    },

    /// View build logs for a specific deployment.
    Logs {
        /// Deployment ID.
        deployment_id: String,
    },

    /// Rollback a function to a previous deployment.
    Rollback {
        /// Function name.
        function: String,

        /// Target version number to rollback to.
        #[arg(long)]
        to: u32,
    },
}

#[derive(Debug, Serialize, Deserialize, tabled::Tabled)]
pub struct DeploymentRow {
    pub version: String,
    pub status: String,
    pub created_at: String,
    pub gpu_type: String,
    pub deployment_id: String,
}

pub async fn run(cmd: DeploymentsCommand, format: OutputFormat) -> Result<()> {
    let token = config::require_token()?;
    let client = ApiClient::new(token)?;

    match cmd {
        DeploymentsCommand::List { function } => {
            let deployments: Vec<DeploymentRow> = client
                .get(&format!("/v1/functions/{function}/deployments"))
                .await?;
            crate::output::print_list(&deployments, format)?;
        }

        DeploymentsCommand::Logs { deployment_id } => {
            let logs: serde_json::Value = client
                .get(&format!("/v1/deployments/{deployment_id}/logs"))
                .await?;

            if let Some(lines) = logs["lines"].as_array() {
                for line in lines {
                    if let Some(text) = line.as_str() {
                        println!("{text}");
                    }
                }
            } else {
                crate::output::print_json(&logs, format)?;
            }
        }

        DeploymentsCommand::Rollback { function, to } => {
            let spinner = style::spinner();
            spinner.set_message(format!("Rolling back {function} to v{to}..."));

            let resp: serde_json::Value = client
                .post(
                    &format!("/v1/functions/{function}/rollback"),
                    &serde_json::json!({ "target_version": to }),
                )
                .await?;

            spinner.finish_and_clear();

            let new_deployment = resp["deployment_id"].as_str().unwrap_or("?");
            println!(
                "{}",
                style::success(&format!(
                    "Rolled back {function} to v{to} (deployment: {new_deployment})"
                ))
            );
        }
    }

    Ok(())
}
