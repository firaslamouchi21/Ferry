# Ferry

Ferry moves files, secrets, and short messages straight from one of your machines to another —
no cloud, no account, no server in the middle. Every machine on your team runs the same small
`ferry-daemon`, which finds the others on your network and hands the bytes across a direct,
encrypted connection. Nothing you send ever touches a disk you don't control.

It's a bit like AirDrop, except it also handles `.env` files and API keys, it runs inside your
editor, and it works the same on Linux, macOS, and Windows.

## Where it came from

I built Ferry for myself. I work across a few machines and I kept needing to get the same
`.env` file or a fresh API key from one to another, and every option was annoying: paste it
into some web service, drop it in a chat, carry a flash disk between desks. It always felt
wrong to route a secret through a third party just to move it three metres.

So I wrote a small thing that copies bytes straight from one of my machines to another over
the local network, with no server anywhere. Once it actually worked well, it seemed worth
cleaning up and handing to other people — so that's what this is now: the same tool, made
safe and clear enough to release.

> **This is pre-release software.** The wire format and the on-disk format are still changing,
> and none of it has had an outside security review yet. Play with it, don't depend on it —
> and don't put anything through it that you'd be upset to lose or leak. Compatibility between
> versions isn't promised until 1.0.

## Why bother

Most ways of getting a file or a secret to a teammate go through somebody else's computer — a
cloud drive, a paste site, a chat app, email. That copy sits there, gets indexed, gets backed
up, and is one breach or subpoena away from being somebody else's problem.

Ferry doesn't do any of that. The data leaves your machine and arrives on theirs, and that's
the whole trip. There's no server to break into, bill you for, or take down.

The trade-off is that Ferry only works when both machines can actually reach each other — the
same office network, a phone hotspot, a VPN. There's no relay and there never will be, so it
won't punch through the internet for you. If that's what you need, Ferry isn't the tool.

## What it won't do

A few things worth being upfront about:

- It doesn't sync or back anything up. If a machine dies with items still queued, they're gone.
- A machine never passes along data meant for a third machine. Both ends have to be reachable
  on the same network.
- Messages arrive in order per peer per connection, and that's the only ordering guarantee.
- There are no read receipts unless the sender asks for one, and no "who's online" beyond
  "reachable on this network right now".
- It can't protect a machine from its own user or from malware running as you. Ferry keeps your
  data safe in transit and behind a locked keychain; it can't do anything about a box that's
  already compromised.

## The optional GitHub bits

Two features can reach GitHub, and both are **off until you turn them on** (set
`remote_features_enabled = true` and connect an account):

- **Publish** — take an item you've already sent and put an encrypted copy on GitHub as a
  private gist, then hand someone the link. Useful when you can't get on the same network. The
  trade-off is real: the ciphertext leaves your machine, so burn-after-read can't be honoured
  any more, and GitHub learns the recipient's public key and the file size.
- **Team roster** — pull a signed roster from a repo file, check who signed it, look at the
  add/remove diff, and import on a second confirmation.

Nothing else touches a network you didn't ask it to. A peer-to-peer transfer is always direct;
Ferry never falls back to GitHub on its own. A personal access token is all you need; the
browser device-flow login additionally wants a one-time OAuth App registration —
[docs/GITHUB_APP_SETUP.md](docs/GITHUB_APP_SETUP.md).

## How the security works

Every machine has one identity key, made the first time the daemon runs and never written to
disk in the clear. Two machines only become peers through a deliberate handshake: someone reads
a short code across, and then both people confirm that the same verification phrase shows up on
both screens. Ferry never skips that step or assumes yes.

After that, every connection starts with a Noise handshake that hangs up on any key that isn't
already in your roster — before it reads a single command. Files are sealed with `age`. Secrets
get their own throwaway key on top, and "burn after reading" genuinely shreds them. The audit
log knows that something happened and when, and nothing about what it was.

## Getting it

Installers aren't wired up yet — for now you build it yourself (see below). Once there are
releases:

- **Command line** — grab the `ferry` and `ferry-daemon` binaries for your platform from the
  releases page and drop them on your `PATH`.
- **VS Code** — install the Ferry extension. It brings its own daemon, so there's nothing else
  to set up.
- **Desktop app** — a signed installer for macOS, Windows, and Linux.

## A first run, from the terminal

```sh
# on both machines
ferry daemon start

# machine A: show a code
ferry pair listen --name laptop-a
#   prints an address and a 6-digit code, then a verification phrase

# machine B: type them in
ferry pair connect 192.168.1.20:53124 482913 --name laptop-b
#   shows the same phrase — both people confirm it matches

# machine A: send something
ferry send laptop-b ./build/app.tar.gz
ferry send laptop-b ./.env --burn      # gone once they open it

# machine B: pick it up
ferry receive list
ferry receive open <item-id> --out ./app.tar.gz
```

`ferry send` just writes the item to a local queue; the daemon delivers it the next time the
other machine is around. `ferry activity` shows you the local log of what's happened.

## Running it on a server

A headless box — a CI runner, a NAS, a container — usually has no desktop keychain to store the
identity key in. Tell Ferry to keep it in an encrypted file instead, in `config.toml`:

```toml
identity_keystore = "file"
```

and hand it the passphrase through `FERRY_IDENTITY_PASSPHRASE` (or point
`identity_passphrase_file` at a file). The key then lives in an `age`-encrypted `identity.age`.
Since nobody's there to confirm a verification phrase, pair the box from a laptop before you
deploy it, or import a roster somebody signed for you with `ferry roster import`.

## Building from source

You'll need a recent stable Rust toolchain and `pnpm`.

```sh
cargo build --release -p ferry-daemon -p ferry     # the binaries
cargo test --workspace                             # the whole test suite
bash testing/local-two-peer.sh                     # two real daemons on one machine
pnpm install && pnpm --filter @ferry/ui build      # the shared UI
```

[CONTRIBUTING.md](CONTRIBUTING.md) walks through the layout and the rules a change has to
respect.

## Contributing

Found a bug or want to help? [Open an issue](https://github.com/firaslamouchi21/Ferry/issues/new)
or send a pull request. Start with [CONTRIBUTING.md](CONTRIBUTING.md).

## License

[Apache 2.0](LICENSE).
