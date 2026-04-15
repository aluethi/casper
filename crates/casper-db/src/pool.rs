use casper_base::TenantId;
use sqlx::PgPool;

/// Holds main and analytics database pools.
#[derive(Clone)]
pub struct DatabasePools {
    pub main: PgPool,
    pub analytics: PgPool,
}

/// Wrapper that sets RLS tenant context on connections.
pub struct TenantDb {
    pub pool: PgPool,
    pub tenant_id: TenantId,
}

impl TenantDb {
    pub fn new(pool: PgPool, tenant_id: TenantId) -> Self {
        Self { pool, tenant_id }
    }
}
