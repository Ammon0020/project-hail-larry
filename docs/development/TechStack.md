# Tech Stack

Chosen for battery efficiency, single-binary distribution, and dev velocity.

## Daemon / Backend — Go

- Single static binary, cross-compiled for macOS/Linux/Windows. No runtime dependency.
- ~10-20MB RAM at idle, minimal CPU. Battery-friendly.
- `go:embed` bundles the frontend into the binary.
- `net/http` for the web server, `nhooyr.io/websocket` or `gorilla/websocket` for WebSocket.
- `os/exec` for spawning agent processes (ACP stdio JSON-RPC).

## Frontend — React + Vite + TailwindCSS

- React component model fits event-driven UI. Vite for hot reload and optimized bundles.
- TailwindCSS matches the mockup directly.
- CodeMirror 6 for code editor, diff view, and merge UI. Uses `@uiw/react-codemirror` (React wrapper) plus modular `@codemirror/*` packages (`state`, `view`, `commands`, `language`, `autocomplete`, `search`, `merge`) and per-language packages (`@codemirror/lang-javascript`, `@codemirror/lang-python`, etc.).
- `lucide-react` for icons.

## Database — SQLite

- Embedded, zero-config. Single file in `~/.local-agent/`.
- WAL mode for append-heavy event log with concurrent readers.
- `modernc.org/sqlite` (pure Go, no CGO) or `mattn/go-sqlite3` (CGO, faster).

## ACP Transport — stdio JSON-RPC

Agents communicate over stdin/stdout using JSON-RPC. The daemon spawns the agent via `os/exec`, pipes messages, and translates them into internal events. See https://agentclientprotocol.com/get-started/introduction.

## Dev / Prod

- **Dev** — Vite dev server proxies to Go backend. Hot reload both sides.
- **Prod** — `npm run build` → `go:embed` compiles frontend into daemon binary. One binary, `app start`, open browser.

## Libraries

| Concern | Library |
|---|---|
| WebSocket | `nhooyr.io/websocket` or `gorilla/websocket` |
| SQLite | `modernc.org/sqlite` or `mattn/go-sqlite3` |
| QR code | `github.com/skip2/go-qrcode` |
| mDNS | `github.com/brutella/dnssd` |
| CLI | `github.com/spf13/cobra` |
| CodeMirror 6 | `@uiw/react-codemirror` + `@codemirror/*` packages |
| Icons | `lucide-react` |
| Three-way merge | `github.com/sergi/go-diff` |
