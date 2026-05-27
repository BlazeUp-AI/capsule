# Capsule

**AI-native ephemeral coding runtime.** Browser terminal connected to isolated Docker containers via WebSocket — built to run Claude Code sessions in the cloud.

---

## What It Is

Capsule spins up a fresh Docker container for each session and connects it to a full xterm.js terminal in the browser over WebSocket. Each container is isolated, resource-limited, and disposable. The runtime is a single Rust binary (Axum + Tokio) that manages container lifecycle, PTY I/O, session reconnection, and workspace export.

The end goal is a zero-setup environment where you open a URL and immediately have a terminal running Claude Code.

---

<img width="2559" height="1473" alt="image" src="https://github.com/user-attachments/assets/996bf3e2-eaaa-474f-973b-f7b8ed5d4f57" />

## Architecture

```
Browser (xterm.js)
    ↕  WebSocket  (ws://host:3001/ws)
Rust Runtime (Axum)
    ↕  bollard Docker API
Docker Container  (capsule image)
    ↕  PTY (portable-pty)
bash / Claude Code
```

**Runtime modules:**

| Module | Responsibility |
|---|---|
| `main.rs` | Axum server setup, routes, session cleanup loop |
| `websocket.rs` | WebSocket handler, message protocol, session lifecycle |
| `session.rs` | Session state machine, `SessionManager` (create / reconnect / cleanup) |
| `docker.rs` | Container create/start/remove/export via bollard |
| `pty.rs` | PTY spawn (local or Docker exec), I/O threads, resize |

---

## Quick Start

### Prerequisites

- Rust 1.75+
- Node.js 18+
- Docker (daemon running)

### Build the container image

```bash
docker build -t capsule-runtime docker/
```

### Start the runtime

```bash
cd runtime
cargo run --release
# Listening on http://0.0.0.0:3001
# WebSocket at ws://0.0.0.0:3001/ws
```

### Start the frontend dev server

```bash
cd frontend
node server.js
# Serving at http://localhost:3002
```

Open **http://localhost:3002** in your browser.

---

## Project Structure

```
capsule/
├── runtime/              # Rust backend
│   └── src/
│       ├── main.rs       # Axum server, routes, cleanup task
│       ├── websocket.rs  # WS handler + client/server message protocol
│       ├── session.rs    # Session state + SessionManager
│       ├── docker.rs     # Container lifecycle via bollard
│       └── pty.rs        # PTY spawning + I/O threads
├── frontend/
│   ├── index.html        # Full terminal UI (xterm.js)
│   ├── test.html         # Minimal test terminal
│   ├── server.js         # Static dev server (port 3002)
│   └── lib/              # xterm.js bundle + CSS
└── docker/
    └── Dockerfile        # Container image (Debian + bash + dev tools)
```

---

## How It Works

1. Browser opens a WebSocket to `/ws` (optionally with `?session_id=` to reconnect).
2. Runtime waits for an initial `Resize` message to learn the terminal dimensions.
3. `SessionManager` creates a Docker container (`capsule-{uuid}`) and spawns a PTY attached to it.
4. Container resources are capped: **4 GB RAM**, **2 vCPUs**, **256 PIDs**.
5. Terminal I/O flows bidirectionally: keystrokes → PTY → container; container output → WebSocket → xterm.js.
6. On disconnect, the session is kept alive for **60 seconds** — reconnecting within that window resumes the same container.
7. After expiry, the container is removed and the session is cleaned up.

### WebSocket Message Protocol

**Client → Server**

```json
{ "type": "input",  "data": "ls -la\r" }
{ "type": "resize", "cols": 220, "rows": 50 }
{ "type": "ping" }
```

**Server → Client**

```json
{ "type": "output",     "data": "<base64>" }
{ "type": "connected",  "session_id": "<uuid>" }
{ "type": "error",      "message": "..." }
{ "type": "pong" }
```

### API Endpoints

| Method | Path | Description |
|---|---|---|
| `GET` | `/ws` | WebSocket upgrade (query: `?session_id=` for reconnect) |
| `GET` | `/health` | Health check |
| `GET` | `/api/export/{session_id}` | Download workspace as `.tar.gz` |

---

## Container Image

The `docker/Dockerfile` builds a `debian:bookworm-slim` image with:

- bash, git, git-lfs, curl, build-essential, vim, tmux, sudo
- fastfetch (runs on login)
- `developer` user with passwordless sudo
- Working directory `/workspace`
- `TERM=xterm-256color`, UTF-8 locale

---

## Status

- [x] WebSocket server
- [x] PTY management (local + Docker exec)
- [x] Docker container lifecycle (create / start / remove)
- [x] xterm.js terminal UI
- [x] Resize handling
- [x] Session reconnect (60s grace period)
- [x] Workspace export (tar archive download)
- [x] Session cleanup loop
- [ ] Claude Code integration
- [ ] Session persistence across restarts
- [ ] Auth / multi-user
- [ ] Production deployment config
