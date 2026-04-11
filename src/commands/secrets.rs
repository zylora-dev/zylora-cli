use anyhow::Result;
use clap::Subcommand;
use serde::{Deserialize, Serialize};

use crate::client::ApiClient;
use crate::config;
use crate::output::OutputFormat;
use crate::style;

#[derive(Debug, Subcommand)]
pub enum SecretsCommand {
    /// Create or update a secret.
    Set {
        /// Secret name.
        name: String,

        /// Secret value (or pipe from stdin).
        value: Option<String>,
    },

    /// List secret names (values are never shown).
    List,

    /// Delete a secret.
    Delete {
        /// Secret name.
        name: String,
    },
}

#[derive(Debug, Serialize, Deserialize, tabled::Tabled)]
pub struct SecretRow {
    pub name: String,
    pub created_at: String,
    pub updated_at: String,
}

pub async fn run(cmd: SecretsCommand, format: OutputFormat, yes: bool) -> Result<()> {
    let token = config::require_token()?;
    let client = ApiClient::new(token)?;

    match cmd {
        SecretsCommand::Set { name, value } => {
            let secret_value = resolve_secret_value(value)?;

            client
                .post_no_response(
                    "/v1/secrets",
                    &serde_json::json!({
                        "name": name,
                        "value": secret_value,
                    }),
                )
                .await?;

            println!("{}", style::success(&format!("Secret '{name}' set.")));
        }

        SecretsCommand::List => {
            let secrets: Vec<SecretRow> = client.get("/v1/secrets").await?;
            crate::output::print_list(&secrets, format)?;
        }

        SecretsCommand::Delete { name } => {
            if !yes {
                let confirm = dialoguer::Confirm::new()
                    .with_prompt(format!("Delete secret '{name}'?"))
                    .default(false)
                    .interact()
                    .unwrap_or(false);

                if !confirm {
                    println!("Cancelled.");
                    return Ok(());
                }
            }

            client.delete(&format!("/v1/secrets/{name}")).await?;
            println!("{}", style::success(&format!("Secret '{name}' deleted.")));
        }
    }

    Ok(())
}

/// Resolve secret value from argument or stdin.
fn resolve_secret_value(value: Option<String>) -> Result<String> {
    if let Some(v) = value {
        return Ok(v);
    }

    // Read from stdin if piped
    if !std::io::IsTerminal::is_terminal(&std::io::stdin()) {
        let mut buf = String::new();
        std::io::Read::read_to_string(&mut std::io::stdin(), &mut buf)?;
        let trimmed = buf.trim().to_string();
        if !trimmed.is_empty() {
            return Ok(trimmed);
        }
    }

    // Interactive prompt (hidden input)
    let value = dialoguer::Password::new()
        .with_prompt("Secret value")
        .interact()?;

    Ok(value)
}
