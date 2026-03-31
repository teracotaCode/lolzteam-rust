//! Error types for the Lolzteam API client.

/// All errors that can occur when using the Lolzteam API client.
#[derive(Debug, thiserror::Error)]
pub enum LolzteamError {
    /// Generic HTTP error with status code and response body.
    #[error("HTTP {status}: {body}")]
    Http {
        status: u16,
        body: String,
        retry_after: Option<f64>,
    },

    /// Authentication failed (HTTP 401).
    #[error("Authentication failed (401)")]
    Auth { body: String },

    /// Forbidden (HTTP 403).
    #[error("Forbidden (403)")]
    Forbidden { body: String },

    /// Resource not found (HTTP 404).
    #[error("Not found (404)")]
    NotFound { body: String },

    /// Rate limited (HTTP 429).
    #[error("Rate limited (429)")]
    RateLimit {
        body: String,
        retry_after: Option<f64>,
    },

    /// Server error (HTTP 5xx).
    #[error("Server error ({status})")]
    Server { status: u16, body: String },

    /// Underlying network / transport error from reqwest.
    #[error("Network error: {0}")]
    Network(#[from] reqwest::Error),

    /// Configuration error (invalid base URL, missing token, etc.).
    #[error("Config error: {0}")]
    Config(String),

    /// All retry attempts have been exhausted.
    #[error("Retry exhausted after {attempts} attempts: {last_error}")]
    RetryExhausted {
        attempts: u32,
        last_error: Box<LolzteamError>,
    },
}

impl LolzteamError {
    /// Returns `true` if this error is eligible for automatic retry.
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            LolzteamError::RateLimit { .. }
                | LolzteamError::Server { .. }
                | LolzteamError::Network(_)
                | LolzteamError::Http {
                    status: 408 | 429 | 502 | 503 | 504,
                    ..
                }
        )
    }

    /// Returns `true` if this error represents a rate-limit response.
    pub fn is_rate_limit(&self) -> bool {
        matches!(self, LolzteamError::RateLimit { .. })
    }

    /// If the server sent a `Retry-After` value, return it as seconds.
    pub fn retry_after(&self) -> Option<f64> {
        match self {
            LolzteamError::RateLimit { retry_after, .. } => *retry_after,
            LolzteamError::Http { retry_after, .. } => *retry_after,
            _ => None,
        }
    }

    /// Build the appropriate error variant from an HTTP status code and body.
    pub(crate) fn from_status(status: u16, body: String, retry_after: Option<f64>) -> Self {
        match status {
            401 => LolzteamError::Auth { body },
            403 => LolzteamError::Forbidden { body },
            404 => LolzteamError::NotFound { body },
            429 => LolzteamError::RateLimit { body, retry_after },
            500..=599 => LolzteamError::Server { status, body },
            _ => LolzteamError::Http {
                status,
                body,
                retry_after,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Display impl tests
    // -----------------------------------------------------------------------

    #[test]
    fn display_http_error() {
        let err = LolzteamError::Http {
            status: 418,
            body: "I'm a teapot".into(),
            retry_after: None,
        };
        assert_eq!(err.to_string(), "HTTP 418: I'm a teapot");
    }

    #[test]
    fn display_auth_error() {
        let err = LolzteamError::Auth {
            body: "bad token".into(),
        };
        assert_eq!(err.to_string(), "Authentication failed (401)");
    }

    #[test]
    fn display_forbidden_error() {
        let err = LolzteamError::Forbidden {
            body: "no access".into(),
        };
        assert_eq!(err.to_string(), "Forbidden (403)");
    }

    #[test]
    fn display_not_found_error() {
        let err = LolzteamError::NotFound {
            body: "gone".into(),
        };
        assert_eq!(err.to_string(), "Not found (404)");
    }

    #[test]
    fn display_rate_limit_error() {
        let err = LolzteamError::RateLimit {
            body: "slow down".into(),
            retry_after: Some(3.0),
        };
        assert_eq!(err.to_string(), "Rate limited (429)");
    }

    #[test]
    fn display_server_error() {
        let err = LolzteamError::Server {
            status: 502,
            body: "bad gateway".into(),
        };
        assert_eq!(err.to_string(), "Server error (502)");
    }

    #[test]
    fn display_config_error() {
        let err = LolzteamError::Config("missing token".into());
        assert_eq!(err.to_string(), "Config error: missing token");
    }

    #[test]
    fn display_retry_exhausted() {
        let inner = LolzteamError::Server {
            status: 503,
            body: "unavailable".into(),
        };
        let err = LolzteamError::RetryExhausted {
            attempts: 3,
            last_error: Box::new(inner),
        };
        assert_eq!(
            err.to_string(),
            "Retry exhausted after 3 attempts: Server error (503)"
        );
    }

    // -----------------------------------------------------------------------
    // is_retryable() tests
    // -----------------------------------------------------------------------

    #[test]
    fn rate_limit_is_retryable() {
        let err = LolzteamError::RateLimit {
            body: String::new(),
            retry_after: None,
        };
        assert!(err.is_retryable());
    }

    #[test]
    fn server_500_is_retryable() {
        let err = LolzteamError::Server {
            status: 500,
            body: String::new(),
        };
        assert!(err.is_retryable());
    }

    #[test]
    fn server_502_is_retryable() {
        let err = LolzteamError::Server {
            status: 502,
            body: String::new(),
        };
        assert!(err.is_retryable());
    }

    #[test]
    fn server_503_is_retryable() {
        let err = LolzteamError::Server {
            status: 503,
            body: String::new(),
        };
        assert!(err.is_retryable());
    }

    #[test]
    fn server_504_is_retryable() {
        let err = LolzteamError::Server {
            status: 504,
            body: String::new(),
        };
        assert!(err.is_retryable());
    }

    #[test]
    fn http_408_is_retryable() {
        let err = LolzteamError::Http {
            status: 408,
            body: String::new(),
            retry_after: None,
        };
        assert!(err.is_retryable());
    }

    #[test]
    fn http_429_is_retryable() {
        let err = LolzteamError::Http {
            status: 429,
            body: String::new(),
            retry_after: None,
        };
        assert!(err.is_retryable());
    }

    #[test]
    fn http_502_is_retryable() {
        let err = LolzteamError::Http {
            status: 502,
            body: String::new(),
            retry_after: None,
        };
        assert!(err.is_retryable());
    }

    #[test]
    fn http_503_is_retryable() {
        let err = LolzteamError::Http {
            status: 503,
            body: String::new(),
            retry_after: None,
        };
        assert!(err.is_retryable());
    }

    #[test]
    fn http_504_is_retryable() {
        let err = LolzteamError::Http {
            status: 504,
            body: String::new(),
            retry_after: None,
        };
        assert!(err.is_retryable());
    }

    #[test]
    fn http_400_not_retryable() {
        let err = LolzteamError::Http {
            status: 400,
            body: String::new(),
            retry_after: None,
        };
        assert!(!err.is_retryable());
    }

    #[test]
    fn auth_not_retryable() {
        let err = LolzteamError::Auth {
            body: String::new(),
        };
        assert!(!err.is_retryable());
    }

    #[test]
    fn forbidden_not_retryable() {
        let err = LolzteamError::Forbidden {
            body: String::new(),
        };
        assert!(!err.is_retryable());
    }

    #[test]
    fn not_found_not_retryable() {
        let err = LolzteamError::NotFound {
            body: String::new(),
        };
        assert!(!err.is_retryable());
    }

    #[test]
    fn config_not_retryable() {
        let err = LolzteamError::Config("bad".into());
        assert!(!err.is_retryable());
    }

    #[test]
    fn retry_exhausted_not_retryable() {
        let inner = LolzteamError::Server {
            status: 503,
            body: String::new(),
        };
        let err = LolzteamError::RetryExhausted {
            attempts: 3,
            last_error: Box::new(inner),
        };
        assert!(!err.is_retryable());
    }

    // -----------------------------------------------------------------------
    // is_rate_limit() tests
    // -----------------------------------------------------------------------

    #[test]
    fn rate_limit_variant_is_rate_limit() {
        let err = LolzteamError::RateLimit {
            body: String::new(),
            retry_after: None,
        };
        assert!(err.is_rate_limit());
    }

    #[test]
    fn server_error_is_not_rate_limit() {
        let err = LolzteamError::Server {
            status: 503,
            body: String::new(),
        };
        assert!(!err.is_rate_limit());
    }

    #[test]
    fn http_429_generic_is_not_rate_limit() {
        // Http variant with status 429 is NOT the RateLimit variant
        let err = LolzteamError::Http {
            status: 429,
            body: String::new(),
            retry_after: None,
        };
        assert!(!err.is_rate_limit());
    }

    // -----------------------------------------------------------------------
    // retry_after() tests
    // -----------------------------------------------------------------------

    #[test]
    fn retry_after_from_rate_limit() {
        let err = LolzteamError::RateLimit {
            body: String::new(),
            retry_after: Some(2.5),
        };
        assert_eq!(err.retry_after(), Some(2.5));
    }

    #[test]
    fn retry_after_from_rate_limit_none() {
        let err = LolzteamError::RateLimit {
            body: String::new(),
            retry_after: None,
        };
        assert_eq!(err.retry_after(), None);
    }

    #[test]
    fn retry_after_from_http() {
        let err = LolzteamError::Http {
            status: 503,
            body: String::new(),
            retry_after: Some(10.0),
        };
        assert_eq!(err.retry_after(), Some(10.0));
    }

    #[test]
    fn retry_after_from_server_is_none() {
        let err = LolzteamError::Server {
            status: 503,
            body: String::new(),
        };
        assert_eq!(err.retry_after(), None);
    }

    #[test]
    fn retry_after_from_config_is_none() {
        let err = LolzteamError::Config("x".into());
        assert_eq!(err.retry_after(), None);
    }

    #[test]
    fn retry_after_from_auth_is_none() {
        let err = LolzteamError::Auth {
            body: String::new(),
        };
        assert_eq!(err.retry_after(), None);
    }

    // -----------------------------------------------------------------------
    // from_status() tests
    // -----------------------------------------------------------------------

    #[test]
    fn from_status_401_maps_to_auth() {
        let err = LolzteamError::from_status(401, "unauthorized".into(), None);
        assert!(matches!(err, LolzteamError::Auth { .. }));
    }

    #[test]
    fn from_status_403_maps_to_forbidden() {
        let err = LolzteamError::from_status(403, "forbidden".into(), None);
        assert!(matches!(err, LolzteamError::Forbidden { .. }));
    }

    #[test]
    fn from_status_404_maps_to_not_found() {
        let err = LolzteamError::from_status(404, "not found".into(), None);
        assert!(matches!(err, LolzteamError::NotFound { .. }));
    }

    #[test]
    fn from_status_429_maps_to_rate_limit() {
        let err = LolzteamError::from_status(429, "rate limited".into(), Some(5.0));
        match err {
            LolzteamError::RateLimit { body, retry_after } => {
                assert_eq!(body, "rate limited");
                assert_eq!(retry_after, Some(5.0));
            }
            _ => panic!("expected RateLimit variant"),
        }
    }

    #[test]
    fn from_status_500_maps_to_server() {
        let err = LolzteamError::from_status(500, "internal".into(), None);
        assert!(matches!(err, LolzteamError::Server { status: 500, .. }));
    }

    #[test]
    fn from_status_502_maps_to_server() {
        let err = LolzteamError::from_status(502, "bad gateway".into(), None);
        assert!(matches!(err, LolzteamError::Server { status: 502, .. }));
    }

    #[test]
    fn from_status_599_maps_to_server() {
        let err = LolzteamError::from_status(599, "custom 5xx".into(), None);
        assert!(matches!(err, LolzteamError::Server { status: 599, .. }));
    }

    #[test]
    fn from_status_400_maps_to_http() {
        let err = LolzteamError::from_status(400, "bad request".into(), None);
        assert!(matches!(
            err,
            LolzteamError::Http {
                status: 400,
                ..
            }
        ));
    }

    #[test]
    fn from_status_418_maps_to_http() {
        let err = LolzteamError::from_status(418, "teapot".into(), Some(1.0));
        match err {
            LolzteamError::Http {
                status,
                body,
                retry_after,
            } => {
                assert_eq!(status, 418);
                assert_eq!(body, "teapot");
                assert_eq!(retry_after, Some(1.0));
            }
            _ => panic!("expected Http variant"),
        }
    }

    #[test]
    fn from_status_200_range_maps_to_http() {
        // from_status is only called for error statuses in practice,
        // but it should still produce Http for < 400 codes
        let err = LolzteamError::from_status(302, "redirect".into(), None);
        assert!(matches!(
            err,
            LolzteamError::Http {
                status: 302,
                ..
            }
        ));
    }

    // -----------------------------------------------------------------------
    // Error is Send + Sync (compile-time check)
    // -----------------------------------------------------------------------

    #[test]
    fn error_is_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<LolzteamError>();
    }

    // -----------------------------------------------------------------------
    // Debug impl doesn't panic
    // -----------------------------------------------------------------------

    #[test]
    fn debug_impl_works_for_all_variants() {
        let variants: Vec<LolzteamError> = vec![
            LolzteamError::Http {
                status: 418,
                body: "teapot".into(),
                retry_after: Some(1.0),
            },
            LolzteamError::Auth {
                body: "bad".into(),
            },
            LolzteamError::Forbidden {
                body: "no".into(),
            },
            LolzteamError::NotFound {
                body: "gone".into(),
            },
            LolzteamError::RateLimit {
                body: "slow".into(),
                retry_after: None,
            },
            LolzteamError::Server {
                status: 500,
                body: "oops".into(),
            },
            LolzteamError::Config("bad config".into()),
            LolzteamError::RetryExhausted {
                attempts: 5,
                last_error: Box::new(LolzteamError::Config("inner".into())),
            },
        ];
        for err in &variants {
            let dbg = format!("{:?}", err);
            assert!(!dbg.is_empty());
        }
    }
}
