# Ferry for VS Code

Send files, secrets, and short messages straight to your teammates' machines — over your own
network, encrypted end to end, with no server, no cloud, and no account.

Every machine runs the same small daemon. They find each other on the local network and connect
directly. Nothing you send passes through anyone else's computer.

The extension ships with the Ferry daemon for your platform, so there is nothing else to install.

## Getting started

1. Install the extension on both machines and open **Ferry** from the Activity Bar.
2. On one machine choose **Pair a device → Show a code**; on the other, choose **Enter a code** and
   type the address and six-digit code.
3. Compare the verification phrase on both screens and confirm. That's it — you're paired.
4. Right-click any file in the Explorer and choose **Send File**.

If the other machine is offline, the item waits on yours and is delivered when it comes back.

## Features

- **Files** of any size, resumed after a disconnect and checked against their hash on arrival
- **Secrets** such as `.env` values, sealed at rest and never written to disk in the clear
- **Messages** to a teammate, kept only on your two machines
- **Expiry and burn-after-read**, enforced on the receiving machine
- **Notifications** when something arrives, and daemon status in the status bar

## Commands

| Command | What it does |
| --- | --- |
| Ferry: Open Panel | The full Ferry interface in an editor tab |
| Ferry: Pair a Device | Walk through pairing |
| Ferry: Send File | Send the selected file (also in the Explorer and tab menus) |
| Ferry: Open Inbox | What people have sent you |
| Ferry: Copy My Fingerprint | Copy this machine's identity |
| Ferry: Start / Stop / Restart Daemon | Manage the background daemon |

## Settings

| Setting | Default | |
| --- | --- | --- |
| `ferry.autoStartDaemon` | `true` | Start the daemon when VS Code starts |
| `ferry.notifyOnReceive` | `true` | Notify you when something arrives |
| `ferry.socketPath` | empty | Connect to a daemon you manage yourself |
| `ferry.daemonPath` | `ferry-daemon` | Only used by builds without a bundled daemon |

## What it does on your computer

- The daemon listens on TCP port 47821 and announces itself over mDNS on your local network. Your
  operating system may ask you to allow this. Devices you have not paired with are refused before
  anything they send is read.
- It keeps running after VS Code closes so items can still arrive. Run **Ferry: Stop
  Daemon** to stop it, and turn off `ferry.autoStartDaemon` to keep it from starting again.
- Your identity key is kept in the operating system's keychain.
- No telemetry. Ferry contacts GitHub only if you turn on its optional GitHub features.

The extension runs on your own computer, even when VS Code is connected to SSH, WSL, or a
container, so it sends and receives files on this computer only.

## Troubleshooting

**A teammate shows as offline.** Both machines must be on the same network with mDNS (UDP 5353)
and TCP 47821 allowed. Guest Wi-Fi and some VPNs block both.

**The daemon won't start.** Run **Ferry: Start Daemon** to see the reason. On Linux, the keychain
needs a running secret service such as GNOME Keyring or KWallet.

[Source and issues](https://github.com/firaslamouchi21/Ferry) ·
[Privacy](https://github.com/firaslamouchi21/Ferry/blob/main/PRIVACY.md) · Apache-2.0
