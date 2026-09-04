# Ferry for VS Code

Serverless peer-to-peer file, secret, and message transfer for developer teams.
No server, no relay, no hosted component — every machine runs an identical daemon
that is simultaneously client and server, discovering peers over the LAN and
connecting directly over an authenticated, encrypted channel.

This extension hosts the Ferry panel inside VS Code and talks to a local
`ferry-daemon`. Install and run the daemon separately; see the project README at
https://github.com/firaslamouchi21/Ferry.

## Commands

- **Ferry: Open Panel** — the full Ferry UI in a webview
- **Ferry: Pair a Device** — run the pairing ceremony
- **Ferry: Send File** — also on the explorer right-click menu
- **Ferry: Open Inbox**
- **Ferry: Copy My Fingerprint**

## Settings

- `ferry.socketPath` — path to the daemon IPC socket (empty = platform default)
- `ferry.daemonPath` — the `ferry-daemon` binary for lifecycle actions
