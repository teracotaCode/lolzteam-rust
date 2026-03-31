//! Core HTTP client that wraps reqwest with auth, rate limiting, and retries.

use crate::runtime::errors::LolzteamError;
use crate::runtime::rate_limiter::RateLimiter;
use crate::runtime::retry::{execute_with_retry, RetryConfig};
use crate::runtime::types::{ClientConfig, RequestOptions};
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, USER_AGENT};
use reqwest::Method;
use std::str::FromStr;

/// Low-level HTTP client with authentication, rate limiting, and retry.
pub struct HttpClient {
    client: reqwest::Client,
    base_url: String,
    token: String,
    retry_config: RetryConfig,
    rate_limiter: RateLimiter,
}

impl std::fmt::Debug for HttpClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HttpClient")
            .field("base_url", &self.base_url)
            .field("token", &"<redacted>")
            .finish()
    }
}

impl HttpClient {
    /// Build a new `HttpClient` from the given configuration.
    pub fn new(config: ClientConfig) -> Result<Self, LolzteamError> {
        if config.token.is_empty() {
            return Err(LolzteamError::Config("Token cannot be empty".into()));
        }

        let mut default_headers = HeaderMap::new();
        default_headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {}", config.token))
                .map_err(|e| LolzteamError::Config(format!("Invalid token characters: {}", e)))?,
        );
        default_headers.insert(
            USER_AGENT,
            HeaderValue::from_str(&config.user_agent)
                .map_err(|e| LolzteamError::Config(format!("Invalid User-Agent: {}", e)))?,
        );

        for (k, v) in &config.extra_headers {
            let name = reqwest::header::HeaderName::from_str(k)
                .map_err(|e| LolzteamError::Config(format!("Invalid header name '{}': {}", k, e)))?;
            let val = HeaderValue::from_str(v)
                .map_err(|e| LolzteamError::Config(format!("Invalid header value for '{}': {}", k, e)))?;
            default_headers.insert(name, val);
        }

        let mut builder = reqwest::Client::builder()
            .timeout(config.timeout)
            .default_headers(default_headers);

        if let Some(ref proxy_cfg) = config.proxy {
            let proxy = proxy_cfg.to_reqwest_proxy()?;
            builder = builder.proxy(proxy);
        }

        let client = builder
            .build()
            .map_err(|e| LolzteamError::Config(format!("Failed to build HTTP client: {}", e)))?;

        let retry_config = RetryConfig {
            max_retries: config.max_retries,
            ..RetryConfig::default()
        };

        let rate_limiter = RateLimiter::new(config.rps_general, config.rps_search);

        Ok(Self {
            client,
            base_url: config.base_url.trim_end_matches('/').to_string(),
            token: config.token,
            retry_config,
            rate_limiter,
        })
    }

    /// Return a reference to the base URL.
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Return a reference to the token (useful for generated code).
    #[allow(dead_code)]
    pub(crate) fn token(&self) -> &str {
        &self.token
    }

    /// Perform an HTTP request with rate limiting and retry.
    pub async fn request(
        &self,
        method: &str,
        path: &str,
        opts: RequestOptions,
    ) -> Result<serde_json::Value, LolzteamError> {
        let method = Method::from_str(method)
            .map_err(|e| LolzteamError::Config(format!("Invalid HTTP method '{}': {}", method, e)))?;

        let url = format!("{}/{}", self.base_url, path.trim_start_matches('/'));

        // Rate limit
        self.rate_limiter.wait(opts.is_search).await;

        let client = self.client.clone();
        let url = url.clone();
        let method = method.clone();

        execute_with_retry(
            || {
                let client = client.clone();
                let url = url.clone();
                let method = method.clone();
                let opts = opts.clone();
                async move {
                    let mut req = client.request(method, &url);

                    // Query parameters
                    if let Some(ref query) = opts.query {
                        req = req.query(query);
                    }

                    // Body: JSON, form, or multipart
                    if let Some(ref json) = opts.json {
                        req = req.json(json);
                    } else if let Some(ref form) = opts.form {
                        req = req.form(form);
                    } else if opts.files.is_some() || opts.multipart_fields.is_some() {
                        let mut multipart = reqwest::multipart::Form::new();
                        if let Some(ref fields) = opts.multipart_fields {
                            for (name, value) in fields {
                                multipart = multipart.text(name.clone(), value.clone());
                            }
                        }
                        if let Some(ref files) = opts.files {
                            for upload in files {
                                let part = reqwest::multipart::Part::bytes(upload.data.clone())
                                    .file_name(upload.file_name.clone())
                                    .mime_str(&upload.mime_type)
                                    .map_err(|e| {
                                        LolzteamError::Config(format!(
                                            "Invalid MIME type '{}': {}",
                                            upload.mime_type, e
                                        ))
                                    })?;
                                multipart = multipart.part(upload.field_name.clone(), part);
                            }
                        }
                        req = req.multipart(multipart);
                    }

                    let response = req.send().await?;
                    let status = response.status().as_u16();

                    // Parse Retry-After header
                    let retry_after = response
                        .headers()
                        .get("retry-after")
                        .and_then(|v| v.to_str().ok())
                        .and_then(|v| v.parse::<f64>().ok());

                    let body = response.text().await?;

                    if status >= 200 && status < 300 {
                        let value: serde_json::Value =
                            serde_json::from_str(&body).unwrap_or_else(|_| {
                                serde_json::json!({ "raw": body })
                            });
                        Ok(value)
                    } else {
                        Err(LolzteamError::from_status(status, body, retry_after))
                    }
                }
            },
            &self.retry_config,
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_empty_token() {
        let config = ClientConfig::forum("");
        let result = HttpClient::new(config);
        assert!(result.is_err());
    }

    #[test]
    fn builds_with_valid_config() {
        let config = ClientConfig::forum("test-token-123");
        let client = HttpClient::new(config);
        assert!(client.is_ok());
    }
}
