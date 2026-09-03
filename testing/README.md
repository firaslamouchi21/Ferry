# testing/ — two-peer end-to-end harness

Runs two `ferry-daemon` instances as separate containers on one bridge
network and drives a real pair → send → receive → open cycle against
them, asserting the final content hash matches.

```
bash testing/scenario.sh
```

## What it covers

Verified passing on a plain docker bridge:

- both daemons boot with a real identity in a real Secret Service
  (`entrypoint.sh` runs a session `dbus-daemon` + `gnome-keyring-daemon`
  on a fixed bus address shared by the daemon and every `exec`'d CLI)
- SPAKE2 pairing over a real TCP socket, verification phrase (the PGP
  word list) computed and confirmed on both sides via piped `y`
- signed roster write on both ends
- `ferry send` writes an outbox row

Attempted, best-effort:

- `ferry send` → mDNS reappearance → chunked transfer → hash verify →
  `ferry receive open`. **This step is skipped with a WARN if bob does
  not receive the item in 45s**, because a plain docker bridge does not
  reliably carry mDNS multicast between containers (no IGMP querier).
  The scenario still exits 0 — pairing/roster/queue are the hard
  assertions, and the chunked-transfer + hash path is covered by the
  workspace unit and integration tests.

## What it does not replace

The Definition of Done (`docs/private/FERRY_TECHNICAL_DOCUMENTATION.md`
§K/§P) still wants a run on two physical machines including a hotspot run
and a closed-laptop run. This harness gives repeatable, CI-able coverage
of the code paths; it is not the physical run.

## Notes

- Each container gets its own `XDG_DATA_HOME=/data` volume, so the two
  daemons never share state. A `shared` volume is only used to hand the
  pairing code and payload files between them.
- `entrypoint.sh` starts a session D-Bus and `gnome-keyring-daemon` so
  `keyring` has a Secret Service to talk to — the real identity code path,
  not a mock.
- mDNS multicast does not cross the default docker bridge here (checked:
  alice's discovery loop never logs a `ServiceResolved` for bob). Fixing
  it needs host-level bridge config (an IGMP querier / `mcast_querier=1`)
  or a macvlan network with a host-specific parent interface — neither
  belongs in a committed, portable compose file. The transfer path is
  otherwise fully covered by the workspace tests; real cross-device
  transfer verification is the physical two-machine run.
- Windows is not covered here — Windows containers need a Windows host.
  Windows build/test runs on the `windows-latest` CI runner instead.
