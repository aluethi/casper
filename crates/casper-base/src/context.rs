use crate::scope::{Scope, has_scope};
use crate::{CasperError, CorrelationId, Role, Subject, TenantId};

/// Authenticated request context. Built by auth middleware, passed via Axum extensions.
#[derive(Debug, Clone)]
pub struct TenantContext {
    pub tenant_id: TenantId,
    pub subject: Subject,
    pub role: Role,
    pub scopes: Vec<Scope>,
    pub token_id: String,
    pub correlation_id: CorrelationId,
}

impl TenantContext {
    /// Check that the caller has a specific scope.
    pub fn require_scope(&self, requested: &str) -> Result<(), CasperError> {
        let req = Scope::parse(requested)
            .map_err(|e| CasperError::Internal(format!("invalid scope: {e}")))?;
        if has_scope(&self.scopes, &req) {
            Ok(())
        } else {
            Err(CasperError::Forbidden(format!(
                "Token lacks scope {requested}"
            )))
        }
    }

    /// Check that the caller has at least the given role.
    pub fn require_role(&self, minimum: Role) -> Result<(), CasperError> {
        if self.role >= minimum {
            Ok(())
        } else {
            Err(CasperError::Forbidden(format!(
                "Role {} insufficient, need at least {}",
                self.role, minimum
            )))
        }
    }

    /// Returns the subject as a display string (for audit/logging).
    pub fn actor(&self) -> String {
        self.subject.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn make_ctx(scopes: &[&str], role: Role) -> TenantContext {
        TenantContext {
            tenant_id: TenantId(Uuid::nil()),
            subject: Subject::User("test@test.com".to_string()),
            role,
            scopes: scopes.iter().map(|s| Scope::parse(s).unwrap()).collect(),
            token_id: "jti-123".to_string(),
            correlation_id: CorrelationId::new(),
        }
    }

    #[test]
    fn require_scope_passes() {
        let ctx = make_ctx(&["agents:run", "inference:call"], Role::Operator);
        assert!(ctx.require_scope("agents:triage:run").is_ok());
        assert!(ctx.require_scope("inference:sonnet-fast:call").is_ok());
    }

    #[test]
    fn require_scope_fails() {
        let ctx = make_ctx(&["agents:triage:run"], Role::Operator);
        let err = ctx.require_scope("agents:devops:run").unwrap_err();
        assert!(matches!(err, CasperError::Forbidden(_)));
    }

    #[test]
    fn require_role_passes() {
        let ctx = make_ctx(&["admin:*"], Role::Admin);
        assert!(ctx.require_role(Role::Operator).is_ok());
        assert!(ctx.require_role(Role::Admin).is_ok());
    }

    #[test]
    fn require_role_fails() {
        let ctx = make_ctx(&[], Role::Viewer);
        let err = ctx.require_role(Role::Admin).unwrap_err();
        assert!(matches!(err, CasperError::Forbidden(_)));
    }

    #[test]
    fn admin_star_grants_any_scope() {
        let ctx = make_ctx(&["admin:*"], Role::Owner);
        assert!(ctx.require_scope("agents:triage:run").is_ok());
        assert!(ctx.require_scope("inference:sonnet-fast:call").is_ok());
        assert!(ctx.require_scope("platform:admin").is_ok());
    }
}
