use anyhow::Result;
use clap::Parser;
use commands::Cli;

mod client;
mod commands;
mod config;
mod error;
mod output;
mod style;

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    if let Err(e) = run(cli).await {
        let stderr = console::Term::stderr();
        let _ = stderr.write_line(&style::error_message(&e));
        std::process::exit(1);
    }
}

async fn run(cli: Cli) -> Result<()> {
    // Respect NO_COLOR env var and --no-color flag
    if cli.no_color || std::env::var("NO_COLOR").is_ok() {
        console::set_colors_enabled(false);
        console::set_colors_enabled_stderr(false);
    }

    cli.execute().await
}

