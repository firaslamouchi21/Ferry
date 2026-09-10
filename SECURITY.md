# Security

## Found something?

Please don't open a public issue for it. Email **firaslamou@gmail.com** with:

- what the problem is and roughly where in the code,
- how to reproduce it, or a proof of concept,
- what you think someone could do with it.

You'll hear back within a few days. Give it a reasonable window to be fixed and released before
you write about it publicly.

## What counts

Fair game: the pairing ceremony, the Noise transport, roster enforcement, how payloads are
sealed, burn-after-read, expiry and TTL handling, the audit log staying content-free, the IPC
boundary, and both identity keystores (keychain and encrypted file).

**Not in scope: a machine that's already compromised.** Ferry keeps your data safe while it's
moving and while it's sitting behind a locked keychain or an encrypted identity file. It can't
help a box that's running malware as you, or someone who already has the file keystore
passphrase.

## Where things stand

Ferry is pre-release. The protocol and the storage format aren't frozen, and no outside
security review has happened yet. Treat it with that in mind until 1.0.
