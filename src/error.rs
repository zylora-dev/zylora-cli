use thiserror::Error;

/// CLI-specific errors.
#[derive(Debug, Error)]
#[allow(dead_code)]
pub enum CliError {
    #[error("Not logged in. Run `zy login` first.")]
    NotAuthenticated,

    #[error("Config file not found at {path}")]
    ConfigNotFound { path: String },

    #[error("Project not initialized. Run `zy init` in your project directory.")]
    ProjectNotInitialized,

    #[error("{0}")]
    Api(ApiErrorDetail),

    #[error("{0}")]
    Other(String),
}

/// Structured API error from the server.
#[derive(Debug)]
#[allow(dead_code)]
pub struct ApiErrorDetail {
    pub status: u16,
    pub code: String,
    pub message: String,
    pub request_id: Option<String>,
}

impl std::fmt::Display for ApiErrorDetail {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({})", self.message, self.status)?;
        if let Some(ref rid) = self.request_id {
            write!(f, " [request_id: {rid}]")?;
        }
        Ok(())
    }
}
