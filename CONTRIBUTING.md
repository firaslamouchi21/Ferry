# Contributing to Ferry

Glad you're here. This file covers the things that will save you a round-trip in review: the
handful of rules Ferry won't bend on, how the code is laid out, and what "done" looks like.

The short version of the review bar: a change that respects the invariants below is easy to
merge even if it's rough. A change that quietly breaks one gets sent back no matter how useful
it is.

## The things Ferry won't compromise on

1. **There is no server, and there never will be** — no relay, no rendezvous, no hosted
   discovery, not even as a stopgap. If a feature seems to need one, it's a different product.
2. **A machine that isn't in your roster can't do anything.** That's enforced during the Noise
   handshake, before a single command is parsed — never just hidden in the UI.
3. **Pairing always needs a code read out loud and a phrase both people confirm.** No skipping
   it, no defaulting to yes, no "trust this network".
4. **Every item has a real state, and every transition goes through the one validator.** The
   status you see is the status that's actually recorded — Ferry never shows "sent" hopefully.
5. **Sending is a local write.** The queue row and the state change happen in one transaction;
   something else delivers it later.
6. **Expiry and burn-after-read run on the *receiver's* clock, from the moment of delivery** —
   never on a timestamp the sender supplied. If the clocks disagree after a reboot, the item
   expires.
7. **A plaintext secret never lands on disk, in a log, or in a crash report.** Burning one
   deletes the wrapped key first, then the ciphertext, then the row.
8. **The audit log records that something happened — never what it was.**
9. **No machine ever forwards data for another machine.** Delivery is always direct.
10. **Crypto is built from primitives that have been reviewed by other people** — age, Noise
    XK, SPAKE2, ed25519. Nothing hand-rolled.

If you want the reasoning behind any of these, it's in
`docs/private/FERRY_TECHNICAL_DOCUMENTATION.md`.

## How the code is arranged

Dependencies only ever point one way:

```
daemon / cli / desktop  →  core  →  net / store / crypto  →  proto
```

- **`crates/proto`** is just types, enums, and error codes — no logic. The TypeScript bindings
  are generated from it, and CI fails if you hand-edit them.
- **`crates/core`** holds all the actual behaviour, and it has to run unchanged whether it's
  driven by the daemon, the CLI, or a test with a fake network. It never mentions a socket, a
  database, or React.
- **`crates/net`, `store`, `crypto`** are the machinery underneath — bytes, storage, primitives.
- **`crates/daemon` and `crates/cli`** wire everything together and do nothing else.
- **`ui/`** is one React app that runs in three places (VS Code, the desktop shell, a dev
  browser). No screen talks to the daemon directly — it all goes through one typed client in
  `ui/src/lib/ipc`.
- **`ext/vscode/` and `desktop/`** are thin shells around that app.

## minimal comments

keep comments minimal unless there's a genuine reason a future
reader couldn't work it out from the code itself (a subtle safety invariant, a workaround for
someone else's bug)

## Getting a change in

- **A new feature** roughly goes: decide the state model, add the `proto` type, write the
  migration, put the logic in `core`, decide what happens offline, add the policy check, wire
  the IPC handler, add it to the typed client, then the UI, then tests, then run it between two
  real machines, then a ledger entry.
- **A bug fix** starts by reproducing it in the two-node harness. Say what actually caused it in
  the commit message, fix it in the right layer, and check it on two machines.
- **A protocol change** means bumping the protocol version, writing down what happens when an
  old version meets a new one, and adding a harness case for exactly that. Changing the wire
  format without a version bump is a bug.

## Tests

The goal is simple: **if CI is green, Ferry works.** Not "it compiled" — it works. So:

- Test the decision, not the plumbing.
- Every bug that reaches `main` gets a regression test in the same change that fixes it — a
  harness case if you can manage one.
- New crypto, roster, or pairing code needs an adversarial test, not just a happy path.

Run this locally before you open a PR:

```sh
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo deny check && cargo audit
cargo test --package ferry-proto --locked && git diff --exit-code -- bindings/
pnpm install --frozen-lockfile
pnpm --filter @ferry/ui typecheck && pnpm --filter @ferry/ui test
bash testing/local-two-peer.sh       # two real daemons on one machine
bash testing/verify-invariants.sh    # roster / burn / TTL / offline / audit
bash testing/scenario.sh             # the same across two containers
```

CI runs all of that plus Windows and macOS builds, a strict two-container run that hard-fails
if the real transfer + invariant suite doesn't complete over multicast, a VS Code extension
activation smoke, and a forward-migration test against a checked-in old database file.

## The optional GitHub features

Ferry can publish a sealed item as a private gist and pull a signed team roster from a repo.
Both are **off by default** (`remote_features_enabled`), make no network call until a user
connects an account, and never put the token in `config.toml` / the IPC boundary / the audit
log. They live behind port traits in `crates/core` (`RemoteFetch`, `SnippetPublisher`); the
only crate that knows HTTP exists is `crates/remote`. If you touch this path, read §S of
`docs/private/FERRY_TECHNICAL_DOCUMENTATION.md` first — INV-S1..S7 bound what it may do. The
blocking HTTPS calls run on a dedicated worker thread, never on the IPC loop. Setup for the
device-flow login is in `docs/GITHUB_APP_SETUP.md`.

## Commits and pull requests

- Conventional commit prefixes (`fix:`, `feat:`, `refactor:`, `docs:`, `ci:`). The body says
  what actually went wrong, not just the symptom.
- One idea per pull request. Keep the diff small enough to review in one sitting.
- If you change a structural decision, update `docs/private/FERRY_TECHNICAL_DOCUMENTATION.md` to
  match — and don't re-argue a decision that's already written down there.

## Licensing

Ferry is [Apache 2.0](LICENSE), and it's also sold as packaged builds. When you send a
contribution:

- **Sign off your commits** (`git commit -s`). That's the
  [Developer Certificate of Origin](https://developercertificate.org/) — you're stating the
  code is yours to give.
- Your contribution goes in under Apache 2.0, the same as the rest of the project, and can be
  included in the commercial builds on those same terms.

## Security problems

Please don't file those as public issues — see [SECURITY.md](SECURITY.md).
