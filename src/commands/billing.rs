use anyhow::{Result, anyhow, bail};
use chrono::{Datelike, TimeZone, Utc};
use clap::Subcommand;
use serde::{Deserialize, Serialize};

use crate::client::ApiClient;
use crate::config;
use crate::output::OutputFormat;
use crate::style;

#[derive(Debug, Subcommand)]
pub enum BillingCommand {
    /// Show current credit balance and billing settings.
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
    free_credits_cents: i64,
    auto_recharge_enabled: bool,
    auto_recharge_threshold_cents: i64,
    auto_recharge_amount_cents: i64,
    monthly_cap_cents: Option<i64>,
    current_month_usage_cents: i64,
}

impl std::fmt::Display for CreditsResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(
            f,
            "Balance:       {}",
            style::format_credits(self.balance_cents)
        )?;
        writeln!(
            f,
            "Free credits:  {}",
            style::format_credits(self.free_credits_cents)
        )?;
        writeln!(
            f,
            "Auto-recharge: {}",
            if self.auto_recharge_enabled { "on" } else { "off" }
        )?;
        if self.auto_recharge_enabled {
            writeln!(
                f,
                "Threshold:     {}",
                style::format_credits(self.auto_recharge_threshold_cents)
            )?;
            writeln!(
                f,
                "Top-up amount: {}",
                style::format_credits(self.auto_recharge_amount_cents)
            )?;
        }
        writeln!(
            f,
            "MTD usage:     {}",
            style::format_credits(self.current_month_usage_cents)
        )?;
        if let Some(cap) = self.monthly_cap_cents {
            writeln!(f, "Monthly cap:   {}", style::format_credits(cap))?;
        }
        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct UsageResponse {
    from: String,
    to: String,
    total_cost_cents: i64,
    total_invocations: Option<i64>,
    total_gpu_seconds: Option<f64>,
    buckets: Vec<UsageBucketResponse>,
}

#[derive(Debug, Serialize, Deserialize)]
struct UsageBucketResponse {
    date: String,
    cost_cents: i64,
    invocations: i64,
    gpu_seconds: Option<f64>,
}

#[derive(Debug, Serialize, Deserialize, tabled::Tabled)]
struct UsageBucket {
    date: String,
    cost: String,
    invocations: i64,
    gpu_seconds: String,
}

impl std::fmt::Display for UsageResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "From:   {}", self.from)?;
        writeln!(f, "To:     {}", self.to)?;
        writeln!(
            f,
            "Total:  {}",
            style::format_credits(self.total_cost_cents)
        )?;
        if let Some(total_invocations) = self.total_invocations {
            writeln!(f, "Calls:  {total_invocations}")?;
        }
        if let Some(total_gpu_seconds) = self.total_gpu_seconds {
            writeln!(f, "GPU s:  {:.2}", total_gpu_seconds)?;
        }
        Ok(())
    }
}

fn resolve_period(period: &str) -> Result<(String, String)> {
    let now = Utc::now();

    let (year, month) = match period {
        "current_month" => (now.year(), now.month()),
        "last_month" => {
            if now.month() == 1 {
                (now.year() - 1, 12)
            } else {
                (now.year(), now.month() - 1)
            }
        }
        other => bail!(
            "Unsupported period '{other}'. Use 'current_month' or 'last_month'."
        ),
    };

    let from = Utc
        .with_ymd_and_hms(year, month, 1, 0, 0, 0)
        .single()
        .ok_or_else(|| anyhow!("Failed to compute billing period start"))?;

    let (to_year, to_month) = if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    };

    let to = Utc
        .with_ymd_and_hms(to_year, to_month, 1, 0, 0, 0)
        .single()
        .ok_or_else(|| anyhow!("Failed to compute billing period end"))?;

    Ok((from.to_rfc3339(), to.to_rfc3339()))
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
            let (from, to) = resolve_period(&period)?;
            let usage: UsageResponse = client
                .get_with_query(
                    "/v1/billing/usage",
                    &[("from", &from), ("to", &to), ("granularity", "day")],
                )
                .await?;

            match format {
                OutputFormat::Table => {
                    println!("{usage}");

                    let buckets: Vec<UsageBucket> = usage
                        .buckets
                        .iter()
                        .map(|bucket| UsageBucket {
                            date: bucket.date.clone(),
                            cost: style::format_credits(bucket.cost_cents),
                            invocations: bucket.invocations,
                            gpu_seconds: format!("{:.2}", bucket.gpu_seconds.unwrap_or(0.0)),
                        })
                        .collect();

                    if !buckets.is_empty() {
                        println!("Buckets:");
                        crate::output::print_list(&buckets, format)?;
                    }
                }
                _ => {
                    crate::output::print_json(&serde_json::to_value(&usage)?, format)?;
                }
            }
        }

        BillingCommand::Budget { amount } => {
            let cents = (amount * 100.0) as i64;
            bail!(
                "Monthly cap updates are not exposed by the current API contract. Configure the cap in the dashboard after support lands. Requested value: {}",
                style::format_credits(cents)
            );
        }
    }

    Ok(())
}
