# Ferry — Privacy Policy

Ferry has no server, no backend, and no account system operated by its authors. There is no
central service that could collect data about you, because there is nothing hosted to collect it.
This policy describes what the software itself does, since there is no company-run service to
describe instead.

## What Ferry collects

**Nothing is sent to the Ferry project, its maintainers, or any hosted Ferry service.** No
telemetry, no analytics, no crash reporting, no usage tracking — none exists in the codebase.

## What data exists, and where it stays

- **Identity keys** (Ed25519 signing key, age sealing key) are generated on your machine and
  stored in your OS keychain (Windows Credential Manager, macOS Keychain, or the Secret Service
  on Linux), or in an `age`-encrypted file if you choose file-based key storage. They never leave
  the machine that generated them.
- **Files, secrets, and messages** you send go directly, peer-to-peer, to the recipient machine
  over your LAN, hotspot, or another direct network path you configure. Ferry has no relay and
  no intermediary server; no third party — including the Ferry project — ever holds or can read
  this content, encrypted or otherwise.
- **The roster** (the list of machines you've paired with) and a **local audit log** are stored
  in a SQLite database on your own machine. The audit log records that an event happened — who
  did what, to which item, when, with what outcome — and never records the content of a transfer,
  a filename you marked sensitive, or a secret's key name.
- **Burn-after-read** items are deleted from local storage once opened; the deletion order is
  designed so an interrupted burn never leaves recoverable plaintext.

## The one optional path that talks to a third party

Ferry can optionally connect to **GitHub's API** (`api.github.com`, `github.com`) if you turn on
`remote_features_enabled` and connect an account. This is off by default and never activated
without your explicit action. When used:

- **Publishing a gist** sends an already end-to-end-encrypted (`age`-sealed) copy of one item you
  choose to GitHub as a private gist. GitHub can see the ciphertext size and ferry's own gist
  metadata; it cannot read the content, which is sealed to the recipient's key.
- **Fetching a team roster** reads a file from a repository you specify, to verify its signature
  and preview changes before importing.
- Your GitHub personal access token or OAuth device-flow token is stored the same way your
  identity key is (OS keychain, or an `age`-encrypted file) and is sent only to GitHub's own API
  over HTTPS, never to any other party.

No other third-party service is contacted by Ferry.

## Your controls

- Remote (GitHub) features are opt-in and can be disconnected at any time
  (`ferry provider disconnect`), which deletes the stored token.
- You can remove a paired peer, abort a queued transfer, or delete the local database entirely —
  it is a plain SQLite file under your own control.

## Source

Ferry is open source (Apache-2.0) at <https://github.com/firaslamouchi21/Ferry>. Every claim
above can be checked against the code that makes it.

## Changes to this policy

This file is versioned in the same repository as the software. Changes are visible in the git
history at the link above.

## Contact

Open an issue: <https://github.com/firaslamouchi21/Ferry/issues>
