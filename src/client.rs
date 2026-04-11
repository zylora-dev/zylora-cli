use anyhow::{Context, Result};
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, USER_AGENT};
use reqwest::{Client, Method, Response, StatusCode};
use serde::de::DeserializeOwned;

use crate::error::{ApiErrorDetail, CliError};

/// Default API base URL (overridable via ZYLORA_API_URL).
const DEFAULT_API_URL: &str = "https://api.zylora.dev";

/// API client for the Zylora engine.
pub struct ApiClient {
    client: Client,
    base_url: String,
    token: String,
}

/// Server error response format.
#[derive(serde::Deserialize)]
struct ErrorEnvelope {
    error: ErrorBody,
}

#[derive(serde::Deserialize)]
struct ErrorBody {
    code: String,
    message: String,
    #[serde(default)]
    request_id: Option<String>,
}

impl ApiClient {
    /// Create a new API client with the given auth token.
    pub fn new(token: String) -> Result<Self> {
        let base_url = std::env::var("ZYLORA_API_URL").unwrap_or_else(|_| DEFAULT_API_URL.into());
        let client = Client::builder()
            .default_headers(Self::default_headers(&token)?)
            .timeout(std::time::Duration::from_secs(300))
            .build()
            .context("Failed to initialize HTTP client")?;

        Ok(Self {
            client,
            base_url,
            token,
        })
    }

    fn default_headers(token: &str) -> Result<HeaderMap> {
        let mut headers = HeaderMap::new();
        let auth_value = if token.starts_with("zy_") {
            // API key — use X-API-Key style, but server also accepts Bearer
            format!("Bearer {token}")
        } else {
            format!("Bearer {token}")
        };
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&auth_value).context("Invalid auth token characters")?,
        );
        headers.insert(
            USER_AGENT,
            HeaderValue::from_static(concat!("zy-cli/", env!("CARGO_PKG_VERSION"))),
        );
        Ok(headers)
    }

    /// Return base URL (for constructing SSE endpoints etc.).
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Return the raw auth token.
    pub fn token(&self) -> &str {
        &self.token
    }

    /// GET request, deserialize JSON response.
    pub async fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        let resp = self.request(Method::GET, path, None::<&()>).await?;
        self.handle_response(resp).await
    }

    /// GET with query parameters.
    pub async fn get_with_query<T: DeserializeOwned>(
        &self,
        path: &str,
        params: &[(&str, &str)],
    ) -> Result<T> {
        let query_string: String = params
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join("&");
        let url = if query_string.is_empty() {
            format!("{}{path}", self.base_url)
        } else {
            format!("{}{path}?{query_string}", self.base_url)
        };
        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .context("Request failed")?;
        self.handle_response(resp).await
    }

    /// POST request with JSON body.
    pub async fn post<T: DeserializeOwned, B: serde::Serialize>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T> {
        let resp = self.request(Method::POST, path, Some(body)).await?;
        self.handle_response(resp).await
    }

    /// POST that returns no body (204-style).
    pub async fn post_no_response<B: serde::Serialize>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<()> {
        let resp = self.request(Method::POST, path, Some(body)).await?;
        self.check_status(resp).await
    }

    /// PUT request with JSON body.
    #[allow(dead_code)]
    pub async fn put<T: DeserializeOwned, B: serde::Serialize>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T> {
        let resp = self.request(Method::PUT, path, Some(body)).await?;
        self.handle_response(resp).await
    }

    /// DELETE request.
    pub async fn delete(&self, path: &str) -> Result<()> {
        let resp = self.request(Method::DELETE, path, None::<&()>).await?;
        self.check_status(resp).await
    }

    /// Raw POST returning the response (for SSE streaming etc.).
    pub async fn post_raw<B: serde::Serialize>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<Response> {
        self.request(Method::POST, path, Some(body)).await
    }

    /// Send a multipart form request (for file uploads).
    pub async fn upload(
        &self,
        path: &str,
        form: reqwest::multipart::Form,
    ) -> Result<serde_json::Value> {
        let url = format!("{}{path}", self.base_url);
        let resp = self
            .client
            .post(&url)
            .multipart(form)
            .send()
            .await
            .context("Upload request failed")?;
        self.handle_response(resp).await
    }

    // ── Internal ─────────────────────────────────────────────────────────────

    async fn request<B: serde::Serialize>(
        &self,
        method: Method,
        path: &str,
        body: Option<&B>,
    ) -> Result<Response> {
        let url = format!("{}{path}", self.base_url);
        let mut req = self.client.request(method, &url);
        if let Some(b) = body {
            req = req.json(b);
        }
        req.send().await.context("Request failed")
    }

    async fn handle_response<T: DeserializeOwned>(&self, resp: Response) -> Result<T> {
        let status = resp.status();
        if status.is_success() {
            let body = resp.json::<T>().await.context("Failed to parse response")?;
            return Ok(body);
        }
        Err(self.extract_error(resp, status).await)
    }

    async fn check_status(&self, resp: Response) -> Result<()> {
        let status = resp.status();
        if status.is_success() {
            return Ok(());
        }
        Err(self.extract_error(resp, status).await)
    }

    async fn extract_error(&self, resp: Response, status: StatusCode) -> anyhow::Error {
        let request_id = resp
            .headers()
            .get("x-request-id")
            .and_then(|v| v.to_str().ok())
            .map(String::from);

        if let Ok(envelope) = resp.json::<ErrorEnvelope>().await {
            return CliError::Api(ApiErrorDetail {
                status: status.as_u16(),
                code: envelope.error.code,
                message: envelope.error.message,
                request_id: request_id.or(envelope.error.request_id),
            })
            .into();
        }

        CliError::Api(ApiErrorDetail {
            status: status.as_u16(),
            code: "unknown".into(),
            message: format!("HTTP {status}"),
            request_id,
        })
        .into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_api_url() {
        assert_eq!(DEFAULT_API_URL, "https://api.zylora.dev");
    }
}
