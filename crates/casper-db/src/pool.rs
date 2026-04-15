use casper_base::TenantId;
use sqlx::postgres::PgPoolOptions;
use sqlx::{PgPool, Postgres, Transaction};

/// Holds main and analytics database pools.
#[derive(Clone)]
pub struct DatabasePools {
    pub main: PgPool,
    pub analytics: PgPool,
}

impl DatabasePools {
    /// Create pools from a database URL and pool sizes.
    pub async fn connect(
        url: &str,
        main_pool_size: u32,
        analytics_pool_size: u32,
    ) -> Result<Self, sqlx::Error> {
        let main = PgPoolOptions::new()
            .max_connections(main_pool_size)
            .connect(url)
            .await?;

        let analytics = PgPoolOptions::new()
            .max_connections(analytics_pool_size)
            .connect(url)
            .await?;

        Ok(Self { main, analytics })
    }
}

/// Wrapper that sets RLS tenant context on transactions.
///
/// Usage:
/// ```no_run
/// # async fn example(pool: sqlx::PgPool) {
/// let tenant_db = casper_db::TenantDb::new(pool, casper_base::TenantId::new());
/// let mut tx = tenant_db.begin().await.unwrap();
/// // All queries on tx are now scoped to the tenant via RLS
/// # }
/// ```
pub struct TenantDb {
    pub pool: PgPool,
    pub tenant_id: TenantId,
}

impl TenantDb {
    pub fn new(pool: PgPool, tenant_id: TenantId) -> Self {
        Self { pool, tenant_id }
    }

    /// Begin a transaction with the tenant context set via SET LOCAL.
    /// SET LOCAL only lasts for the transaction, so RLS is automatically cleaned up.
    pub async fn begin(&self) -> Result<Transaction<'_, Postgres>, sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        sqlx::query(&format!(
            "SET LOCAL app.tenant_id = '{}'",
            self.tenant_id.0
        ))
        .execute(&mut *tx)
        .await?;
        Ok(tx)
    }

    /// Acquire a connection and set the tenant context.
    /// Returns an owned connection from the pool.
    pub async fn acquire(
        &self,
    ) -> Result<sqlx::pool::PoolConnection<Postgres>, sqlx::Error> {
        let mut conn = self.pool.acquire().await?;
        sqlx::query(&format!(
            "SET LOCAL app.tenant_id = '{}'",
            self.tenant_id.0
        ))
        .execute(&mut *conn)
        .await?;
        Ok(conn)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn test_db_url() -> String {
        std::env::var("DATABASE_URL")
            .unwrap_or_else(|_| "postgres://casper:casper@localhost:5432/casper".to_string())
    }

    #[tokio::test]
    async fn database_pools_connect() {
        let pools = DatabasePools::connect(&test_db_url(), 2, 1).await.unwrap();
        let row: (i32,) = sqlx::query_as("SELECT 1").fetch_one(&pools.main).await.unwrap();
        assert_eq!(row.0, 1);
    }

    #[tokio::test]
    async fn tenant_db_rls_isolation() {
        let pool = PgPoolOptions::new()
            .max_connections(2)
            .connect(&test_db_url())
            .await
            .unwrap();

        // Use random UUIDs to avoid collision with application data
        let tenant_a = TenantId(Uuid::now_v7());
        let tenant_b = TenantId(Uuid::now_v7());

        // Setup: create test tenants and users using the pool owner (bypasses RLS)
        // We use a separate connection for setup that connects as the DB owner
        // Use Unix socket peer auth for the DB owner (sysadm)
        let setup_url = std::env::var("DATABASE_URL_OWNER")
            .unwrap_or_else(|_| "postgres:///casper?host=/var/run/postgresql&user=sysadm".to_string());
        let setup_pool = PgPoolOptions::new()
            .max_connections(1)
            .connect(&setup_url)
            .await
            .unwrap();

        // Clean up first (order matters due to FK constraints)
        sqlx::query("DELETE FROM token_revocations WHERE tenant_id IN ($1, $2)")
            .bind(tenant_a.0)
            .bind(tenant_b.0)
            .execute(&setup_pool)
            .await
            .unwrap();
        sqlx::query("DELETE FROM tenant_users WHERE tenant_id IN ($1, $2)")
            .bind(tenant_a.0)
            .bind(tenant_b.0)
            .execute(&setup_pool)
            .await
            .unwrap();
        sqlx::query("DELETE FROM tenants WHERE id IN ($1, $2)")
            .bind(tenant_a.0)
            .bind(tenant_b.0)
            .execute(&setup_pool)
            .await
            .ok(); // OK to fail if other FKs exist

        // Create tenants
        let slug_a = format!("test-a-{}", &tenant_a.0.to_string()[..8]);
        let slug_b = format!("test-b-{}", &tenant_b.0.to_string()[..8]);
        sqlx::query("INSERT INTO tenants (id, slug, display_name) VALUES ($1, $3, 'Test A'), ($2, $4, 'Test B')")
            .bind(tenant_a.0)
            .bind(tenant_b.0)
            .bind(&slug_a)
            .bind(&slug_b)
            .execute(&setup_pool)
            .await
            .unwrap();

        // Create users
        sqlx::query("INSERT INTO tenant_users (id, tenant_id, subject, role, scopes, created_by) VALUES ($1, $2, 'user:a@test.com', 'admin', '{\"admin:*\"}', 'test')")
            .bind(Uuid::now_v7())
            .bind(tenant_a.0)
            .execute(&setup_pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO tenant_users (id, tenant_id, subject, role, scopes, created_by) VALUES ($1, $2, 'user:b@test.com', 'admin', '{\"admin:*\"}', 'test')")
            .bind(Uuid::now_v7())
            .bind(tenant_b.0)
            .execute(&setup_pool)
            .await
            .unwrap();

        // Test RLS: tenant A should only see their user
        let db_a = TenantDb::new(pool.clone(), tenant_a);
        let mut tx_a = db_a.begin().await.unwrap();
        let users_a: Vec<(String,)> =
            sqlx::query_as("SELECT subject FROM tenant_users")
                .fetch_all(&mut *tx_a)
                .await
                .unwrap();
        tx_a.commit().await.unwrap();
        assert_eq!(users_a.len(), 1);
        assert_eq!(users_a[0].0, "user:a@test.com");

        // Test RLS: tenant B should only see their user
        let db_b = TenantDb::new(pool.clone(), tenant_b);
        let mut tx_b = db_b.begin().await.unwrap();
        let users_b: Vec<(String,)> =
            sqlx::query_as("SELECT subject FROM tenant_users")
                .fetch_all(&mut *tx_b)
                .await
                .unwrap();
        tx_b.commit().await.unwrap();
        assert_eq!(users_b.len(), 1);
        assert_eq!(users_b[0].0, "user:b@test.com");

        // Cleanup
        sqlx::query("DELETE FROM tenant_users WHERE tenant_id IN ($1, $2)")
            .bind(tenant_a.0)
            .bind(tenant_b.0)
            .execute(&setup_pool)
            .await
            .unwrap();
        sqlx::query("DELETE FROM tenants WHERE id IN ($1, $2)")
            .bind(tenant_a.0)
            .bind(tenant_b.0)
            .execute(&setup_pool)
            .await
            .ok(); // May fail if other FK refs exist from prior test runs
    }
}
