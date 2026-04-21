use crate::{CasperClaims, CasperError, TenantId};
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use uuid::Uuid;

/// Signs Casper JWTs using an Ed25519 private key (PKCS8 DER format).
pub struct JwtSigner {
    encoding_key: EncodingKey,
}

impl JwtSigner {
    /// Create from PKCS8 DER bytes (Ed25519 private key).
    pub fn from_pkcs8_der(pkcs8_der: &[u8]) -> Result<Self, CasperError> {
        let encoding_key = EncodingKey::from_ed_der(pkcs8_der);
        Ok(Self { encoding_key })
    }

    /// Sign a set of claims into a JWT string.
    pub fn sign(&self, claims: &CasperClaims) -> Result<String, CasperError> {
        let header = Header::new(Algorithm::EdDSA);
        jsonwebtoken::encode(&header, claims, &self.encoding_key)
            .map_err(|e| CasperError::Internal(format!("JWT signing failed: {e}")))
    }

    /// Build claims and sign an access token (15 min).
    pub fn sign_access_token(
        &self,
        tenant_id: TenantId,
        subject: &str,
        role: &str,
        scopes: Vec<String>,
    ) -> Result<(String, String), CasperError> {
        let now = time::OffsetDateTime::now_utc().unix_timestamp();
        let jti = Uuid::now_v7().to_string();
        let claims = CasperClaims {
            sub: subject.to_string(),
            tid: tenant_id.0,
            role: role.to_string(),
            scopes,
            exp: now + 900, // 15 minutes
            iat: now,
            iss: "casper".to_string(),
            jti: jti.clone(),
        };
        let token = self.sign(&claims)?;
        Ok((token, jti))
    }

    /// Build claims and sign a refresh token (7 days).
    pub fn sign_refresh_token(
        &self,
        tenant_id: TenantId,
        subject: &str,
    ) -> Result<(String, String), CasperError> {
        let now = time::OffsetDateTime::now_utc().unix_timestamp();
        let jti = Uuid::now_v7().to_string();
        let claims = CasperClaims {
            sub: subject.to_string(),
            tid: tenant_id.0,
            role: "refresh".to_string(),
            scopes: vec![],
            exp: now + 604800, // 7 days
            iat: now,
            iss: "casper".to_string(),
            jti: jti.clone(),
        };
        let token = self.sign(&claims)?;
        Ok((token, jti))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::JwtVerifier;
    use ring::signature::KeyPair;

    fn make_signer_and_verifier() -> (JwtSigner, JwtVerifier) {
        let rng = ring::rand::SystemRandom::new();
        let pkcs8 = ring::signature::Ed25519KeyPair::generate_pkcs8(&rng).unwrap();
        let pkcs8_bytes = pkcs8.as_ref();

        let key_pair = ring::signature::Ed25519KeyPair::from_pkcs8(pkcs8_bytes).unwrap();
        let pub_bytes: [u8; 32] = key_pair.public_key().as_ref().try_into().unwrap();

        let signer = JwtSigner::from_pkcs8_der(pkcs8_bytes).unwrap();
        let verifier = JwtVerifier::from_public_key(&pub_bytes).unwrap();
        (signer, verifier)
    }

    #[test]
    fn sign_and_verify() {
        let (signer, verifier) = make_signer_and_verifier();
        let tid = TenantId::new();
        let (token, jti) = signer
            .sign_access_token(tid, "user:test@test.com", "admin", vec!["admin:*".to_string()])
            .unwrap();

        let claims = verifier.verify(&token).unwrap();
        assert_eq!(claims.sub, "user:test@test.com");
        assert_eq!(claims.tid, tid.0);
        assert_eq!(claims.role, "admin");
        assert_eq!(claims.jti, jti);
    }

    #[test]
    fn refresh_token() {
        let (signer, verifier) = make_signer_and_verifier();
        let tid = TenantId::new();
        let (token, _jti) = signer.sign_refresh_token(tid, "user:test@test.com").unwrap();

        let claims = verifier.verify(&token).unwrap();
        assert_eq!(claims.role, "refresh");
        assert!(claims.scopes.is_empty());
        assert!(claims.exp - claims.iat >= 604800);
    }
}
