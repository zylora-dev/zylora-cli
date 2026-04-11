use anyhow::Result;
use clap::Subcommand;
use serde::{Deserialize, Serialize};

use crate::client::ApiClient;
use crate::config;
use crate::output::OutputFormat;
use crate::style;

#[derive(Debug, Subcommand)]
pub enum BillingCommand {
    /// Show current credit balance and plan.
    Status,

    /// Show usage summary for a period.
    Usage {
        /// Period: current_month, last_month.
        #[arg(long, default_value = "current_month")]
        period: String,
    },

    /// Set monthly spending cap.
    Budget {
        /// Monthly budget cap in dollars (e.g., 50.00).
        amount: f64,
    },
}

#[derive(Debug, Serialize, Deserialize)]
struct CreditsResponse {
    balance_cents: i64,
    plan: String,
    auto_recharge: bool,
    budget_cap_cents: Option<i64>,
}

impl std::fmt::Display for CreditsResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(
            f,
            "Balance:       {}",
            style::format_credits(self.balance_cents)
        )?;
        writeln!(f, "Plan:          {}", self.plan)?;
        writeln!(
            f,
            "Auto-recharge: {}",
            if self.auto_recharge { "on" } else { "off" }
        )?;
        if let Some(cap) = self.budget_cap_cents {
            writeln!(f, "Budget cap:    {}", style::format_credits(cap))?;
        }
        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, tabled::Tabled)]
struct UsageBreakdown {
    event_type: String,
    total_cents: String,
    count: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct UsageResponse {
    period: String,
    total_cost_cents: i64,
    breakdown: Vec<UsageBreakdown>,
}

impl std::fmt::Display for UsageResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Period: {}", self.period)?;
        writeln!(
            f,
            "Total:  {}",
            style::format_credits(self.total_cost_cents)
        )?;
        Ok(())
    }
}

pub async fn run(cmd: BillingCommand, format: OutputFormat) -> Result<()> {
    let token = config::require_token()?;
    let client = ApiClient::new(token)?;

    match cmd {
        BillingCommand::Status => {
            let credits: CreditsResponse = client.get("/v1/billing/credits").await?;
            crate::output::print_item(&credits, format)?;
        }

        BillingCommand::Usage { period } => {
            let usage: UsageResponse = client
                .get_with_query("/v1/billing/usage", &[("period", &period)])
                .await?;

            match format {
                OutputFormat::Table => {
                    println!("{usage}");
                    if !usage.breakdown.is_empty() {
                        println!("Breakdown:");
                        crate::output::print_list(&usage.breakdown, format)?;
                    }
                }
                _ => {
                    crate::output::print_json(&serde_json::to_value(&usage)?, format)?;
                }
            }
        }

        BillingCommand::Budget { amount } => {
            let cents = (amount * 100.0) as i64;
            client
                .post_no_response(
                    "/v1/billing/budget",
                    &serde_json::json!({ "budget_cap_cents": cents }),
                )
                .await?;
            println!(
                "{}",
                style::success(&format!(
                    "Monthly budget cap set to {}",
                    style::format_credits(cents)
                ))
            );
        }
    }

    Ok(())
}
