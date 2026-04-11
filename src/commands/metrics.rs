use anyhow::Result;
use clap::Args;
use serde::{Deserialize, Serialize};

use crate::client::ApiClient;
use crate::config;
use crate::output::OutputFormat;
use crate::style;

#[derive(Debug, Args)]
pub struct MetricsArgs {
    /// Function name.
    pub function: String,

    /// Time period (1h, 24h, 7d).
    #[arg(long, default_value = "24h")]
    pub period: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct MetricsResponse {
    function: String,
    period: String,
    invocations: u64,
    p50_latency_ms: f64,
    p95_latency_ms: f64,
    p99_latency_ms: f64,
    error_rate: f64,
    cold_start_pct: f64,
    avg_gpu_utilization: f64,
    total_cost_cents: i64,
}

impl std::fmt::Display for MetricsResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Function: {}", self.function)?;
        writeln!(f, "Period:   {}", self.period)?;
        writeln!(f)?;
        writeln!(f, "Invocations:     {}", self.invocations)?;
        writeln!(
            f,
            "Latency (p50):   {:.0}ms",
            self.p50_latency_ms
        )?;
        writeln!(
            f,
            "Latency (p95):   {:.0}ms",
            self.p95_latency_ms
        )?;
        writeln!(
            f,
            "Latency (p99):   {:.0}ms",
            self.p99_latency_ms
        )?;
        writeln!(f, "Error rate:      {:.1}%", self.error_rate * 100.0)?;
        writeln!(
            f,
            "Cold starts:     {:.1}%",
            self.cold_start_pct * 100.0
        )?;
        writeln!(
            f,
            "GPU utilization: {:.1}%",
            self.avg_gpu_utilization * 100.0
        )?;
        writeln!(
            f,
            "Total cost:      {}",
            style::format_credits(self.total_cost_cents)
        )?;
        Ok(())
    }
}

pub async fn run(args: MetricsArgs, format: OutputFormat) -> Result<()> {
    let token = config::require_token()?;
    let client = ApiClient::new(token)?;

    let metrics: MetricsResponse = client
        .get_with_query(
            &format!("/v1/functions/{}/metrics", args.function),
            &[("period", &args.period)],
        )
        .await?;

    crate::output::print_item(&metrics, format)?;
    Ok(())
}
