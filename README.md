# Capsule

AI-native ephemeral coding runtime. Browser terminal connected to Docker containers via WebSocket.

<img width="2559" height="1473" alt="image" src="https://github.com/user-attachments/assets/996bf3e2-eaaa-474f-973b-f7b8ed5d4f57" />

## Architecture

```
Browser (xterm.js)
    ↕ WebSocket
Rust Runtime (Axum)
    ↕ PTY
Docker Container
    ↕
bash / Claude Code
```

## Quick Start

### Prerequisites
- Rust 1.75+
- Node.js 18+
- Docker

### Run locally

```bash
# Terminal 1: Start runtime
cd runtime
cargo run --release

# Terminal 2: Start frontend
cd frontend
node server.js
```

Open http://localhost:3002

## Project Structure

```
capsule/
├── runtime/          # Rust backend
│   └── src/
│       ├── main.rs       # Axum server
│       ├── websocket.rs  # WS handler + messages
│       ├── session.rs    # Session management
│       └── pty.rs        # PTY spawning
├── frontend/         # Web frontend
│   ├── test.html         # Terminal UI
│   ├── server.js         # Dev server
│   └── lib/              # xterm.js
└── docker/           # Container images (TODO)
```

## Status

MVP in progress:
- [x] WebSocket server
- [x] PTY management
- [x] xterm.js terminal
- [x] Resize handling
- [ ] Docker containers
- [ ] Reconnect handling
- [ ] Claude Code integration
- [ ] Session persistence
