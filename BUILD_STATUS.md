# Casper Build Status

## Task 0A — Workspace and crate stubs
**Status:** PASSED
**Completed:** 2026-04-15T07:30:00Z
**Commit:** 2f13e9a
**Summary:** Created Cargo workspace with 12 crates (10 library + 2 binary). All compile successfully with placeholder types.
**Files changed:** Cargo.toml, crates/*/Cargo.toml, crates/*/src/lib.rs, casper-server/*, casper-cli/*, config/casper-server.yaml, web/package.json, .gitignore
**Dependencies added:** tokio, axum, axum-extra, tower, tower-http, serde, serde_json, sqlx, ed25519-dalek, jsonwebtoken, sha2, rand, aes-gcm, hkdf, base64, zeroize, reqwest, tracing, tracing-subscriber, prometheus, dashmap, uuid, time, chrono, serde_yaml, clap, wiremock, thiserror, bytes, futures, async-trait
**Notes:** casper-base has real CasperError and type stubs (TenantId, CorrelationId). All other crates have minimal placeholder structs. Edition 2024, resolver 3.
---

## Task 0B — Database migration (platform tables)
**Status:** PASSED
**Completed:** 2026-04-15T07:35:00Z
**Commit:** 0ec2cc2
**Summary:** Created 0001_platform.sql with 7 platform tables. pgvector extension deferred to 0002 (not installed on system yet — needs apt install postgresql-16-pgvector).
**Files changed:** migrations/0001_platform.sql
**Dependencies added:** none
**Notes:** pgvector CREATE EXTENSION is commented out in 0001 — it's only needed for document_chunks in 0002. Database runs as sysadm user (owner), casper user has full grants. uuid-ossp extension enabled. Tables owned by sysadm — RLS will use SET LOCAL app.tenant_id.
---

## Task 0C — Database migration (tenant-scoped tables)
**Status:** PASSED
**Completed:** 2026-04-15T07:40:00Z
**Commit:** d3159fa
**Summary:** Created 0002_tenant.sql with 19 tenant-scoped tables + RLS policies + conversation_quality view. 26 total tables, 1 view. RLS verified.
**Files changed:** migrations/0002_tenant.sql
**Dependencies added:** none
**Notes:** document_chunks created WITHOUT embedding vector column (pgvector not installed). Will need migration 0003 to add `embedding vector(1536)` column + ivfflat index once pgvector is installed. Changed usage_events index from `(created_at::date)` to just `created_at` because date cast is not immutable. casper role changed from SUPERUSER to NOSUPERUSER so RLS actually works. App should connect as `casper` user, table owner is `sysadm`.
---

## Task 0D — Server shell
**Status:** PASSED
**Completed:** 2026-04-15T07:45:00Z
**Commit:** 478ce08
**Summary:** Minimal Axum server with YAML config, PgPool, embedded SQL migrations, /health endpoint, CORS, tracing, graceful shutdown via CTRL+C.
**Files changed:** casper-server/src/main.rs, casper-server/src/config.rs
**Dependencies added:** none (all were in workspace from 0A)
**Notes:** Migration runner uses _migrations tracking table with include_str!() to embed SQL. Config loaded from CASPER_CONFIG env var or default config/casper-server.yaml. The server connects as the casper user. migrations are idempotent (skipped if already applied). Server binds 0.0.0.0:{port}.
---

## Task 1A — Base types
**Status:** PASSED
**Completed:** 2026-04-15T07:50:00Z
**Commit:** pending
**Summary:** Implemented TenantId/CorrelationId newtypes, Role enum with PartialOrd/Ord, Subject enum (User/ApiKey), SecretValue (zeroizing), resolve_secret with _FILE convention. 9 unit tests passing.
**Files changed:** crates/casper-base/src/types.rs, crates/casper-base/src/error.rs
**Dependencies added:** none
**Notes:** SecretValue has no Debug/Display/Serialize/Clone as required. Rust 2024 edition requires unsafe blocks for set_var/remove_var in tests. CasperError implements IntoResponse for Axum.
---

## Task 1B — Three-part scope matching
**Status:** PASSED
**Completed:** 2026-04-15T07:55:00Z
**Commit:** pending
**Summary:** Implemented Scope struct with parse/grants/Display/Serialize/Deserialize, TenantContext with require_scope/require_role, has_scope helper. 27 total unit tests passing.
**Files changed:** crates/casper-base/src/scope.rs (new), crates/casper-base/src/context.rs (new), crates/casper-base/src/lib.rs
**Dependencies added:** none
**Notes:** Scope matching rules: exact match, two-part grants three-part, wildcard (*), admin:* grants everything. Three-part scope with specific identifier does NOT grant the two-part version (e.g., agents:triage:run does not grant agents:run).
---

## Task 1C — JWT verification
**Status:** PASSED
**Completed:** 2026-04-15T08:00:00Z
**Commit:** pending
**Summary:** Implemented CasperClaims, JwtVerifier with Ed25519 (via ring/jsonwebtoken), authenticate() building TenantContext, RevocationCheck trait. 32 total tests passing.
**Files changed:** crates/casper-base/src/jwt.rs (new), crates/casper-base/src/lib.rs, crates/casper-base/Cargo.toml
**Dependencies added:** ring (dev-dependency for test key generation)
**Notes:** jsonwebtoken crate uses ring internally. EncodingKey::from_ed_der expects PKCS8 DER for signing. DecodingKey::from_ed_der expects raw 32-byte public key for verification (ring's UnparsedPublicKey). The JwtVerifier stores raw public key bytes. Tests generate keys via ring::signature::Ed25519KeyPair::generate_pkcs8.
---

## Task 1D — JWT signing and revocation cache
**Status:** PASSED
**Completed:** 2026-04-15T08:10:00Z
**Commit:** pending
**Summary:** Implemented JwtSigner (PKCS8 DER, access+refresh tokens), RevocationCache (RwLock<HashSet> + background DB refresh via CancellationToken). 4 casper-auth tests passing.
**Files changed:** crates/casper-auth/src/signer.rs (new), crates/casper-auth/src/revocation.rs (new), crates/casper-auth/src/lib.rs, crates/casper-auth/Cargo.toml, Cargo.toml
**Dependencies added:** tokio-util (workspace)
**Notes:** Access tokens 15 min, refresh tokens 7 days. RevocationCache uses CancellationToken for graceful shutdown. Background refresh from token_revocations table. sign_verify_with_revocation test proves full chain: sign → verify → revoke → reject.
---

## Task 1E — Database layer
**Status:** PASSED
**Completed:** 2026-04-15T08:15:00Z
**Commit:** pending
**Summary:** Implemented DatabasePools (main + analytics), TenantDb with SET LOCAL app.tenant_id in begin() and acquire(). RLS isolation verified with real database test.
**Files changed:** crates/casper-db/src/pool.rs, crates/casper-db/src/lib.rs
**Dependencies added:** none
**Notes:** TenantDb.begin() starts a transaction with SET LOCAL — RLS automatically scoped. Tests use casper user (non-superuser, RLS enforced) for data access and sysadm user (table owner, RLS bypassed) for test setup via peer auth on Unix socket. DATABASE_URL defaults to postgres://casper:casper@localhost:5432/casper.
---

## Task 1F — Secret vault
**Status:** PASSED
**Completed:** 2026-04-15T08:20:00Z
**Commit:** pending
**Summary:** Implemented Vault with HKDF-SHA256 per-tenant key derivation, AES-256-GCM encrypt/decrypt, set/get/delete/list_keys (DB-backed), resolve_mcp_secret, encrypt_value/decrypt_value helpers. 4 tests passing.
**Files changed:** crates/casper-vault/src/vault.rs (new), crates/casper-vault/src/lib.rs, crates/casper-vault/Cargo.toml
**Dependencies added:** none
**Notes:** Vault.encrypt_value/decrypt_value store nonce:ciphertext in a single string (for SSO client_secret_enc, platform_backends.api_key_enc). Vault.set/get use separate DB columns (tenant_secrets table). resolve_mcp_secret parses "secret:key_name" format. Per-tenant key isolation verified in tests.
---

## Task 1G — Audit writer
**Status:** PASSED
**Completed:** 2026-04-15T08:25:00Z
**Commit:** pending
**Summary:** Implemented AuditWriter with bounded mpsc channel, batched inserts, non-blocking log(). Also implemented UsageRecorder and RuntimeMetrics (Prometheus) as part of casper-observe.
**Files changed:** crates/casper-observe/src/audit.rs (new), crates/casper-observe/src/usage.rs (new), crates/casper-observe/src/metrics.rs (new), crates/casper-observe/src/lib.rs
**Dependencies added:** none
**Notes:** AuditWriter uses try_send to never block callers. Background task receives entries, drains up to 100 per batch, inserts individually (could be optimized to multi-row INSERT later). UsageRecorder tracks all four token types (input, output, cache_read, cache_write). RuntimeMetrics has counters for LLM calls, tool calls, actor lifecycle, and HTTP requests.
---

## Task 1H — Usage recorder and metrics endpoint
**Status:** PASSED
**Completed:** 2026-04-15T08:30:00Z
**Commit:** pending
**Summary:** Wired AuditWriter, UsageRecorder, RuntimeMetrics into AppState. Added GET /metrics endpoint returning Prometheus text format. Verified both /health and /metrics work.
**Files changed:** casper-server/src/main.rs
**Dependencies added:** none
**Notes:** AppState now carries audit, usage, metrics. Metrics endpoint returns Prometheus text format via RuntimeMetrics::render().
---

## Task 1I + 1J — JWT and API key auth middleware (unified)
**Status:** PASSED
**Completed:** 2026-04-15T08:40:00Z
**Commit:** pending
**Summary:** Unified auth middleware: extracts Bearer token, routes to JWT verify or API key lookup (csk- prefix). Builds TenantContext for both paths. ScopeGuard extractor for per-route scope enforcement. SHA-256 hashing for API key lookup.
**Files changed:** casper-server/src/auth.rs (new), casper-server/src/main.rs, casper-server/Cargo.toml
**Dependencies added:** sha2, hex, chrono (casper-server)
**Notes:** Both JWT and API key auth produce the same TenantContext type. API keys default to operator role. ScopeGuard extractor reads TenantContext from request extensions and provides require() method. No const generic RequireScope (Rust doesn't support &str const generics). AuthState holds jwt_verifier, revocation_cache, db pool.
---
