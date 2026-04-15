use serde::{Deserialize, Serialize};
use uuid::Uuid;
use zeroize::Zeroize;

// ============================================================
// Newtype IDs
// ============================================================

/// Tenant identifier (UUID v7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TenantId(pub Uuid);

impl TenantId {
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }
}

impl Default for TenantId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for TenantId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl std::str::FromStr for TenantId {
    type Err = uuid::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(Uuid::parse_str(s)?))
    }
}

/// Correlation identifier for request tracing (UUID v7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CorrelationId(pub Uuid);

impl CorrelationId {
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }
}

impl Default for CorrelationId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for CorrelationId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

// ============================================================
// Role
// ============================================================

/// User role with strict ordering: Viewer < Operator < Admin < Owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Viewer,
    Operator,
    Admin,
    Owner,
}

impl Role {
    fn rank(self) -> u8 {
        match self {
            Self::Viewer => 0,
            Self::Operator => 1,
            Self::Admin => 2,
            Self::Owner => 3,
        }
    }
}

impl PartialOrd for Role {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.rank().cmp(&other.rank()))
    }
}

impl Ord for Role {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.rank().cmp(&other.rank())
    }
}

impl std::fmt::Display for Role {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Viewer => write!(f, "viewer"),
            Self::Operator => write!(f, "operator"),
            Self::Admin => write!(f, "admin"),
            Self::Owner => write!(f, "owner"),
        }
    }
}

impl std::str::FromStr for Role {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "viewer" => Ok(Self::Viewer),
            "operator" => Ok(Self::Operator),
            "admin" => Ok(Self::Admin),
            "owner" => Ok(Self::Owner),
            other => Err(format!("unknown role: {other}")),
        }
    }
}

// ============================================================
// Subject
// ============================================================

/// Authentication subject — either a human user or an API key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Subject {
    User(String),
    ApiKey(String),
}

impl Subject {
    pub fn parse(s: &str) -> Result<Self, String> {
        if let Some(rest) = s.strip_prefix("user:") {
            Ok(Self::User(rest.to_string()))
        } else if let Some(rest) = s.strip_prefix("apikey:") {
            Ok(Self::ApiKey(rest.to_string()))
        } else {
            Err(format!("invalid subject format: {s}"))
        }
    }
}

impl std::fmt::Display for Subject {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::User(id) => write!(f, "user:{id}"),
            Self::ApiKey(id) => write!(f, "apikey:{id}"),
        }
    }
}

// ============================================================
// SecretValue
// ============================================================

/// A secret value that zeroizes on drop.
/// No Debug, Display, Serialize, or Clone — prevents accidental leaking.
pub struct SecretValue {
    inner: Vec<u8>,
}

impl SecretValue {
    pub fn new(value: Vec<u8>) -> Self {
        Self { inner: value }
    }

    pub fn from_str(s: &str) -> Self {
        Self {
            inner: s.as_bytes().to_vec(),
        }
    }

    pub fn expose(&self) -> &[u8] {
        &self.inner
    }

    pub fn expose_str(&self) -> Result<&str, std::str::Utf8Error> {
        std::str::from_utf8(&self.inner)
    }
}

impl Drop for SecretValue {
    fn drop(&mut self) {
        self.inner.zeroize();
    }
}

// ============================================================
// resolve_secret — reads from env or file
// ============================================================

/// Resolve a secret from environment variable or file.
/// If `{KEY}_FILE` env var exists, reads secret from that file path.
/// Otherwise, reads from `{KEY}` env var directly.
pub fn resolve_secret(key: &str) -> Result<SecretValue, String> {
    let file_key = format!("{key}_FILE");

    if let Ok(path) = std::env::var(&file_key) {
        let contents = std::fs::read_to_string(&path)
            .map_err(|e| format!("failed to read secret file {path}: {e}"))?;
        Ok(SecretValue::from_str(contents.trim()))
    } else if let Ok(value) = std::env::var(key) {
        Ok(SecretValue::from_str(&value))
    } else {
        Err(format!("secret {key} not found (checked {key} and {file_key})"))
    }
}

// ============================================================
// Tests
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn role_ordering() {
        assert!(Role::Viewer < Role::Operator);
        assert!(Role::Operator < Role::Admin);
        assert!(Role::Admin < Role::Owner);
        assert!(Role::Viewer < Role::Owner);
        assert!(!(Role::Admin < Role::Viewer));
        assert_eq!(Role::Admin, Role::Admin);
    }

    #[test]
    fn role_roundtrip() {
        for role in [Role::Viewer, Role::Operator, Role::Admin, Role::Owner] {
            let s = role.to_string();
            let parsed: Role = s.parse().unwrap();
            assert_eq!(role, parsed);
        }
    }

    #[test]
    fn subject_parsing() {
        let user = Subject::parse("user:alice@ventoo.ch").unwrap();
        assert_eq!(user, Subject::User("alice@ventoo.ch".to_string()));
        assert_eq!(user.to_string(), "user:alice@ventoo.ch");

        let key = Subject::parse("apikey:my-key").unwrap();
        assert_eq!(key, Subject::ApiKey("my-key".to_string()));
        assert_eq!(key.to_string(), "apikey:my-key");

        assert!(Subject::parse("invalid").is_err());
    }

    #[test]
    fn secret_value_basics() {
        let secret = SecretValue::from_str("hunter2");
        assert_eq!(secret.expose_str().unwrap(), "hunter2");
        assert_eq!(secret.expose(), b"hunter2");
    }

    #[test]
    fn resolve_secret_from_env() {
        // SAFETY: test runs single-threaded, no concurrent env access
        unsafe { std::env::set_var("TEST_CASPER_SECRET_1A", "my-secret") };
        let secret = resolve_secret("TEST_CASPER_SECRET_1A").unwrap();
        assert_eq!(secret.expose_str().unwrap(), "my-secret");
        unsafe { std::env::remove_var("TEST_CASPER_SECRET_1A") };
    }

    #[test]
    fn resolve_secret_missing() {
        assert!(resolve_secret("DEFINITELY_NONEXISTENT_SECRET_XYZ").is_err());
    }

    #[test]
    fn tenant_id_display() {
        let id = TenantId::new();
        let s = id.to_string();
        let parsed: TenantId = s.parse().unwrap();
        assert_eq!(id, parsed);
    }
}
