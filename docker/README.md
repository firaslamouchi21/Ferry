# Running Ferry from a container

This is a separate distribution channel from building the CLI/UI from
source — pull the image and go, no Rust/Node toolchain needed. Desktop
and the VS Code extension are not part of this image; get those from
their own release channels.

## Quick start

```
export FERRY_IDENTITY_PASSPHRASE=$(openssl rand -base64 24)   # save this — it unlocks your identity key
docker compose up -d
```

Open `http://localhost:5170` for the UI. `FERRY_IDENTITY_PASSPHRASE` is
required: a container has no OS keychain, so the identity key is stored
age-encrypted on disk (`identity_keystore = "file"`) instead, and this
passphrase is what wraps it. Losing it means losing the identity —
keep it somewhere durable, the same as you would a disk-encryption key.

Identity, roster, and message/transfer history live in the `ferry-data`
named volume, so they survive `docker compose down` (without `-v`) and
container restarts/recreations.

## Why pairing works differently here

Ferry's peer discovery is mDNS-based, which doesn't cross a Docker
bridge network onto your real LAN. Pairing itself doesn't need
discovery though — `ferry pair connect <ADDR> <CODE>` already takes an
explicit address, which is exactly the fallback path this image relies
on. `docker-compose.yml` publishes the P2P port (`47821` by default,
`$FERRY_P2P_PORT` to change it) to the host, so:

- **Pairing out** to another real device on the LAN: run
  `docker compose exec ferry ferry pair connect <their-IP>:<their-port> <code> --name <you>`
  — this works normally, outbound connections aren't affected by the
  container boundary.
- **Pairing in** (someone else connects to your containerized Ferry):
  run `docker compose exec ferry ferry pair listen` inside the
  container, but give the other person `<this-host's-LAN-IP>:47821`
  (or whatever `$FERRY_P2P_PORT` is set to) rather than whatever
  address the command itself prints — that address is the container's
  internal view of itself, not reachable from outside the container.

Once paired, delivery to a rostered peer still works over the direct
Noise-XK transport — it just falls back to a periodic redrive instead
of the instant mDNS-reappearance path, so a delivery may take up to
~60–90s to start after the peer comes back online instead of being
immediate. `ferry send`/`ferry receive`/`ferry status` etc. all work
exactly as documented elsewhere once paired.

## Building the image locally

```
docker build -f docker/Dockerfile -t ferry:local .
```

## Publishing

`.github/workflows/docker-publish.yml` builds and pushes to GHCR on
every `v*` tag automatically (uses the repo's own `GITHUB_TOKEN`, no
setup needed). Docker Hub is opt-in: set the `DOCKERHUB_USERNAME`
repository variable and a `DOCKERHUB_TOKEN` secret to also push there;
leave them unset and that half of the job is skipped.
