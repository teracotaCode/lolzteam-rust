//! # lolzteam
//!
//! Rust API wrapper for the Lolzteam Forum and Market APIs.
//!
//! ## Quick start
//!
//! ```rust,no_run
//! use lolzteam::ForumClient;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let client = ForumClient::new("your-token-here")?;
//!     // Use client.service().method() ...
//!     Ok(())
//! }
//! ```

pub mod runtime;
pub mod generated;

pub use runtime::errors::LolzteamError;
pub use runtime::types::{ClientConfig, FileUpload, ProxyConfig, RequestOptions, StringOrInt};
pub use runtime::HttpClient;

use generated::forum::client::ForumService;
use generated::market::client::MarketService;
use std::sync::Arc;

// ---------------------------------------------------------------------------
// ForumClient
// ---------------------------------------------------------------------------

/// High-level client for the Lolzteam **Forum** API.
#[derive(Debug, Clone)]
pub struct ForumClient {
    http: Arc<HttpClient>,
    forum: ForumService,
}

impl ForumClient {
    /// Create a new Forum client with default settings.
    pub fn new(token: &str) -> Result<Self, LolzteamError> {
        Self::with_config(ClientConfig::forum(token))
    }

    /// Create a new Forum client with custom configuration.
    pub fn with_config(config: ClientConfig) -> Result<Self, LolzteamError> {
        let http = Arc::new(HttpClient::new(config)?);
        let forum = ForumService::new(http.clone());
        Ok(Self { http, forum })
    }

    /// Access the underlying HTTP client.
    pub fn http(&self) -> &HttpClient {
        &self.http
    }

    /// Access the generated Forum service.
    pub fn service(&self) -> &ForumService {
        &self.forum
    }
}

// ---------------------------------------------------------------------------
// MarketClient
// ---------------------------------------------------------------------------

/// High-level client for the Lolzteam **Market** API.
#[derive(Debug, Clone)]
pub struct MarketClient {
    http: Arc<HttpClient>,
    market: MarketService,
}

impl MarketClient {
    /// Create a new Market client with default settings.
    pub fn new(token: &str) -> Result<Self, LolzteamError> {
        Self::with_config(ClientConfig::market(token))
    }

    /// Create a new Market client with custom configuration.
    pub fn with_config(config: ClientConfig) -> Result<Self, LolzteamError> {
        let http = Arc::new(HttpClient::new(config)?);
        let market = MarketService::new(http.clone());
        Ok(Self { http, market })
    }

    /// Access the underlying HTTP client.
    pub fn http(&self) -> &HttpClient {
        &self.http
    }

    /// Access the generated Market service.
    pub fn service(&self) -> &MarketService {
        &self.market
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forum_client_creates_with_valid_token() {
        let client = ForumClient::new("test-token");
        assert!(client.is_ok());
        let client = client.unwrap();
        assert_eq!(client.http().base_url(), "https://prod-api.lolz.live");
    }

    #[test]
    fn market_client_creates_with_valid_token() {
        let client = MarketClient::new("test-token");
        assert!(client.is_ok());
        let client = client.unwrap();
        assert_eq!(client.http().base_url(), "https://prod-api.lzt.market");
    }

    #[test]
    fn rejects_empty_token() {
        assert!(ForumClient::new("").is_err());
        assert!(MarketClient::new("").is_err());
    }

    #[test]
    fn string_or_int_serde() {
        let s = StringOrInt::from("hello");
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(json, "\"hello\"");

        let i = StringOrInt::from(42i64);
        let json = serde_json::to_string(&i).unwrap();
        assert_eq!(json, "42");

        let parsed: StringOrInt = serde_json::from_str("\"world\"").unwrap();
        assert_eq!(parsed, StringOrInt::String("world".into()));

        let parsed: StringOrInt = serde_json::from_str("99").unwrap();
        assert_eq!(parsed, StringOrInt::Int(99));
    }

    #[test]
    fn error_retryable() {
        let err = LolzteamError::Server {
            status: 503,
            body: "down".into(),
        };
        assert!(err.is_retryable());
        assert!(!err.is_rate_limit());

        let err = LolzteamError::RateLimit {
            body: "slow down".into(),
            retry_after: Some(1.5),
        };
        assert!(err.is_retryable());
        assert!(err.is_rate_limit());
        assert_eq!(err.retry_after(), Some(1.5));

        let err = LolzteamError::Auth {
            body: "bad".into(),
        };
        assert!(!err.is_retryable());
    }
}
