# Casper Codebase Review Report

**Date:** 2026-04-21
**Scope:** Full codebase review — all 14 crates (~20k lines of Rust)
**Reviewer:** Automated multi-agent review (Claude)

---

## Executive Summary

Casper is a multi-tenant LLM proxy platform with agent orchestration, MCP tool integration, and a ReAct execution engine. The codebase is well-structured and uses sound cryptographic primitives (Ed25519 for JWTs, AES-256-GCM + HKDF for encryption). However, the review identified **10 critical**, **13 important**, and **14 minor** findings — with security being the primary concern.

**The server is not production-safe in its current state.** The combination of default-open CORS, default-enabled dev auth, SSRF in agent tools, missing tenant isolation on key endpoints, and plaintext secret storage requires remediation before any real deployment.

---

## Priority Fix List

These are the top items to address first, ordered by risk:

1. Flip `dev_auth` default to `false` and restrict CORS default policy
2. Add SSRF protection to `web_fetch` (deny private/link-local IP ranges)
3. Fix tenant isolation on `respond_to_ask` and `get_task_status`
4. Validate nonce length in vault decrypt + enforce master key minimum length
5. Redact API keys from `Debug`/`Serialize` on `ResolvedBackend`
6. Encrypt SSO client secrets through the existing Vault infrastructure
7. Set file permissions to `0600` on CLI credential files
8. Fix 401-to-BadGateway mapping to prevent key leakage across fallback backends
9. Escape SQL wildcards in `knowledge_search` ILIKE query
10. Validate delegation targets against the allowed agent list

---

## Critical Findings

### C-01: CORS allows any origin by default

- **Location:** `casper-server/src/main.rs`, lines 224–236
- **Crate:** casper-server
- **Description:** When `cors_origins` is empty (the default, since the config has no required value), the server uses `CorsLayer::new().allow_origin(Any)` combined with `allow_methods(Any)` and `allow_headers(Any)`. This is a fully open CORS policy. For a multi-tenant platform handling auth tokens, this lets any website make credentialed requests.
- **Impact:** Cross-site request forgery; any malicious website can make authenticated API calls on behalf of a logged-in user.
- **Recommendation:** Default to a restrictive policy (no origins allowed). Require explicit configuration of allowed origins.

---

### C-02: `dev_auth` defaults to `true`

- **Location:** `casper-server/src/config.rs`, line 47
- **Crate:** casper-server
- **Description:** `AuthConfig::default()` sets `dev_auth: true`. If a production config file omits the `[auth]` section entirely, the server starts in dev mode with an insecure deterministic vault master key (`main.rs` line 209) and the `/auth/login` endpoint open.
- **Impact:** Complete authentication bypass in production if the config section is missing.
- **Recommendation:** Default `dev_auth` to `false`. Require explicit opt-in. Log a loud warning when dev mode is active.

---

### C-03: SSO client secret stored in plaintext

- **Location:** `casper-server/src/routes/sso_routes.rs`, lines 97–109
- **Crate:** casper-server
- **Description:** The code ships `client_secret` directly into the `client_secret_enc` column unencrypted. A comment says "proper Vault encryption will be wired in later." The Vault infrastructure already exists and is used by the secret service.
- **Impact:** SSO provider credentials readable by anyone with database access.
- **Recommendation:** Wire in Vault encryption for SSO secrets before enabling any real SSO provider.

---

### C-04: SSRF via `web_fetch` tool

- **Location:** `crates/casper-agent/src/web_fetch.rs`, lines 77–78
- **Crate:** casper-agent
- **Description:** URL validation only checks for `http://` or `https://` prefix. An LLM-directed fetch can reach internal services (e.g., `http://169.254.169.254/` for cloud metadata, `http://localhost:5432/`).
- **Impact:** Server-side request forgery allowing access to internal infrastructure, cloud metadata endpoints, and local services.
- **Recommendation:** Resolve the hostname before connecting. Reject private IP ranges (10.0.0.0/8, 172.16.0.0/12, 192.168.0.0/16), link-local (169.254.0.0/16), and loopback (127.0.0.0/8). Consider using an allowlist approach or a dedicated egress proxy.

---

### C-05: SQL injection in `knowledge_search`

- **Location:** `crates/casper-agent/src/knowledge_search.rs`, line 88
- **Crate:** casper-agent
- **Description:** The user-provided query is interpolated into an ILIKE pattern via `format!("%{query}%")` without escaping SQL wildcards. Characters `%`, `_`, and `\` in the query manipulate the search pattern.
- **Impact:** Search pattern manipulation. While not full SQL injection (parameterized queries are used for the value), the ILIKE wildcards allow matching any content.
- **Recommendation:** Escape `%`, `_`, and `\` in the query string before wrapping it in the ILIKE pattern.

---

### C-06: Nonce length not validated during decryption

- **Location:** `crates/casper-vault/src/vault.rs`, line 67
- **Crate:** casper-vault
- **Description:** `Nonce::from_slice` will panic at runtime if the decoded nonce is not exactly 12 bytes. A malformed or tampered database row causes a process crash.
- **Impact:** Denial of service — a single corrupted row crashes the entire server.
- **Recommendation:** Validate nonce length before constructing. Return `CasperError::Internal("invalid nonce length")` instead of panicking.

```rust
let nonce_bytes = BASE64.decode(nonce_b64)?;
if nonce_bytes.len() != 12 {
    return Err(CasperError::Internal("invalid nonce length".into()));
}
```

---

### C-07: No master key length validation

- **Location:** `crates/casper-vault/src/vault.rs`, line 20
- **Crate:** casper-vault
- **Description:** `Vault::new` accepts any `Vec<u8>` — even empty. A too-short key silently degrades security.
- **Impact:** Weak or non-functional encryption with no warning.
- **Recommendation:** Enforce a minimum key length (32 bytes for AES-256).

---

### C-08: API keys exposed via `Debug`/`Serialize` on `ResolvedBackend`

- **Location:** `crates/casper-catalog/src/routing.rs`, lines 25–32
- **Crate:** casper-catalog
- **Description:** The `api_key_enc` field has both `Debug` and `Serialize` derives. Any `tracing::debug!` that formats a `ResolvedBackend` (or any JSON serialization) will emit the key. Tests even exercise `format!("{backend:?}")`.
- **Impact:** API key leakage through logs, error responses, or serialization.
- **Recommendation:** Implement a custom `Debug` that redacts the field. Add `#[serde(skip_serializing)]` to `api_key_enc`.

---

### C-09: Provider 401 mapped to `BadGateway` — key leakage across backends

- **Location:** `crates/casper-proxy/src/anthropic.rs`, line 518; `crates/casper-proxy/src/openai.rs`, line 471
- **Crate:** casper-proxy
- **Description:** A 401 (Unauthorized) from a provider is mapped to `CasperError::BadGateway`, which `is_non_retryable` considers retryable. The proxy will retry an invalid API key and then fall through to backup providers, potentially leaking the key context.
- **Impact:** Wasted retries on auth failures; credentials from one provider context sent to fallback providers.
- **Recommendation:** Map 401 responses to `CasperError::Unauthorized` and mark it as non-retryable.

---

### C-10: CLI credentials saved world-readable

- **Location:** `casper-cli/src/credentials.rs`, line 37
- **Crate:** casper-cli
- **Description:** `std::fs::write` creates the file with default umask permissions (typically 0644). The file contains `access_token` and `refresh_token` in plaintext JSON.
- **Impact:** On shared systems, any user can read the credentials.
- **Recommendation:** Use `std::os::unix::fs::OpenOptionsExt` to set mode `0o600` before writing.

---

## Important Findings

### I-01: `respond_to_ask` lacks tenant-scoped authorization

- **Location:** `casper-server/src/routes/run_routes.rs`, lines 189–203
- **Crate:** casper-server
- **Description:** The handler accepts a `_guard: ScopeGuard` (note the underscore — unused) and never verifies that the caller belongs to the same tenant that owns the conversation. Any authenticated user can inject an answer into any pending conversation if they can guess the `conversation_id` (a v7 UUID, which is time-sortable and partially predictable).
- **Recommendation:** Verify tenant ownership of the conversation before accepting the response.

---

### I-02: Async task status has no tenant isolation

- **Location:** `casper-server/src/routes/run_routes.rs`, lines 206–213
- **Crate:** casper-server
- **Description:** The `async_tasks` DashMap is keyed only by `task_id` with no tenant association. A user in tenant A can poll a `task_id` belonging to tenant B.
- **Recommendation:** Store `tenant_id` alongside each task entry and verify it on lookup.

---

### I-03: `/metrics` endpoint is unauthenticated

- **Location:** `casper-server/src/main.rs`, line 309
- **Crate:** casper-server
- **Description:** The Prometheus metrics endpoint is on the public router. Metrics can leak operational details (request rates, backend counts, error rates).
- **Recommendation:** Gate behind auth, or bind to a separate internal-only listener.

---

### I-04: `set_config` with `local_only=true` without a transaction

- **Location:** `crates/casper-db/src/pool.rs`, lines 66–75
- **Crate:** casper-db
- **Description:** `set_config(..., true)` means "local to current transaction." But `acquire()` does not start a transaction — the RLS tenant context only applies to the first query. Subsequent queries on the same connection run without tenant scoping.
- **Impact:** Row-level security bypass on multi-query flows that don't use `begin()`.
- **Recommendation:** Either start a transaction inside `acquire()`, use `set_config(..., false)` (session-scoped), or remove `acquire()` and force callers through `begin()`.

---

### I-05: Internal error messages leak to clients

- **Location:** `crates/casper-base/src/error.rs`, line 69
- **Crate:** casper-base
- **Description:** `to_json` serializes the full message for all error variants including `Internal`. Strings like "PKCS8 DER too short" are sent to API consumers.
- **Recommendation:** Return a generic message for `Internal` errors and log the detail server-side.

---

### I-06: Silent scope dropping in JWT authentication

- **Location:** `crates/casper-base/src/jwt.rs`, lines 102–106
- **Crate:** casper-base
- **Description:** `filter_map` silently discards unparseable scopes from the JWT. A malformed token authenticates with reduced permissions instead of being rejected.
- **Recommendation:** Reject the token entirely if any scope fails to parse.

---

### I-07: Master key stored in plain `Vec<u8>` — not zeroed on drop

- **Location:** `crates/casper-vault/src/vault.rs`, line 16
- **Crate:** casper-vault
- **Description:** The master key sits in ordinary heap memory, is `Clone`-able, and will not be zeroed on drop.
- **Recommendation:** Use `zeroize::Zeroizing<Vec<u8>>` or `secrecy::SecretVec`. Remove `#[derive(Clone)]` from `Vault`.

---

### I-08: HKDF without salt weakens key derivation

- **Location:** `crates/casper-vault/src/vault.rs`, line 26
- **Crate:** casper-vault
- **Description:** `Hkdf::new(None, &self.master_key)` passes `None` for the salt. A random, persisted salt significantly strengthens HKDF when the master key has less than ideal entropy.
- **Recommendation:** Generate a salt once at provisioning time and store it alongside the master key.

---

### I-09: Revocation cache grows without bound

- **Location:** `crates/casper-auth/src/revocation.rs`, lines 52–65
- **Crate:** casper-auth
- **Description:** `refresh_from_db` loads every row from `token_revocations` with no `WHERE` clause filtering expired tokens.
- **Recommendation:** Add `WHERE expires_at > now()` and periodically prune stale entries.

---

### I-10: Quota check is a no-op

- **Location:** `crates/casper-catalog/src/routing.rs`, lines 238–261
- **Crate:** casper-catalog
- **Description:** `check_quota` reads `requests_per_minute` from the database but never compares it against actual usage. Any tenant with an active quota row passes unconditionally.
- **Recommendation:** Implement actual rate counting (e.g., sliding window counter in Redis or a database-backed counter).

---

### I-11: Unbounded exponential backoff can overflow

- **Location:** `crates/casper-proxy/src/dispatch.rs`, line 73
- **Crate:** casper-proxy
- **Description:** `1u64 << (attempt - 1)` with no cap on `retry_attempts` (an `i32` from the DB). Large retry counts cause overflow or multi-hour sleeps.
- **Recommendation:** Clamp the delay to a ceiling (e.g., 30 seconds). Add jitter.

---

### I-12: Delegation target not validated against allowed list

- **Location:** `crates/casper-agent/src/engine/mod.rs`, lines 333–334; `crates/casper-agent/src/helpers.rs`, lines 167–280
- **Crate:** casper-agent
- **Description:** The LLM chooses which agent to delegate to. A prompt-injection attack could cause delegation to any agent name in the tenant, including agents the original user should not invoke.
- **Recommendation:** Validate the `target` agent against the `agents` list from the `Delegates` prompt block.

---

### I-13: Audit flush is row-by-row, not batched

- **Location:** `crates/casper-observe/src/audit.rs`, lines 97–118
- **Crate:** casper-observe
- **Description:** Despite the comment saying "batched inserts," `flush()` issues one `INSERT` per entry in a loop. Under load this means N database round-trips per batch.
- **Recommendation:** Use a single multi-row `INSERT` or `COPY`.

---

## Minor Findings

### M-01: `run` and `run_stream` share ~80% duplicated logic

- **Location:** `crates/casper-agent/src/engine/mod.rs`, lines 114–858
- **Crate:** casper-agent
- **Description:** These two methods share most of their ReAct loop logic. Divergence bugs are inevitable as the codebase evolves.
- **Recommendation:** Extract shared logic into a helper parameterized by a sink trait or callback.

---

### M-02: `ask_rx` channel shared across unrelated sentinel types

- **Location:** `crates/casper-agent/src/engine/mod.rs`, lines 640–775
- **Crate:** casper-agent
- **Description:** The same channel is used for ask_user, connect_required, and MCP OAuth responses. Out-of-order messages could be consumed by the wrong handler.
- **Recommendation:** Use distinct channels or tag messages with a discriminant.

---

### M-03: OAuth PRM origin validation is reversed

- **Location:** `crates/casper-mcp/src/oauth.rs`, line 137
- **Crate:** casper-mcp
- **Description:** `expected_origin.starts_with(&resource_origin)` should be an equality check or `resource_origin.starts_with(expected_origin)`. The current logic allows an empty `resource_origin` to pass validation.
- **Recommendation:** Use strict equality for origin comparison.

---

### M-04: `has_tool_use` checks wrong message format

- **Location:** `crates/casper-agent/src/history.rs`, lines 95–103
- **Crate:** casper-agent
- **Description:** The engine stores assistant messages in OpenAI format (`tool_calls` array), but `has_tool_use` looks for Anthropic-style `{"type": "tool_use"}` blocks. This function may never match, breaking turn-grouping for tool calls loaded from history.
- **Recommendation:** Align the check with the actual storage format.

---

### M-05: Leap year calculation is wrong for century years

- **Location:** `crates/casper-agent/src/assembler.rs`, line 62
- **Crate:** casper-agent
- **Description:** `year % 4 == 0` misclassifies century years (e.g., 2100 is not a leap year).
- **Recommendation:** Use `chrono`'s date utilities instead of manual calculation.

---

### M-06: Duplicated tool-definition conversion logic

- **Location:** `crates/casper-proxy/src/anthropic.rs` (lines 38–54 vs 127–143)
- **Crate:** casper-proxy
- **Description:** OpenAI-to-Anthropic tool mapping is copy-pasted between `call` and `call_stream`.
- **Recommendation:** Extract into a shared helper function.

---

### M-07: Streaming silently swallows channel send errors

- **Location:** `crates/casper-proxy/src/anthropic.rs`, `crates/casper-proxy/src/openai.rs`
- **Crate:** casper-proxy
- **Description:** `let _ = tx.send(...)` discards errors. If the client disconnects, the stream loop continues consuming the full provider response.
- **Recommendation:** Check the send result and break early on error.

---

### M-08: Token counts use `i32` — truncation risk

- **Location:** `crates/casper-proxy/src/types.rs`, lines 68–69; `crates/casper-agent/src/actor.rs`, line 99
- **Crate:** casper-proxy, casper-agent
- **Description:** `as i32` casts on token counts could truncate for very large context windows.
- **Recommendation:** Use `i64` or `u64`.

---

### M-09: No `max_tokens` upper-bound validation

- **Location:** `crates/casper-proxy/src/types.rs`, line 41
- **Crate:** casper-proxy
- **Description:** The `max_tokens` field is passed through to upstream providers without bounds checking. A malicious caller can request arbitrarily large generations.
- **Recommendation:** Validate or cap before dispatch, ideally tied to the tenant's quota.

---

### M-10: Password accepted as CLI argument

- **Location:** `casper-cli/src/commands/auth.rs`, line 17
- **Crate:** casper-cli
- **Description:** `--password` is a plain clap argument. Running `casper auth login --password s3cret` exposes the password in `ps`, shell history, and logs.
- **Recommendation:** Read from stdin or use a secure prompt (e.g., `rpassword` crate) when the flag is absent.

---

### M-11: `print_response` returns `Ok(())` on HTTP errors

- **Location:** `casper-cli/src/http.rs`, line 44
- **Crate:** casper-cli
- **Description:** Non-success HTTP responses are printed to stderr but the function still returns `Ok(())`, so the CLI exits with code 0 on server errors.
- **Recommendation:** Return an error for non-2xx responses.

---

### M-12: WebSocket receive loop blocks on inference dispatch

- **Location:** `casper-agent-backend/src/ws.rs`, lines 88–116
- **Crate:** casper-agent-backend
- **Description:** The main loop awaits inference completion before reading the next WebSocket message. Pings arriving during long inference are not answered, risking disconnection.
- **Recommendation:** Move the response send into the spawned task with a shared `Arc<Mutex<ws_sink>>`.

---

### M-13: Reconnect backoff has no jitter

- **Location:** `casper-agent-backend/src/main.rs`, lines 113–126
- **Crate:** casper-agent-backend
- **Description:** All sidecars use the same deterministic backoff. A server restart causes a thundering herd of simultaneous reconnections.
- **Recommendation:** Add randomized jitter to the backoff delay.

---

### M-14: `RuntimeMetrics::new()` panics on double-registration

- **Location:** `crates/casper-observe/src/metrics.rs`, lines 28–120
- **Crate:** casper-observe
- **Description:** Every `unwrap()` on metric creation and registration will panic if a metric is double-registered (e.g., in tests or if names collide).
- **Recommendation:** Return `Result` or use `once_cell`/`LazyLock`.

---

## Structural Observations

### Crate granularity is excessive for the project's size

The 14-crate workspace houses ~20k lines of Rust. Several crates are under 350 lines:

| Crate | Lines |
|---|---|
| casper-knowledge | 2 |
| casper-wire | 198 |
| casper-db | 205 |
| casper-auth | 246 |
| casper-vault | 248 |
| casper-observe | 349 |
| casper-catalog | 346 |

This overhead (14 Cargo.toml files, crate-boundary compile friction, cross-crate type coordination) is not justified at current scale. Consider consolidating to ~6 crates:

- **casper-base** ← merge in db, auth, vault, observe (all foundational infra, tightly coupled)
- **casper-catalog** ← merge in proxy (proxy exists solely to serve catalog routing)
- **casper-agent** ← merge in mcp (mcp is only used by agent)
- **casper-wire** — keep as-is (shared protocol, correctly isolated)
- **casper-server** — keep as-is (binary)
- **casper-cli** — keep as-is (binary)
- **casper-agent-backend** — keep as-is (binary)

Delete `casper-knowledge` until it has real code.

### No test coverage

No test files were found across the crates beyond the inline test in `casper-catalog/src/routing.rs` (which itself exercises `Debug` formatting that leaks API keys — see C-08). For a platform handling authentication, encryption, and multi-tenant isolation, automated tests are essential.

### Dead code

- `resolve_user_oauth_token` in `crates/casper-mcp/src/mcp.rs` (lines 231–285) always returns `Err` and is never called.
- `casper-knowledge` is an empty stub (2 lines).

---

## Checklist

Use this checklist to track remediation progress:

- [ ] **C-01** — Restrict default CORS policy
- [ ] **C-02** — Default `dev_auth` to `false`
- [ ] **C-03** — Encrypt SSO client secrets via Vault
- [ ] **C-04** — Add SSRF protection to `web_fetch`
- [ ] **C-05** — Escape SQL wildcards in `knowledge_search`
- [ ] **C-06** — Validate nonce length before decryption
- [ ] **C-07** — Enforce master key minimum length
- [ ] **C-08** — Redact API keys from Debug/Serialize
- [ ] **C-09** — Map provider 401 to Unauthorized (not BadGateway)
- [ ] **C-10** — Set credential file permissions to 0600
- [ ] **I-01** — Add tenant check to `respond_to_ask`
- [ ] **I-02** — Add tenant isolation to async task status
- [ ] **I-03** — Gate `/metrics` behind auth or internal listener
- [ ] **I-04** — Fix RLS `set_config` to use transactions
- [ ] **I-05** — Redact internal error messages from API responses
- [ ] **I-06** — Reject tokens with unparseable scopes
- [ ] **I-07** — Use `zeroize` for master key material
- [ ] **I-08** — Add salt to HKDF key derivation
- [ ] **I-09** — Filter expired entries in revocation cache
- [ ] **I-10** — Implement actual quota enforcement
- [ ] **I-11** — Cap exponential backoff + add jitter
- [ ] **I-12** — Validate delegation targets
- [ ] **I-13** — Batch audit inserts
- [ ] **M-01** — Deduplicate run/run_stream logic
- [ ] **M-02** — Separate ask_rx channels by type
- [ ] **M-03** — Fix OAuth origin validation
- [ ] **M-04** — Fix `has_tool_use` format check
- [ ] **M-05** — Fix leap year calculation
- [ ] **M-06** — Extract tool-definition conversion helper
- [ ] **M-07** — Break streaming loop on send error
- [ ] **M-08** — Widen token count types to i64
- [ ] **M-09** — Validate max_tokens upper bound
- [ ] **M-10** — Use secure password prompt in CLI
- [ ] **M-11** — Return error on non-2xx HTTP responses in CLI
- [ ] **M-12** — Fix WebSocket ping handling during inference
- [ ] **M-13** — Add jitter to reconnect backoff
- [ ] **M-14** — Handle metric double-registration gracefully
