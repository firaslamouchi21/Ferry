# @ferry/ui

One React + TypeScript + Vite bundle, built once and hosted three ways: a dev
browser, the VS Code webview (`ext/vscode`), and the Tauri desktop shell
(`desktop`).

## Rules

- **One IPC boundary.** Every daemon call goes through `src/lib/ipc` (the `ferry`
  client) via the hooks in `src/lib/query`. No screen opens a socket, reads the
  store, or performs a permission check.
- **Types come only from `../bindings`.** Those are generated from
  `crates/proto` by `cargo test -p ferry-proto`. `tsc --noEmit` here imports them,
  so proto drift breaks this build too. Never hand-edit a file in `bindings/`.
- **Daemon state lives in TanStack Query.** `IpcEvent::Changed` invalidates by
  resource; `Progress` updates a per-item cache entry; a reconnect invalidates
  everything. Context holds only theme and transient UI.
- **Delivery state is rendered literally** (`StateBadge`) — no optimistic UI.
- **Secrets** are masked by default (`MaskedValue`), revealed per value, re-masked
  on blur, never in the DOM until revealed.

## Run

```sh
pnpm --filter @ferry/ui dev
```

Open `http://localhost:5173`. A Vite plugin starts a WebSocket→Unix-socket bridge
at `/ferry-ipc` so the browser can reach a running `ferry-daemon`
(`FERRY_SOCK` overrides the socket path). Append `?mock` to run against fixtures
with no daemon.

## Check

```sh
pnpm --filter @ferry/ui typecheck   # tsc --noEmit
pnpm --filter @ferry/ui test        # vitest
pnpm --filter @ferry/ui build       # tsc + vite build -> dist/
```

## Layout

| Path | What |
|---|---|
| `src/lib/ipc` | transport (`WebSocket` / `VsCode` / `Mock`), the typed `ferry` client, `FerryIpcError` |
| `src/lib/query` | `FerryProvider`, query keys + resource→invalidation map, hooks + mutations |
| `src/app` | shell, `ConnectionGate`, command palette, toast host |
| `src/components` | `Button`, `DataTable`, `StateBadge`, `MaskedValue`, `Modal`, `ConfirmDialog`, … |
| `src/screens` | the nine screens (Peers, Send + Secret Composer, Inbox, Item detail, Sent, Activity, Pair, Settings) |
| `src/styles` | `tokens.css` (design tokens), `base.css`, `app.css` |
| `dev-bridge/bridge.js` | the browser dev bridge (Node) |
