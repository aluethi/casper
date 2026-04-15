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
