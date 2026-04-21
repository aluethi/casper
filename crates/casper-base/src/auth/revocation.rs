use crate::RevocationCheck;
use sqlx::PgPool;
use std::collections::HashSet;
use std::sync::{Arc, RwLock};
use tokio::time::{Duration, interval};

/// In-memory cache of revoked JTIs, refreshed periodically from the database.
#[derive(Clone)]
pub struct RevocationCache {
    revoked: Arc<RwLock<HashSet<String>>>,
}

impl RevocationCache {
    pub fn new() -> Self {
        Self {
            revoked: Arc::new(RwLock::new(HashSet::new())),
        }
    }

    /// Add a JTI to the revocation cache.
    pub fn revoke(&self, jti: &str) {
        let mut set = self.revoked.write().unwrap_or_else(|e| e.into_inner());
        set.insert(jti.to_string());
    }

    /// Start a background task that refreshes the cache from the database.
    pub fn start_refresh(
        &self,
        pool: PgPool,
        refresh_interval: Duration,
        cancel: tokio_util::sync::CancellationToken,
    ) -> tokio::task::JoinHandle<()> {
        let cache = self.clone();
        tokio::spawn(async move {
            let mut ticker = interval(refresh_interval);
            loop {
                tokio::select! {
                    _ = cancel.cancelled() => {
                        tracing::info!("Revocation cache refresh shutting down");
                        break;
                    }
                    _ = ticker.tick() => {
                        if let Err(e) = cache.refresh_from_db(&pool).await {
                            tracing::warn!("Failed to refresh revocation cache: {e}");
                        }
                    }
                }
            }
        })
    }

    async fn refresh_from_db(&self, pool: &PgPool) -> Result<(), sqlx::Error> {
        let rows: Vec<(String,)> = sqlx::query_as("SELECT jti FROM token_revocations")
            .fetch_all(pool)
            .await?;

        let mut set = self.revoked.write().unwrap_or_else(|e| e.into_inner());
        set.clear();
        for (jti,) in rows {
            set.insert(jti);
        }

        tracing::debug!("Revocation cache refreshed: {} entries", set.len());
        Ok(())
    }
}

impl Default for RevocationCache {
    fn default() -> Self {
        Self::new()
    }
}

impl RevocationCheck for RevocationCache {
    fn is_revoked(&self, jti: &str) -> bool {
        let set = self.revoked.read().unwrap_or_else(|e| e.into_inner());
        set.contains(jti)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RevocationCheck;

    #[test]
    fn revocation_cache() {
        let cache = RevocationCache::new();
        assert!(!cache.is_revoked("jti-1"));

        cache.revoke("jti-1");
        assert!(cache.is_revoked("jti-1"));
        assert!(!cache.is_revoked("jti-2"));
    }

    #[test]
    fn sign_verify_with_revocation() {
        let rng = ring::rand::SystemRandom::new();
        let pkcs8 = ring::signature::Ed25519KeyPair::generate_pkcs8(&rng).unwrap();
        let pkcs8_bytes = pkcs8.as_ref();
        let key_pair = ring::signature::Ed25519KeyPair::from_pkcs8(pkcs8_bytes).unwrap();
        let pub_bytes: [u8; 32] = {
            use ring::signature::KeyPair;
            key_pair.public_key().as_ref().try_into().unwrap()
        };

        let signer = super::super::JwtSigner::from_pkcs8_der(pkcs8_bytes).unwrap();
        let verifier = crate::JwtVerifier::from_public_key(&pub_bytes).unwrap();
        let cache = RevocationCache::new();

        let tid = crate::TenantId::new();
        let (token, jti) = signer
            .sign_access_token(
                tid,
                "user:test@test.com",
                "admin",
                vec!["admin:*".to_string()],
            )
            .unwrap();

        let ctx = verifier.authenticate(&token, &cache).unwrap();
        assert_eq!(ctx.token_id, jti);

        cache.revoke(&jti);
        assert!(verifier.authenticate(&token, &cache).is_err());
    }
}
