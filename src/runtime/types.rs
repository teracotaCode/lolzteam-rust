//! Shared types used throughout the client.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::HashMap;
use std::fmt;
use std::time::Duration;

// ---------------------------------------------------------------------------
// StringOrInt — handles JSON fields that may be string or integer
// ---------------------------------------------------------------------------

/// A value that can be either a string or an integer in JSON.
///
/// Lolzteam APIs sometimes return numeric IDs as strings and sometimes as
/// integers depending on the endpoint. This type transparently handles both.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum StringOrInt {
    String(String),
    Int(i64),
}

impl StringOrInt {
    /// Return the value as a string regardless of the variant.
    pub fn as_str(&self) -> String {
        match self {
            StringOrInt::String(s) => s.clone(),
            StringOrInt::Int(i) => i.to_string(),
        }
    }

    /// Try to return the value as an i64.
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            StringOrInt::Int(i) => Some(*i),
            StringOrInt::String(s) => s.parse().ok(),
        }
    }
}

impl fmt::Display for StringOrInt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StringOrInt::String(s) => write!(f, "{}", s),
            StringOrInt::Int(i) => write!(f, "{}", i),
        }
    }
}

impl From<String> for StringOrInt {
    fn from(s: String) -> Self {
        StringOrInt::String(s)
    }
}

impl From<&str> for StringOrInt {
    fn from(s: &str) -> Self {
        StringOrInt::String(s.to_string())
    }
}

impl From<i64> for StringOrInt {
    fn from(i: i64) -> Self {
        StringOrInt::Int(i)
    }
}

impl Serialize for StringOrInt {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            StringOrInt::String(s) => serializer.serialize_str(s),
            StringOrInt::Int(i) => serializer.serialize_i64(*i),
        }
    }
}

impl<'de> Deserialize<'de> for StringOrInt {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = StringOrInt;

            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                write!(f, "a string or integer")
            }

            fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<StringOrInt, E> {
                Ok(StringOrInt::String(v.to_string()))
            }

            fn visit_string<E: serde::de::Error>(self, v: String) -> Result<StringOrInt, E> {
                Ok(StringOrInt::String(v))
            }

            fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<StringOrInt, E> {
                Ok(StringOrInt::Int(v))
            }

            fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<StringOrInt, E> {
                Ok(StringOrInt::Int(v as i64))
            }
        }
        deserializer.deserialize_any(Visitor)
    }
}

// ---------------------------------------------------------------------------
// RequestOptions — parameters for a single HTTP request
// ---------------------------------------------------------------------------

/// Options passed to [`HttpClient::request`](super::http_client::HttpClient::request).
#[derive(Debug, Default, Clone)]
pub struct RequestOptions {
    /// Query-string parameters.
    pub query: Option<Vec<(String, String)>>,
    /// JSON body (mutually exclusive with `form` and `multipart`).
    pub json: Option<serde_json::Value>,
    /// URL-encoded form body.
    pub form: Option<Vec<(String, String)>>,
    /// Multipart file uploads.
    pub files: Option<Vec<FileUpload>>,
    /// Text fields to include alongside file uploads in a multipart form.
    pub multipart_fields: Option<Vec<(String, String)>>,
    /// Whether this is a search endpoint (stricter rate limit).
    pub is_search: bool,
}

// ---------------------------------------------------------------------------
// FileUpload — a file to upload via multipart
// ---------------------------------------------------------------------------

/// Describes a file to include in a multipart upload.
#[derive(Debug, Clone)]
pub struct FileUpload {
    /// The multipart field name.
    pub field_name: String,
    /// The filename to report to the server.
    pub file_name: String,
    /// The MIME type.
    pub mime_type: String,
    /// Raw file bytes.
    pub data: Vec<u8>,
}

// ---------------------------------------------------------------------------
// ClientConfig — configuration for building an HttpClient
// ---------------------------------------------------------------------------

/// Configuration for constructing a [`ForumClient`](crate::ForumClient) or
/// [`MarketClient`](crate::MarketClient).
#[derive(Debug, Clone)]
pub struct ClientConfig {
    /// Bearer token for authentication.
    pub token: String,
    /// Base URL, e.g. `https://prod-api.lzt.market`.
    pub base_url: String,
    /// Optional HTTP / SOCKS5 proxy.
    pub proxy: Option<ProxyConfig>,
    /// Request timeout.
    pub timeout: Duration,
    /// Maximum number of retries for transient failures.
    pub max_retries: u32,
    /// User-Agent header value.
    pub user_agent: String,
    /// Requests-per-second limit (general endpoints).
    pub rps_general: f64,
    /// Requests-per-second limit (search endpoints).
    pub rps_search: f64,
    /// Extra default headers.
    pub extra_headers: HashMap<String, String>,
}

impl ClientConfig {
    /// Create a config with sensible defaults for the Lolzteam **Forum** API.
    pub fn forum(token: impl Into<String>) -> Self {
        Self {
            token: token.into(),
            base_url: "https://prod-api.lolz.live".to_string(),
            proxy: None,
            timeout: Duration::from_secs(60),
            max_retries: 3,
            user_agent: format!("lolzteam-rs/{}", env!("CARGO_PKG_VERSION")),
            rps_general: 3.0,
            rps_search: 1.0,
            extra_headers: HashMap::new(),
        }
    }

    /// Create a config with sensible defaults for the Lolzteam **Market** API.
    pub fn market(token: impl Into<String>) -> Self {
        Self {
            token: token.into(),
            base_url: "https://prod-api.lzt.market".to_string(),
            proxy: None,
            timeout: Duration::from_secs(60),
            max_retries: 3,
            user_agent: format!("lolzteam-rs/{}", env!("CARGO_PKG_VERSION")),
            rps_general: 3.0,
            rps_search: 1.0,
            extra_headers: HashMap::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// ProxyConfig
// ---------------------------------------------------------------------------

/// Proxy configuration.
#[derive(Debug, Clone)]
pub struct ProxyConfig {
    /// Full proxy URL, e.g. `socks5://user:pass@host:port`.
    pub url: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // StringOrInt deserialization
    // -----------------------------------------------------------------------

    #[test]
    fn deserialize_string_from_json_string() {
        let v: StringOrInt = serde_json::from_str(r#""hello""#).unwrap();
        assert_eq!(v, StringOrInt::String("hello".into()));
    }

    #[test]
    fn deserialize_int_from_json_int() {
        let v: StringOrInt = serde_json::from_str("42").unwrap();
        assert_eq!(v, StringOrInt::Int(42));
    }

    #[test]
    fn deserialize_negative_int() {
        let v: StringOrInt = serde_json::from_str("-7").unwrap();
        assert_eq!(v, StringOrInt::Int(-7));
    }

    #[test]
    fn deserialize_zero() {
        let v: StringOrInt = serde_json::from_str("0").unwrap();
        assert_eq!(v, StringOrInt::Int(0));
    }

    #[test]
    fn deserialize_large_u64() {
        // u64 values that fit in i64 should work
        let v: StringOrInt = serde_json::from_str("9223372036854775807").unwrap();
        assert_eq!(v, StringOrInt::Int(i64::MAX));
    }

    #[test]
    fn deserialize_empty_string() {
        let v: StringOrInt = serde_json::from_str(r#""""#).unwrap();
        assert_eq!(v, StringOrInt::String(String::new()));
    }

    #[test]
    fn deserialize_numeric_string() {
        // A string that looks like a number stays a string
        let v: StringOrInt = serde_json::from_str(r#""123""#).unwrap();
        assert_eq!(v, StringOrInt::String("123".into()));
    }

    #[test]
    fn deserialize_null_fails() {
        let result = serde_json::from_str::<StringOrInt>("null");
        assert!(result.is_err());
    }

    #[test]
    fn deserialize_bool_fails() {
        let result = serde_json::from_str::<StringOrInt>("true");
        assert!(result.is_err());
    }

    #[test]
    fn deserialize_object_fails() {
        let result = serde_json::from_str::<StringOrInt>("{}");
        assert!(result.is_err());
    }

    #[test]
    fn deserialize_array_fails() {
        let result = serde_json::from_str::<StringOrInt>("[]");
        assert!(result.is_err());
    }

    // -----------------------------------------------------------------------
    // StringOrInt in Option<StringOrInt> (JSON null → None)
    // -----------------------------------------------------------------------

    #[test]
    fn deserialize_option_string_or_int_null() {
        let v: Option<StringOrInt> = serde_json::from_str("null").unwrap();
        assert!(v.is_none());
    }

    #[test]
    fn deserialize_option_string_or_int_present() {
        let v: Option<StringOrInt> = serde_json::from_str("42").unwrap();
        assert_eq!(v, Some(StringOrInt::Int(42)));
    }

    // -----------------------------------------------------------------------
    // StringOrInt serialization
    // -----------------------------------------------------------------------

    #[test]
    fn serialize_string_variant() {
        let v = StringOrInt::String("hello".into());
        assert_eq!(serde_json::to_string(&v).unwrap(), r#""hello""#);
    }

    #[test]
    fn serialize_int_variant() {
        let v = StringOrInt::Int(42);
        assert_eq!(serde_json::to_string(&v).unwrap(), "42");
    }

    #[test]
    fn serialize_negative_int_variant() {
        let v = StringOrInt::Int(-100);
        assert_eq!(serde_json::to_string(&v).unwrap(), "-100");
    }

    #[test]
    fn serialize_empty_string_variant() {
        let v = StringOrInt::String(String::new());
        assert_eq!(serde_json::to_string(&v).unwrap(), r#""""#);
    }

    // -----------------------------------------------------------------------
    // Round-trip serialization
    // -----------------------------------------------------------------------

    #[test]
    fn roundtrip_string() {
        let original = StringOrInt::String("test-value".into());
        let json = serde_json::to_string(&original).unwrap();
        let parsed: StringOrInt = serde_json::from_str(&json).unwrap();
        assert_eq!(original, parsed);
    }

    #[test]
    fn roundtrip_int() {
        let original = StringOrInt::Int(999);
        let json = serde_json::to_string(&original).unwrap();
        let parsed: StringOrInt = serde_json::from_str(&json).unwrap();
        assert_eq!(original, parsed);
    }

    // -----------------------------------------------------------------------
    // as_str() and as_i64() helpers
    // -----------------------------------------------------------------------

    #[test]
    fn as_str_from_string() {
        let v = StringOrInt::String("hello".into());
        assert_eq!(v.as_str(), "hello");
    }

    #[test]
    fn as_str_from_int() {
        let v = StringOrInt::Int(42);
        assert_eq!(v.as_str(), "42");
    }

    #[test]
    fn as_i64_from_int() {
        let v = StringOrInt::Int(42);
        assert_eq!(v.as_i64(), Some(42));
    }

    #[test]
    fn as_i64_from_numeric_string() {
        let v = StringOrInt::String("123".into());
        assert_eq!(v.as_i64(), Some(123));
    }

    #[test]
    fn as_i64_from_non_numeric_string() {
        let v = StringOrInt::String("hello".into());
        assert_eq!(v.as_i64(), None);
    }

    #[test]
    fn as_i64_from_empty_string() {
        let v = StringOrInt::String(String::new());
        assert_eq!(v.as_i64(), None);
    }

    // -----------------------------------------------------------------------
    // Display impl
    // -----------------------------------------------------------------------

    #[test]
    fn display_string_variant() {
        let v = StringOrInt::String("world".into());
        assert_eq!(format!("{}", v), "world");
    }

    #[test]
    fn display_int_variant() {
        let v = StringOrInt::Int(77);
        assert_eq!(format!("{}", v), "77");
    }

    // -----------------------------------------------------------------------
    // From impls
    // -----------------------------------------------------------------------

    #[test]
    fn from_string() {
        let v: StringOrInt = String::from("test").into();
        assert_eq!(v, StringOrInt::String("test".into()));
    }

    #[test]
    fn from_str_ref() {
        let v: StringOrInt = "test".into();
        assert_eq!(v, StringOrInt::String("test".into()));
    }

    #[test]
    fn from_i64() {
        let v: StringOrInt = 42i64.into();
        assert_eq!(v, StringOrInt::Int(42));
    }

    // -----------------------------------------------------------------------
    // Hash + Eq (used as HashMap key)
    // -----------------------------------------------------------------------

    #[test]
    fn can_be_used_as_hashmap_key() {
        let mut map = HashMap::new();
        map.insert(StringOrInt::Int(1), "one");
        map.insert(StringOrInt::String("two".into()), "two");
        assert_eq!(map.get(&StringOrInt::Int(1)), Some(&"one"));
        assert_eq!(map.get(&StringOrInt::String("two".into())), Some(&"two"));
    }

    // -----------------------------------------------------------------------
    // Clone
    // -----------------------------------------------------------------------

    #[test]
    fn clone_preserves_value() {
        let v = StringOrInt::Int(42);
        let cloned = v.clone();
        assert_eq!(v, cloned);
    }

    // -----------------------------------------------------------------------
    // StringOrInt in a struct with serde(default)
    // -----------------------------------------------------------------------

    #[test]
    fn option_string_or_int_in_struct() {
        #[derive(Debug, Deserialize, Default)]
        #[serde(default)]
        struct TestStruct {
            #[serde(default)]
            user_id: Option<StringOrInt>,
            #[serde(default)]
            name: Option<String>,
        }

        // From int
        let v: TestStruct = serde_json::from_str(r#"{"user_id": 42}"#).unwrap();
        assert_eq!(v.user_id, Some(StringOrInt::Int(42)));
        assert_eq!(v.name, None);

        // From string
        let v: TestStruct = serde_json::from_str(r#"{"user_id": "abc"}"#).unwrap();
        assert_eq!(v.user_id, Some(StringOrInt::String("abc".into())));

        // From null
        let v: TestStruct = serde_json::from_str(r#"{"user_id": null}"#).unwrap();
        assert_eq!(v.user_id, None);

        // Missing field
        let v: TestStruct = serde_json::from_str(r#"{}"#).unwrap();
        assert_eq!(v.user_id, None);
    }

    // -----------------------------------------------------------------------
    // ClientConfig defaults
    // -----------------------------------------------------------------------

    #[test]
    fn forum_config_defaults() {
        let cfg = ClientConfig::forum("my-token");
        assert_eq!(cfg.token, "my-token");
        assert_eq!(cfg.base_url, "https://prod-api.lolz.live");
        assert_eq!(cfg.max_retries, 3);
        assert_eq!(cfg.timeout, Duration::from_secs(60));
        assert!(cfg.proxy.is_none());
        assert!(cfg.extra_headers.is_empty());
        assert!(cfg.user_agent.starts_with("lolzteam-rs/"));
        assert_eq!(cfg.rps_general, 3.0);
        assert_eq!(cfg.rps_search, 1.0);
    }

    #[test]
    fn market_config_defaults() {
        let cfg = ClientConfig::market("my-token");
        assert_eq!(cfg.token, "my-token");
        assert_eq!(cfg.base_url, "https://prod-api.lzt.market");
        assert_eq!(cfg.max_retries, 3);
        assert_eq!(cfg.timeout, Duration::from_secs(60));
        assert!(cfg.proxy.is_none());
        assert!(cfg.extra_headers.is_empty());
        assert!(cfg.user_agent.starts_with("lolzteam-rs/"));
        assert_eq!(cfg.rps_general, 3.0);
        assert_eq!(cfg.rps_search, 1.0);
    }

    #[test]
    fn config_accepts_string_reference() {
        let token = String::from("dynamic-token");
        let cfg = ClientConfig::forum(&token);
        assert_eq!(cfg.token, "dynamic-token");
    }

    // -----------------------------------------------------------------------
    // RequestOptions defaults
    // -----------------------------------------------------------------------

    #[test]
    fn request_options_default() {
        let opts = RequestOptions::default();
        assert!(opts.query.is_none());
        assert!(opts.json.is_none());
        assert!(opts.form.is_none());
        assert!(opts.files.is_none());
        assert!(!opts.is_search);
    }

    // -----------------------------------------------------------------------
    // FileUpload construction
    // -----------------------------------------------------------------------

    #[test]
    fn file_upload_holds_data() {
        let upload = FileUpload {
            field_name: "avatar".into(),
            file_name: "photo.png".into(),
            mime_type: "image/png".into(),
            data: vec![0x89, 0x50, 0x4E, 0x47],
        };
        assert_eq!(upload.field_name, "avatar");
        assert_eq!(upload.file_name, "photo.png");
        assert_eq!(upload.mime_type, "image/png");
        assert_eq!(upload.data.len(), 4);
    }
}
