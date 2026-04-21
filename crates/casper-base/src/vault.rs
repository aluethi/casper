use aes_gcm::{
    Aes256Gcm, KeyInit, Nonce,
    aead::Aead,
};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use crate::{CasperError, SecretValue, TenantId};
use hkdf::Hkdf;
use sha2::Sha256;
use sqlx::PgPool;
use uuid::Uuid;

/// AES-256-GCM secret vault with HKDF-derived per-tenant keys.
#[derive(Clone)]
pub struct Vault {
    master_key: Vec<u8>,
}

impl Vault {
    pub fn new(master_key: Vec<u8>) -> Self {
        Self { master_key }
    }

    /// Derive a per-tenant encryption key using HKDF-SHA256.
    fn derive_key(&self, tenant_id: TenantId) -> [u8; 32] {
        let hk = Hkdf::<Sha256>::new(None, &self.master_key);
        let info = format!("casper-vault:{}", tenant_id.0);
        let mut key = [0u8; 32];
        hk.expand(info.as_bytes(), &mut key)
            .expect("HKDF expand failed");
        key
    }

    /// Encrypt a value for a specific tenant.
    fn encrypt(&self, tenant_id: TenantId, plaintext: &[u8]) -> Result<(String, String), CasperError> {
        let key = self.derive_key(tenant_id);
        let cipher = Aes256Gcm::new_from_slice(&key)
            .map_err(|e| CasperError::Internal(format!("cipher init failed: {e}")))?;

        let nonce_bytes: [u8; 12] = rand::random();
        let nonce = Nonce::from_slice(&nonce_bytes);

        let ciphertext = cipher
            .encrypt(nonce, plaintext)
            .map_err(|e| CasperError::Internal(format!("encryption failed: {e}")))?;

        Ok((BASE64.encode(&ciphertext), BASE64.encode(nonce_bytes)))
    }

    /// Decrypt a value for a specific tenant.
    fn decrypt(
        &self,
        tenant_id: TenantId,
        ciphertext_b64: &str,
        nonce_b64: &str,
    ) -> Result<SecretValue, CasperError> {
        let key = self.derive_key(tenant_id);
        let cipher = Aes256Gcm::new_from_slice(&key)
            .map_err(|e| CasperError::Internal(format!("cipher init failed: {e}")))?;

        let ciphertext = BASE64
            .decode(ciphertext_b64)
            .map_err(|e| CasperError::Internal(format!("base64 decode ciphertext: {e}")))?;
        let nonce_bytes = BASE64
            .decode(nonce_b64)
            .map_err(|e| CasperError::Internal(format!("base64 decode nonce: {e}")))?;
        let nonce = Nonce::from_slice(&nonce_bytes);

        let plaintext = cipher
            .decrypt(nonce, ciphertext.as_ref())
            .map_err(|_| CasperError::Internal("decryption failed".to_string()))?;

        Ok(SecretValue::new(plaintext))
    }

    /// Store (upsert) a secret for a tenant.
    pub async fn set(
        &self,
        pool: &PgPool,
        tenant_id: TenantId,
        key: &str,
        value: &[u8],
    ) -> Result<(), CasperError> {
        let (ciphertext_b64, nonce_b64) = self.encrypt(tenant_id, value)?;

        sqlx::query(
            "INSERT INTO tenant_secrets (id, tenant_id, key, ciphertext_b64, nonce_b64)
             VALUES ($1, $2, $3, $4, $5)
             ON CONFLICT (tenant_id, key) DO UPDATE
             SET ciphertext_b64 = EXCLUDED.ciphertext_b64,
                 nonce_b64 = EXCLUDED.nonce_b64,
                 updated_at = now()"
        )
        .bind(Uuid::now_v7())
        .bind(tenant_id.0)
        .bind(key)
        .bind(&ciphertext_b64)
        .bind(&nonce_b64)
        .execute(pool)
        .await
        .map_err(|e| CasperError::Internal(format!("DB write failed: {e}")))?;

        Ok(())
    }

    /// Retrieve a secret for a tenant.
    pub async fn get(
        &self,
        pool: &PgPool,
        tenant_id: TenantId,
        key: &str,
    ) -> Result<SecretValue, CasperError> {
        let row: (String, String) = sqlx::query_as(
            "SELECT ciphertext_b64, nonce_b64 FROM tenant_secrets
             WHERE tenant_id = $1 AND key = $2"
        )
        .bind(tenant_id.0)
        .bind(key)
        .fetch_optional(pool)
        .await
        .map_err(|e| CasperError::Internal(format!("DB read failed: {e}")))?
        .ok_or_else(|| CasperError::NotFound(format!("secret {key} not found")))?;

        self.decrypt(tenant_id, &row.0, &row.1)
    }

    /// Delete a secret.
    pub async fn delete(
        &self,
        pool: &PgPool,
        tenant_id: TenantId,
        key: &str,
    ) -> Result<bool, CasperError> {
        let result = sqlx::query(
            "DELETE FROM tenant_secrets WHERE tenant_id = $1 AND key = $2"
        )
        .bind(tenant_id.0)
        .bind(key)
        .execute(pool)
        .await
        .map_err(|e| CasperError::Internal(format!("DB delete failed: {e}")))?;

        Ok(result.rows_affected() > 0)
    }

    /// List secret keys (not values) for a tenant.
    pub async fn list_keys(
        &self,
        pool: &PgPool,
        tenant_id: TenantId,
    ) -> Result<Vec<String>, CasperError> {
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT key FROM tenant_secrets WHERE tenant_id = $1 ORDER BY key"
        )
        .bind(tenant_id.0)
        .fetch_all(pool)
        .await
        .map_err(|e| CasperError::Internal(format!("DB read failed: {e}")))?;

        Ok(rows.into_iter().map(|(k,)| k).collect())
    }

    /// Resolve MCP secrets: given a token_ref like "secret:key_name",
    /// fetch and decrypt the secret value.
    pub async fn resolve_mcp_secret(
        &self,
        pool: &PgPool,
        tenant_id: TenantId,
        token_ref: &str,
    ) -> Result<SecretValue, CasperError> {
        let key = token_ref
            .strip_prefix("secret:")
            .ok_or_else(|| CasperError::BadRequest(format!(
                "invalid token_ref format: {token_ref} (expected 'secret:key_name')"
            )))?;
        self.get(pool, tenant_id, key).await
    }

    /// Encrypt a value and return the base64-encoded ciphertext+nonce (for SSO client secrets, API keys, etc.)
    pub fn encrypt_value(
        &self,
        tenant_id: TenantId,
        plaintext: &[u8],
    ) -> Result<String, CasperError> {
        let (ciphertext_b64, nonce_b64) = self.encrypt(tenant_id, plaintext)?;
        Ok(format!("{nonce_b64}:{ciphertext_b64}"))
    }

    /// Decrypt a combined nonce:ciphertext value.
    pub fn decrypt_value(
        &self,
        tenant_id: TenantId,
        combined: &str,
    ) -> Result<SecretValue, CasperError> {
        let (nonce_b64, ciphertext_b64) = combined
            .split_once(':')
            .ok_or_else(|| CasperError::Internal("invalid encrypted value format".to_string()))?;
        self.decrypt(tenant_id, ciphertext_b64, nonce_b64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_vault() -> Vault {
        Vault::new(b"test-master-key-32-bytes-long!!!".to_vec())
    }

    #[test]
    fn encrypt_decrypt_roundtrip() {
        let vault = test_vault();
        let tenant = TenantId::new();
        let (ct, nonce) = vault.encrypt(tenant, b"hello world").unwrap();
        let decrypted = vault.decrypt(tenant, &ct, &nonce).unwrap();
        assert_eq!(decrypted.expose_str().unwrap(), "hello world");
    }

    #[test]
    fn different_tenants_different_keys() {
        let vault = test_vault();
        let t1 = TenantId::new();
        let t2 = TenantId::new();
        let (ct, nonce) = vault.encrypt(t1, b"secret").unwrap();
        assert!(vault.decrypt(t2, &ct, &nonce).is_err());
    }

    #[test]
    fn encrypt_value_roundtrip() {
        let vault = test_vault();
        let tenant = TenantId::new();
        let combined = vault.encrypt_value(tenant, b"my-api-key-123").unwrap();
        let decrypted = vault.decrypt_value(tenant, &combined).unwrap();
        assert_eq!(decrypted.expose_str().unwrap(), "my-api-key-123");
    }

    #[test]
    fn resolve_mcp_secret_validates_prefix() {
        assert!("invalid_ref".strip_prefix("secret:").is_none());
        assert_eq!("secret:my_key".strip_prefix("secret:"), Some("my_key"));
    }
}
