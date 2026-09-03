# @ferry/desktop

Tauri 2 shell that hosts the shared `@ferry/ui` bundle as a native window.

The frontend (`src/`) imports `App` and `FerryProvider` straight from `../ui/src`
(vite aliases `@ferry/ui/*`) and injects a `TauriTransport` that speaks to the
daemon through two Rust commands in `src-tauri/src/lib.rs`:

- `ipc_request(envelope)` — one connect-per-request against the daemon Unix
  socket, 4-byte length-framed, same wire as the CLI.
- `ipc_subscribe()` — holds a socket open and re-emits each `IpcEvent` frame as a
  Tauri `ferry://event`.

## Status

Frontend builds (`pnpm --filter @ferry/desktop build`). The Rust `src-tauri`
crate is **not** built yet — it needs the Tauri system prerequisites
(webkit2gtk, etc.) and an app icon at `src-tauri/icons/icon.png`. Before the
first `pnpm --filter @ferry/desktop tauri dev`:

1. install the Tauri prerequisites for your OS,
2. `pnpm --filter @ferry/desktop exec tauri icon <some-1024px.png>`,
3. add a tray-event handler and OS-notification calls in `lib.rs` (currently
   config-only).
