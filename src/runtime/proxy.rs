//! Proxy configuration helpers.

use crate::runtime::errors::LolzteamError;
use crate::runtime::types::ProxyConfig;

impl ProxyConfig {
    /// Validate the proxy URL and convert it into a [`reqwest::Proxy`].
    pub fn to_reqwest_proxy(&self) -> Result<reqwest::Proxy, LolzteamError> {
        if self.url.is_empty() {
            return Err(LolzteamError::Config("Proxy URL cannot be empty".into()));
        }

        // Basic validation: must start with a known scheme.
        let valid_schemes = ["http://", "https://", "socks5://", "socks5h://"];
        if !valid_schemes.iter().any(|s| self.url.starts_with(s)) {
            return Err(LolzteamError::Config(format!(
                "Proxy URL must start with one of: {}",
                valid_schemes.join(", ")
            )));
        }

        reqwest::Proxy::all(&self.url).map_err(|e| {
            LolzteamError::Config(format!("Invalid proxy URL '{}': {}", self.url, e))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_empty_url() {
        let cfg = ProxyConfig {
            url: String::new(),
        };
        assert!(cfg.to_reqwest_proxy().is_err());
    }

    #[test]
    fn rejects_bad_scheme() {
        let cfg = ProxyConfig {
            url: "ftp://example.com".into(),
        };
        assert!(cfg.to_reqwest_proxy().is_err());
    }

    #[test]
    fn accepts_socks5() {
        let cfg = ProxyConfig {
            url: "socks5://127.0.0.1:1080".into(),
        };
        assert!(cfg.to_reqwest_proxy().is_ok());
    }

    #[test]
    fn accepts_http() {
        let cfg = ProxyConfig {
            url: "http://127.0.0.1:8080".into(),
        };
        assert!(cfg.to_reqwest_proxy().is_ok());
    }
}
