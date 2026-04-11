use console::style;
use indicatif::ProgressStyle;

/// Format a success message with green checkmark.
pub fn success(msg: &str) -> String {
    format!("{} {msg}", style("✓").green().bold())
}

/// Format an error for display on stderr.
pub fn error_message(err: &anyhow::Error) -> String {
    let mut msg = format!("{} {err}", style("Error:").red().bold());

    // Show chain of causes
    for cause in err.chain().skip(1) {
        msg.push_str(&format!("\n  {} {cause}", style("→").dim()));
    }

    // Add suggestion for common errors
    let err_str = err.to_string();
    if err_str.contains("Not logged in") {
        msg.push_str(&format!(
            "\n  {} Run `zy login` to authenticate.",
            style("→").cyan()
        ));
    } else if err_str.contains("402") || err_str.contains("Insufficient credits") {
        msg.push_str(&format!(
            "\n  {} Run `zy billing topup` or enable auto-recharge in the dashboard.",
            style("→").cyan()
        ));
    } else if err_str.contains("Project not initialized") {
        msg.push_str(&format!(
            "\n  {} Run `zy init` in your project directory.",
            style("→").cyan()
        ));
    }

    msg
}

/// Format a warning message.
pub fn warning(msg: &str) -> String {
    format!("{} {msg}", style("⚠").yellow().bold())
}

/// Format a dim info message.
pub fn dim(msg: &str) -> String {
    style(msg).dim().to_string()
}

/// Create a spinner-style progress bar (no length).
pub fn spinner() -> indicatif::ProgressBar {
    let pb = indicatif::ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::with_template("{spinner:.cyan} {msg}")
            .unwrap()
            .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]),
    );
    pb.enable_steady_tick(std::time::Duration::from_millis(80));
    pb
}

/// Create a progress bar for upload/download with bytes.
pub fn progress_bar(total: u64) -> indicatif::ProgressBar {
    let pb = indicatif::ProgressBar::new(total);
    pb.set_style(
        ProgressStyle::with_template(
            "{spinner:.cyan} [{bar:30.cyan/dim}] {bytes}/{total_bytes} ({eta}) {msg}",
        )
        .unwrap()
        .progress_chars("━╸─"),
    );
    pb.enable_steady_tick(std::time::Duration::from_millis(100));
    pb
}

/// Format cents as dollar string for display.
pub fn format_credits(cents: i64) -> String {
    let dollars = cents as f64 / 100.0;
    format!("${dollars:.2}")
}
