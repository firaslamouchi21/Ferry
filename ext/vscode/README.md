# Ferry for VS Code

Ferry moves files, secrets, and short messages straight between the machines on your team —
no cloud, no account, no server anywhere in the path. Every machine runs the same small daemon,
finds the others on your network, and connects to them directly over an encrypted channel.

This extension puts the Ferry panel inside VS Code and runs the daemon for you — it ships with
the matching `ferry-daemon` binary, so there's nothing else to install.

## What you can do

- **Ferry: Open Panel** — the full Ferry UI in a tab
- **Ferry: Pair a Device** — walk through the pairing handshake
- **Ferry: Send File** — also on the right-click menu in the Explorer
- **Ferry: Open Inbox** — see what people have sent you
- **Ferry: Copy My Fingerprint**
- **Ferry: Start Daemon** / **Restart Daemon** — the extension does this on its own when you
  open the panel, but the commands are there if you need them

## Settings

- `ferry.socketPath` — where the daemon's IPC socket lives (leave blank for the default)
- `ferry.daemonPath` — only used by builds that don't bundle a daemon; the platform-specific
  extension carries its own

The project lives at https://github.com/firaslamouchi21/Ferry.
