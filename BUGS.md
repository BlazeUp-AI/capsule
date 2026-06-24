# Bug Hunt Findings

Ordered by severity.

## Critical

### `/ws` is unauthenticated even when `CAPSULE_API_TOKEN` is set

`runtime/src/main.rs` exposes `/ws`, but `runtime/src/websocket.rs` destructures the API token as `_` and never checks it. Anyone who can reach the runtime can start an interactive container and potentially consume configured Anthropic/OpenRouter keys.

### Runtime defaults are network-exposed and unauthenticated

`runtime/src/main.rs` disables API auth when `CAPSULE_API_TOKEN` is absent, allows any CORS origin, and binds `0.0.0.0:3001` by default. Combined with session creation, exec, and file APIs, this is effectively an unauthenticated LAN-exposed container control plane unless the environment is carefully locked down.

## High

### Initial resize/session config can be silently dropped before WebSocket open

`frontend/src/Terminal.svelte` sends resize/config after a fixed 50ms timer, while `frontend/src/ws.js` drops sends unless the socket is already `OPEN`. If the socket opens after that timer, the backend falls back after 5s with default config in `runtime/src/websocket.rs`. User-entered DeepSeek/OpenRouter/Anthropic credentials may never reach the container, causing confusing "Claude Code not configured" behavior or accidental use of server keys.

### Exec output is accumulated unbounded before truncation

`runtime/src/docker.rs` appends all stdout/stderr into memory until the command finishes. The 1MB cap in `runtime/src/api.rs` happens only after collection. A command like `yes`, a large `cat`, or a noisy build can balloon runtime memory and response size before the safety limit applies.

### Observal session tokens are persisted in `localStorage`

`frontend/src/Workspace.svelte` stores access/refresh tokens in both `sessionStorage` and `localStorage`; `runtime/src/main.rs` does the same inside the iframe. Those users are created as `super_admin` in `runtime/src/observal.rs`, so a same-origin script or stale browser profile can retain powerful Observal credentials beyond the intended session lifetime.

## Medium

### File APIs allow arbitrary container paths, not just `/workspace`

`runtime/src/api.rs` accepts caller-provided paths directly for listing, reading, and writing files. This lets API callers read/write outside `/workspace`, including shell config or telemetry config inside the container.

### UTF-8 truncation can panic the API handler

`runtime/src/api.rs` checks byte length, then slices `s[..MAX_OUTPUT_BYTES]`. If the cutoff lands inside a multibyte character, Rust panics and the request fails instead of returning truncated output.

## Validation

- `cargo check -p capsule-runtime` passed.
- `npm run build` passed.
- `bash -n dev.sh` passed.
