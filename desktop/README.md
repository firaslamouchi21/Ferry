# @ferry/desktop

Tauri 2 shell that hosts the shared `@ferry/ui` bundle as a native window.

The frontend (`src/`) imports `App` and `FerryProvider` straight from `../ui/src`
(vite aliases `@ferry/ui/*`) and injects a `TauriTransport` that speaks to the
daemon through Rust commands in `src-tauri/src/lib.rs`:

- `ipc_request(envelope)` — one connect-per-request against the daemon socket,
  4-byte length-framed, same wire as the CLI.
- `ipc_subscribe()` — holds a socket open and re-emits each `IpcEvent` frame as a
  Tauri `ferry://event`; emits `ferry://subscribe-closed` when the daemon goes away
  so the UI reconnects.
- `start_daemon()` / `daemon_running()` — the app auto-starts the daemon on launch.
- `pick_file()` / `pick_save_path(name)` / `write_file(path, content_base64)` —
  native file dialogs and a write, so sending and receiving files use real paths.

## The daemon ships inside the app

`tauri.conf.json` declares `ferry-daemon` as an `externalBin` sidecar. Tauri
expects it at `src-tauri/binaries/ferry-daemon-<host-triple>` at build time and
installs it next to the app binary (`/usr/bin/ferry-daemon` from the `.deb`, inside
the `.app` on macOS, beside the `.exe` on Windows), which is exactly where
`lib.rs`'s `daemon_bin()` looks. The staged copy is gitignored; stage it with:

```
cargo build -p ferry-daemon                       # or --release
pnpm --filter @ferry/desktop stage-daemon --profile debug   # or release (default)
```

`tauri dev` and `tauri build` run the staging step themselves (`beforeDevCommand` /
`beforeBuildCommand`), but a bare `cargo check` in `src-tauri` needs it done first —
the Tauri build script refuses to compile without the sidecar present.

## Running

```
pnpm --filter @ferry/desktop tauri dev                 # dev window against the vite server
pnpm --filter @ferry/desktop tauri build --bundles deb  # a real package (also appimage, rpm, dmg, msi, nsis)
```

On Linux with a hybrid Intel/NVIDIA GPU under Wayland, WebKitGTK may render a blank
window; `WEBKIT_DISABLE_DMABUF_RENDERER=1 WEBKIT_DISABLE_COMPOSITING_MODE=1` in front
of either command works around it.

The app binary is named `ferry-desktop` (`mainBinaryName`) on purpose so a `.deb`
never collides with the CLI's `ferry` on `PATH`.

## Testing a package without installing it

```
dpkg -x src-tauri/target/release/bundle/deb/Ferry_1.0.0_amd64.deb /tmp/ferry-pkg
mkdir -p /tmp/ferry-data/ferry && printf 'identity_keystore = "file"\n' > /tmp/ferry-data/ferry/config.toml
XDG_DATA_HOME=/tmp/ferry-data FERRY_IDENTITY_PASSPHRASE=test /tmp/ferry-pkg/usr/bin/ferry-desktop
ferry --socket /tmp/ferry-data/ferry/ferry.sock status
```

The isolated data dir plus the file keystore keep the test away from your real
identity and keychain. Closing the window leaves the daemon running (it's detached
on purpose); relaunching the app reuses it.

`src-tauri/capabilities/default.json` grants the main window `core`, `dialog`,
`notification`, and `shell` permissions explicitly.
