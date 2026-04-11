pub mod auth;
pub mod billing;
pub mod deploy;
pub mod deployments;
pub mod functions;
pub mod init;
pub mod invoke;
pub mod metrics;
pub mod models;
pub mod secrets;

use anyhow::Result;
use clap::{Parser, Subcommand};

use crate::output::OutputFormat;

/// Zylora GPU platform CLI.
///
/// Deploy serverless GPU functions, push models, manage your account.
/// Docs: https://docs.zylora.dev/cli
#[derive(Debug, Parser)]
#[command(
    name = "zy",
    version,
    about = "Zylora GPU platform CLI",
    long_about = None,
    propagate_version = true
)]
pub struct Cli {
    /// Disable colored output (or set NO_COLOR=1).
    #[arg(long, global = true)]
    pub no_color: bool,

    /// Output format.
    #[arg(long, global = true, default_value = "table")]
    pub output: OutputFormat,

    /// Skip confirmation prompts (for CI/CD).
    #[arg(long, short = 'y', global = true)]
    pub yes: bool,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Authenticate with Zylora (browser-based OAuth).
    Login,

    /// Remove stored authentication token.
    Logout,

    /// Show current user, organization, and plan.
    Whoami,

    /// Initialize a new Zylora project in the current directory.
    Init(init::InitArgs),

    /// Deploy the current project to Zylora.
    Deploy(deploy::DeployArgs),

    /// Invoke a deployed function.
    Invoke(invoke::InvokeArgs),

    /// Tail function invocation logs.
    Logs(invoke::LogsArgs),

    /// Function management commands.
    #[command(subcommand)]
    Functions(functions::FunctionsCommand),

    /// Model management commands.
    #[command(subcommand)]
    Models(models::ModelsCommand),

    /// Deployment history and rollback.
    #[command(subcommand)]
    Deployments(deployments::DeploymentsCommand),

    /// Billing, credits, and usage.
    #[command(subcommand)]
    Billing(billing::BillingCommand),

    /// Secret management.
    #[command(subcommand)]
    Secrets(secrets::SecretsCommand),

    /// Show function metrics.
    Metrics(metrics::MetricsArgs),

    /// Manage organizations.
    #[command(subcommand)]
    Org(auth::OrgCommand),

    /// Print current auth token (for CI/CD piping).
    #[command(name = "auth")]
    Auth(auth::AuthArgs),
}

impl Cli {
    pub async fn execute(self) -> Result<()> {
        let format = self.output;
        let yes = self.yes;

        match self.command {
            Command::Login => auth::login().await,
            Command::Logout => auth::logout(),
            Command::Whoami => auth::whoami(format).await,
            Command::Init(args) => init::run(args).await,
            Command::Deploy(args) => deploy::run(args, format).await,
            Command::Invoke(args) => invoke::run(args, format).await,
            Command::Logs(args) => invoke::logs(args).await,
            Command::Functions(cmd) => functions::run(cmd, format, yes).await,
            Command::Models(cmd) => models::run(cmd, format, yes).await,
            Command::Deployments(cmd) => deployments::run(cmd, format).await,
            Command::Billing(cmd) => billing::run(cmd, format).await,
            Command::Secrets(cmd) => secrets::run(cmd, format, yes).await,
            Command::Metrics(args) => metrics::run(args, format).await,
            Command::Org(cmd) => auth::org(cmd, format).await,
            Command::Auth(args) => auth::token(args),
        }
    }
}
