use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use serde::Deserialize;
use tokio::io::AsyncWriteExt;

use crate::client::ApiClient;
use crate::config;
use crate::output::OutputFormat;
use crate::style;

// ── zy login ─────────────────────────────────────────────────────────────────

/// Browser-based OAuth flow:
/// 1. Start local HTTP server on random port
/// 2. Open browser to app.zylora.dev/cli-auth?port={port}
/// 3. Browser redirects back with token
/// 4. Store token in ~/.zylora/config.toml
pub async fn login() -> Result<()> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .context("Failed to bind local callback server")?;
    let port = listener.local_addr()?.port();

    let auth_url = format!("https://app.zylora.dev/cli-auth?port={port}");
    println!("Opening browser for authentication...");
    println!("{}", style::dim(&format!("If it doesn't open, visit: {auth_url}")));

    open::that(&auth_url).context("Failed to open browser")?;

    let spinner = style::spinner();
    spinner.set_message("Waiting for authentication...");

    // Accept one connection — the callback from the browser (5-minute timeout)
    let (stream, _) = tokio::time::timeout(
        std::time::Duration::from_secs(300),
        listener.accept(),
    )
    .await
    .context("Authentication timed out after 5 minutes")?
    .context("Callback connection failed")?;
    spinner.finish_and_clear();

    // Read the HTTP request
    stream.readable().await?;
    let mut buf = vec![0u8; 4096];
    let n = stream.try_read(&mut buf).unwrap_or(0);
    let request = String::from_utf8_lossy(&buf[..n]);

    // Extract token from query string: GET /callback?token=...
    let token = request
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|path| {
            url_query_param(path, "token")
        })
        .context("No token received in callback")?;

    // Send HTTP response to browser
    let response = b"HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nConnection: close\r\n\r\n\
        <html><body><h2>Authenticated!</h2><p>You can close this tab and return to the terminal.</p></body></html>";
    stream.writable().await?;
    let mut stream = stream;
    stream.write_all(response).await.ok(); // best-effort — do not fail login if write fails

    // Validate the token by calling whoami
    let client = ApiClient::new(token.clone())?;
    let user: WhoamiResponse = client
        .get("/v1/auth/me")
        .await
        .context("Token validation failed")?;

    // Save to config
    let mut cfg = config::load_config()?;
    cfg.auth.token = Some(token);
    config::save_config(&cfg)?;

    println!(
        "{}",
        style::success(&format!(
            "Logged in as {} ({})",
            user.email,
            user.plan.unwrap_or_else(|| "free".into())
        ))
    );
    Ok(())
}

/// Extract a query parameter value from a URL path.
fn url_query_param(path: &str, key: &str) -> Option<String> {
    let query = path.split('?').nth(1)?;
    for pair in query.split('&') {
        let mut parts = pair.splitn(2, '=');
        if parts.next()? == key {
            return parts.next().map(String::from);
        }
    }
    None
}

// ── zy logout ────────────────────────────────────────────────────────────────

pub fn logout() -> Result<()> {
    let mut cfg = config::load_config()?;
    cfg.auth.token = None;
    config::save_config(&cfg)?;
    println!("{}", style::success("Logged out."));
    Ok(())
}

// ── zy whoami ────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, serde::Serialize)]
struct WhoamiResponse {
    email: String,
    plan: Option<String>,
    org: Option<String>,
}

impl std::fmt::Display for WhoamiResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Email: {}", self.email)?;
        writeln!(
            f,
            "Plan:  {}",
            self.plan.as_deref().unwrap_or("free")
        )?;
        if let Some(ref org) = self.org {
            writeln!(f, "Org:   {org}")?;
        }
        Ok(())
    }
}

pub async fn whoami(format: OutputFormat) -> Result<()> {
    let token = config::require_token()?;
    let client = ApiClient::new(token)?;
    let user: WhoamiResponse = client.get("/v1/auth/me").await?;
    crate::output::print_item(&user, format)?;
    Ok(())
}

// ── zy auth token ────────────────────────────────────────────────────────────

#[derive(Debug, Args)]
pub struct AuthArgs {
    #[command(subcommand)]
    pub command: AuthCommand,
}

#[derive(Debug, Subcommand)]
pub enum AuthCommand {
    /// Print the current auth token.
    Token,
}

pub fn token(args: AuthArgs) -> Result<()> {
    match args.command {
        AuthCommand::Token => {
            let t = config::require_token()?;
            println!("{t}");
            Ok(())
        }
    }
}

// ── zy org ───────────────────────────────────────────────────────────────────

#[derive(Debug, Subcommand)]
pub enum OrgCommand {
    /// List organizations you belong to.
    List,
    /// Switch the default organization context.
    Switch {
        /// Organization slug.
        slug: String,
    },
}

#[derive(Debug, Deserialize, serde::Serialize, tabled::Tabled)]
pub struct OrgRow {
    pub slug: String,
    pub name: String,
    pub role: String,
}

pub async fn org(cmd: OrgCommand, format: OutputFormat) -> Result<()> {
    let token = config::require_token()?;
    let client = ApiClient::new(token)?;

    match cmd {
        OrgCommand::List => {
            let orgs: Vec<OrgRow> = client.get("/v1/orgs").await?;
            crate::output::print_list(&orgs, format)?;
        }
        OrgCommand::Switch { slug } => {
            // Validate the org exists
            let _: serde_json::Value = client.get(&format!("/v1/orgs/{slug}")).await?;

            let mut cfg = config::load_config()?;
            cfg.defaults.org = Some(slug.clone());
            config::save_config(&cfg)?;
            println!("{}", style::success(&format!("Switched to org: {slug}")));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_token_from_callback() {
        let path = "/callback?token=zy_live_abc123&state=ok";
        assert_eq!(
            url_query_param(path, "token"),
            Some("zy_live_abc123".to_string())
        );
    }

    #[test]
    fn extract_missing_param() {
        let path = "/callback?state=ok";
        assert_eq!(url_query_param(path, "token"), None);
    }
}
