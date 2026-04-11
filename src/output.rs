use anyhow::Result;
use serde::Serialize;
use tabled::{Table, Tabled};

/// Output format as selected by the user.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum OutputFormat {
    #[default]
    Table,
    Json,
    Yaml,
}

/// Print a list of items in the requested format.
pub fn print_list<T: Serialize + Tabled>(items: &[T], format: OutputFormat) -> Result<()> {
    match format {
        OutputFormat::Table => {
            if items.is_empty() {
                println!("No results.");
            } else {
                let table = Table::new(items)
                    .with(tabled::settings::Style::rounded())
                    .to_string();
                println!("{table}");
            }
        }
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(items)?);
        }
        OutputFormat::Yaml => {
            // Serialize as JSON then print (no yaml dep — keep it simple)
            println!("{}", serde_json::to_string_pretty(items)?);
        }
    }
    Ok(())
}

/// Print a single item in the requested format.
pub fn print_item<T: Serialize + std::fmt::Display>(item: &T, format: OutputFormat) -> Result<()> {
    match format {
        OutputFormat::Table => {
            println!("{item}");
        }
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(item)?);
        }
        OutputFormat::Yaml => {
            println!("{}", serde_json::to_string_pretty(item)?);
        }
    }
    Ok(())
}

/// Print a raw JSON value in the requested format.
pub fn print_json(value: &serde_json::Value, format: OutputFormat) -> Result<()> {
    match format {
        OutputFormat::Json | OutputFormat::Yaml => {
            println!("{}", serde_json::to_string_pretty(value)?);
        }
        OutputFormat::Table => {
            // Best-effort pretty print for table mode
            println!("{}", serde_json::to_string_pretty(value)?);
        }
    }
    Ok(())
}
