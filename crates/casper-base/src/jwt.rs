use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::context::TenantContext;
use crate::error::CasperError;
use crate::scope::Scope;
use crate::types::{CorrelationId, Role, Subject, TenantId};

/// JWT claims structure for Casper tokens.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CasperClaims {
    /// Subject: "user:alice@ventoo.ch"
    pub sub: String,
    /// Tenant ID
    pub tid: Uuid,
    /// Role
    pub role: String,
    /// Scopes
    pub scopes: Vec<String>,
    /// Expiration (Unix timestamp)
    pub exp: i64,
    /// Issued at (Unix timestamp)
    pub iat: i64,
    /// Issuer
    pub iss: String,
    /// JWT ID (for revocation)
    pub jti: String,
}

/// Trait for checking if a token has been revoked.
pub trait RevocationCheck: Send + Sync {
    fn is_revoked(&self, jti: &str) -> bool;
}

/// Verifies Casper JWTs using Ed25519.
/// Stores the public key in DER format for use with jsonwebtoken (which uses ring).
pub struct JwtVerifier {
    /// Ed25519 public key in DER format
    public_key_der: Vec<u8>,
}

impl JwtVerifier {
    /// Create from raw Ed25519 public key bytes (32 bytes).
    /// jsonwebtoken's from_ed_der for decoding expects raw public key bytes
    /// (ring's UnparsedPublicKey takes raw bytes for Ed25519).
    pub fn from_public_key(public_key_bytes: &[u8; 32]) -> Result<Self, CasperError> {
        Ok(Self {
            public_key_der: public_key_bytes.to_vec(),
        })
    }

    /// Create from PKCS8 DER bytes (for direct use with ring-generated keys).
    pub fn from_pkcs8_der(pkcs8_der: &[u8]) -> Result<Self, CasperError> {
        // Extract public key from PKCS8 — the last 32 bytes of an Ed25519 PKCS8 key
        // Actually for decoding we need the public key in SubjectPublicKeyInfo format
        // For ring Ed25519, the PKCS8 is 83 bytes and contains both private+public
        // The public key is the last 32 bytes
        if pkcs8_der.len() < 32 {
            return Err(CasperError::Internal("PKCS8 DER too short".into()));
        }
        let pub_bytes: [u8; 32] = pkcs8_der[pkcs8_der.len() - 32..]
            .try_into()
            .map_err(|_| CasperError::Internal("invalid key length".into()))?;
        Self::from_public_key(&pub_bytes)
    }

    /// Verify a JWT token string and return the claims.
    pub fn verify(&self, token: &str) -> Result<CasperClaims, CasperError> {
        use jsonwebtoken::{Algorithm, DecodingKey, Validation};

        let decoding_key = DecodingKey::from_ed_der(&self.public_key_der);
        let mut validation = Validation::new(Algorithm::EdDSA);
        validation.set_issuer(&["casper"]);
        validation.set_required_spec_claims(&["exp", "iss", "sub"]);

        let token_data = jsonwebtoken::decode::<CasperClaims>(token, &decoding_key, &validation)
            .map_err(|_| CasperError::Unauthorized)?;

        Ok(token_data.claims)
    }

    /// Full authentication: verify token, check revocation, build TenantContext.
    pub fn authenticate(
        &self,
        token: &str,
        revocation: &dyn RevocationCheck,
    ) -> Result<TenantContext, CasperError> {
        let claims = self.verify(token)?;

        if revocation.is_revoked(&claims.jti) {
            return Err(CasperError::Unauthorized);
        }

        let subject = Subject::parse(&claims.sub)
            .map_err(|e| CasperError::BadRequest(format!("invalid subject: {e}")))?;

        let role: Role = claims
            .role
            .parse()
            .map_err(|e: String| CasperError::BadRequest(format!("invalid role: {e}")))?;

        let scopes: Vec<Scope> = claims
            .scopes
            .iter()
            .filter_map(|s| Scope::parse(s).ok())
            .collect();

        Ok(TenantContext {
            tenant_id: TenantId(claims.tid),
            subject,
            role,
            scopes,
            token_id: claims.jti,
            correlation_id: CorrelationId::new(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jsonwebtoken::{Algorithm, EncodingKey, Header};
    use ring::signature::KeyPair;

    struct NoRevocation;
    impl RevocationCheck for NoRevocation {
        fn is_revoked(&self, _jti: &str) -> bool {
            false
        }
    }

    struct RevokeAll;
    impl RevocationCheck for RevokeAll {
        fn is_revoked(&self, _jti: &str) -> bool {
            true
        }
    }

    /// Generate an Ed25519 keypair using ring, returning (pkcs8_der, encoding_key, verifier).
    fn make_keypair() -> (Vec<u8>, EncodingKey, JwtVerifier) {
        let rng = ring::rand::SystemRandom::new();
        let pkcs8 = ring::signature::Ed25519KeyPair::generate_pkcs8(&rng).unwrap();
        let pkcs8_bytes = pkcs8.as_ref().to_vec();

        // Extract public key via ring's KeyPair API
        let key_pair =
            ring::signature::Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).unwrap();
        let pub_bytes: [u8; 32] = key_pair
            .public_key()
            .as_ref()
            .try_into()
            .unwrap();

        let encoding_key = EncodingKey::from_ed_der(&pkcs8_bytes);
        let verifier = JwtVerifier::from_public_key(&pub_bytes).unwrap();

        (pkcs8_bytes, encoding_key, verifier)
    }

    fn sign_token(encoding_key: &EncodingKey, claims: &CasperClaims) -> String {
        let header = Header::new(Algorithm::EdDSA);
        jsonwebtoken::encode(&header, claims, encoding_key).unwrap()
    }

    fn make_claims() -> CasperClaims {
        let now = time::OffsetDateTime::now_utc().unix_timestamp();
        CasperClaims {
            sub: "user:alice@ventoo.ch".to_string(),
            tid: Uuid::nil(),
            role: "admin".to_string(),
            scopes: vec!["admin:*".to_string()],
            exp: now + 900,
            iat: now,
            iss: "casper".to_string(),
            jti: "test-jti-123".to_string(),
        }
    }

    #[test]
    fn roundtrip_sign_verify() {
        let (_, encoding_key, verifier) = make_keypair();
        let claims = make_claims();
        let token = sign_token(&encoding_key, &claims);

        let decoded = verifier.verify(&token).unwrap();
        assert_eq!(decoded.sub, "user:alice@ventoo.ch");
        assert_eq!(decoded.jti, "test-jti-123");
    }

    #[test]
    fn expired_rejected() {
        let (_, encoding_key, verifier) = make_keypair();
        let mut claims = make_claims();
        claims.exp = time::OffsetDateTime::now_utc().unix_timestamp() - 100;

        let token = sign_token(&encoding_key, &claims);
        assert!(matches!(verifier.verify(&token), Err(CasperError::Unauthorized)));
    }

    #[test]
    fn bad_signature_rejected() {
        let (_, encoding_key, _) = make_keypair();
        let (_, _, other_verifier) = make_keypair();
        let claims = make_claims();
        let token = sign_token(&encoding_key, &claims);

        assert!(matches!(
            other_verifier.verify(&token),
            Err(CasperError::Unauthorized)
        ));
    }

    #[test]
    fn authenticate_builds_context() {
        let (_, encoding_key, verifier) = make_keypair();
        let claims = make_claims();
        let token = sign_token(&encoding_key, &claims);

        let ctx = verifier.authenticate(&token, &NoRevocation).unwrap();
        assert_eq!(ctx.tenant_id.0, Uuid::nil());
        assert_eq!(ctx.subject, Subject::User("alice@ventoo.ch".to_string()));
        assert_eq!(ctx.role, Role::Admin);
        assert_eq!(ctx.token_id, "test-jti-123");
        assert!(ctx.require_scope("agents:triage:run").is_ok());
    }

    #[test]
    fn revoked_token_rejected() {
        let (_, encoding_key, verifier) = make_keypair();
        let claims = make_claims();
        let token = sign_token(&encoding_key, &claims);

        assert!(matches!(
            verifier.authenticate(&token, &RevokeAll),
            Err(CasperError::Unauthorized)
        ));
    }
}
