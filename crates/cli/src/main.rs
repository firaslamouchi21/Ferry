use std::io::{self, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{ExitCode, Stdio};
use std::sync::OnceLock;
use std::time::Duration;

use base64::Engine;
use clap::{Parser, Subcommand};
use ferry_core::pairing;
use ferry_proto::ipc::{
    IpcEnvelope, IpcOutcome, IpcRequest, IpcResponse, IpcResult, RequestId, IPC_PROTOCOL_VERSION,
};

static JSON_OUTPUT: OnceLock<bool> = OnceLock::new();
static SOCKET_OVERRIDE: OnceLock<Option<PathBuf>> = OnceLock::new();

fn json_output() -> bool {
    *JSON_OUTPUT.get().unwrap_or(&false)
}

fn print_json(value: &impl serde::Serialize) {
    println!("{}", serde_json::to_string_pretty(value).unwrap_or_else(|_| "null".into()));
}

const EXIT_UNREACHABLE: u8 = 3;
const EXIT_PROTOCOL: u8 = 4;
const EXIT_FAILURE: u8 = 1;

fn exit_code_for_error(message: &str) -> u8 {
    if message.starts_with("unreachable: ") {
        EXIT_UNREACHABLE
    } else if message.starts_with("protocol: ") {
        EXIT_PROTOCOL
    } else {
        EXIT_FAILURE
    }
}

fn user_facing_error(message: &str) -> &str {
    message
        .strip_prefix("unreachable: ")
        .or_else(|| message.strip_prefix("protocol: "))
        .unwrap_or(message)
}

#[derive(Parser)]
#[command(name = "ferry", about = "Ferry — serverless peer-to-peer file, secret, and message transfer")]
#[command(after_help = "Exit codes: 0 ok · 1 failure · 2 usage · 3 daemon unreachable · 4 protocol error")]
struct Cli {
    #[arg(long, global = true, help = "emit the raw IPC result as JSON and nothing else")]
    json: bool,
    #[arg(long, global = true, help = "path to the daemon's IPC socket")]
    socket: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

const DEFAULT_TTL_SECS: u32 = 86_400;

#[derive(Subcommand)]
enum Command {
    #[command(about = "Show the daemon's health — protocol version, store, transport, discovery")]
    Status,
    #[command(about = "Ask the running daemon to shut down")]
    Quit,
    #[command(about = "Queue a file to send to a rostered peer (delivered out of band)")]
    Send {
        #[arg(help = "the recipient's peer id, or a roster display name that uniquely matches")]
        peer: String,
        path: PathBuf,
        #[arg(long, default_value_t = DEFAULT_TTL_SECS, help = "seconds the item stays readable after delivery")]
        ttl: u32,
        #[arg(long, help = "delete the item after the recipient opens it once")]
        burn: bool,
        #[arg(long, help = "get notified when the recipient opens the item")]
        notify_on_open: bool,
    },
    #[command(about = "Start, stop, or run the local daemon")]
    Daemon {
        #[command(subcommand)]
        command: DaemonCommand,
    },
    #[command(about = "Inspect and share the roster of paired peers")]
    Roster {
        #[command(subcommand)]
        command: RosterCommand,
    },
    #[command(about = "Pair with another device — out-of-band code plus a confirmed verification phrase")]
    Pair {
        #[command(subcommand)]
        command: PairCommand,
    },
    #[command(about = "List and open items other peers have sent you")]
    Receive {
        #[command(subcommand)]
        command: ReceiveCommand,
    },
    #[command(about = "Seal a delivered item to a blob file for offline hand-carry")]
    Export {
        item_id: String,
        path: PathBuf,
    },
    #[command(about = "Import a sealed blob file another peer exported for you")]
    Import {
        path: PathBuf,
        #[arg(long, help = "skip the confirmation prompt")]
        yes: bool,
    },
    #[command(about = "Print this machine's fingerprint and public keys")]
    Identity,
    #[command(about = "List or remove entries in the roster")]
    Peer {
        #[command(subcommand)]
        command: PeerCommand,
    },
    #[command(about = "Review, accept, reject, and open offered items")]
    Inbox {
        #[command(subcommand)]
        command: InboxCommand,
    },
    #[command(about = "Track, abort, and retry items you have queued")]
    Sent {
        #[command(subcommand)]
        command: SentCommand,
    },
    #[command(about = "Print the local audit log — actor, kind, item, time, outcome, never contents")]
    Activity {
        #[arg(long, default_value_t = 200)]
        limit: u32,
        #[arg(long, help = "only events strictly before this unix-millis timestamp")]
        before_millis: Option<i64>,
    },
    #[command(about = "Stream daemon events until interrupted")]
    Watch,
}

#[derive(Subcommand)]
enum DaemonCommand {
    #[command(about = "Spawn the daemon in the background and wait for it to accept connections")]
    Start,
    #[command(about = "Ask the running daemon to shut down")]
    Stop,
    #[command(about = "Run the daemon in the foreground (for a service manager)")]
    Run,
    #[command(about = "Show the daemon's health — protocol version, store, transport, discovery")]
    Status,
}

#[derive(Subcommand)]
enum ReceiveCommand {
    #[command(about = "List delivered items waiting to be opened")]
    List,
    #[command(about = "Open an item once — burns it if the sender marked it burn-after-read")]
    Open {
        item_id: String,
        #[arg(long, help = "write the content to this path instead of printing it")]
        out: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
enum PairCommand {
    #[command(about = "Wait for the other device to connect; prints the code to read to them")]
    Listen {
        #[arg(long, default_value = "0.0.0.0:0")]
        bind: String,
        #[arg(long, help = "the name this device shows up as on the other side")]
        name: String,
    },
    #[command(about = "Connect to a listening device using its address and short code")]
    Connect {
        addr: String,
        code: String,
        #[arg(long, help = "the name this device shows up as on the other side")]
        name: String,
    },
}

#[derive(Subcommand)]
enum RosterCommand {
    #[command(about = "List every paired peer with its reachability")]
    List,
    #[command(about = "Write the signed roster to a file to share with teammates")]
    Export {
        path: PathBuf,
    },
    #[command(about = "Merge a signed roster file after showing its signer and peers")]
    Import {
        path: PathBuf,
        #[arg(long, help = "skip the confirmation prompt")]
        yes: bool,
    },
}

#[derive(Subcommand)]
enum PeerCommand {
    #[command(about = "List every paired peer with its reachability")]
    List,
    #[command(about = "Remove a peer from the roster — it can no longer send or receive")]
    Remove {
        peer_id: String,
        #[arg(long, help = "skip the confirmation prompt")]
        yes: bool,
    },
}

#[derive(Subcommand)]
enum InboxCommand {
    #[command(about = "List items that have arrived, including ones awaiting your decision")]
    List,
    #[command(about = "Accept an offered item so its transfer can proceed")]
    Accept { item_id: String },
    #[command(about = "Reject an offered item and tell the sender")]
    Reject {
        item_id: String,
        #[arg(long, help = "skip the confirmation prompt")]
        yes: bool,
    },
    #[command(about = "Open an item once — burns it if the sender marked it burn-after-read")]
    Open {
        item_id: String,
        #[arg(long, help = "write the content to this path instead of printing it")]
        out: Option<PathBuf>,
    },
    #[command(about = "Show one item's full detail without opening it")]
    Show { item_id: String },
}

#[derive(Subcommand)]
enum SentCommand {
    #[command(about = "List every item you have queued and its delivery state")]
    List {
        #[arg(long, help = "only show items in this state")]
        state: Option<String>,
    },
    #[command(about = "Drop a queued item that has not yet been delivered")]
    Abort {
        item_id: String,
        #[arg(long, help = "skip the confirmation prompt")]
        yes: bool,
    },
    #[command(about = "Re-queue a failed item for another delivery attempt")]
    Retry { item_id: String },
}

fn ipc_call(request: IpcRequest) -> Result<IpcResult, String> {
    let socket_path = match SOCKET_OVERRIDE.get().and_then(|o| o.clone()) {
        Some(path) => path,
        None => {
            let data_dir = ferry_core::config::default_data_dir();
            ferry_core::config::ipc_socket_path(&data_dir)
        }
    };

    let mut stream = ferry_net::local_ipc::connect(&socket_path).map_err(|source| {
        format!(
            "unreachable: could not connect to the ferry daemon at {} — is it running? ({source})",
            socket_path.display()
        )
    })?;

    let envelope = IpcEnvelope {
        ipc_protocol_version: IPC_PROTOCOL_VERSION,
        request_id: RequestId(uuid::Uuid::now_v7().to_string()),
        request,
    };
    let bytes = serde_json::to_vec(&envelope).map_err(|e| e.to_string())?;
    ferry_net::framing::write_frame_with_max(&mut stream, &bytes, ferry_net::framing::IPC_MAX_FRAME_BYTES)
        .map_err(|e| e.to_string())?;
    stream.flush().map_err(|e| e.to_string())?;

    let response_bytes = ferry_net::framing::read_frame_with_max(&mut stream, ferry_net::framing::IPC_MAX_FRAME_BYTES)
        .map_err(|e| e.to_string())?;
    let response: IpcResponse = serde_json::from_slice(&response_bytes).map_err(|e| e.to_string())?;

    match response.outcome {
        IpcOutcome::Ok { value } => Ok(value),
        IpcOutcome::Err { error } => Err(format!("protocol: {:?}: {}", error.code, error.message)),
    }
}

fn emit<T: serde::Serialize>(value: &T, human: impl FnOnce()) {
    if json_output() {
        print_json(value);
    } else {
        human();
    }
}

fn cmd_status() -> Result<(), String> {
    match ipc_call(IpcRequest::Status)? {
        IpcResult::Status(status) => {
            println!("protocol_version: {}", status.protocol_version);
            println!("store_ok:         {}", status.store_ok);
            println!("transport_ok:     {}", status.transport_ok);
            println!("discovery_ok:     {}", status.discovery_ok);
            Ok(())
        }
        other => Err(format!("unexpected response to Status: {other:?}")),
    }
}

fn cmd_quit() -> Result<(), String> {
    match ipc_call(IpcRequest::Quit)? {
        IpcResult::Ack => {
            println!("daemon is shutting down");
            Ok(())
        }
        other => Err(format!("unexpected response to Quit: {other:?}")),
    }
}

fn active_socket_path() -> PathBuf {
    match SOCKET_OVERRIDE.get().and_then(|o| o.clone()) {
        Some(path) => path,
        None => ferry_core::config::ipc_socket_path(&ferry_core::config::default_data_dir()),
    }
}

fn daemon_is_running() -> bool {
    ferry_net::local_ipc::connect(&active_socket_path()).is_ok()
}

fn ferry_daemon_bin() -> PathBuf {
    let name = if cfg!(windows) { "ferry-daemon.exe" } else { "ferry-daemon" };
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join(name)))
        .filter(|sibling| sibling.is_file())
        .unwrap_or_else(|| PathBuf::from(name))
}

fn spawn_daemon(stdout: Stdio, stderr: Stdio) -> Result<std::process::Child, String> {
    let bin = ferry_daemon_bin();
    let mut command = std::process::Command::new(&bin);
    command.stdin(Stdio::null()).stdout(stdout).stderr(stderr);
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut command, 0);
    command.spawn().map_err(|source| {
        format!("could not start {} — is ferry-daemon on your PATH or next to this binary? ({source})", bin.display())
    })
}

fn cmd_daemon_run() -> Result<(), String> {
    let status = spawn_daemon(Stdio::inherit(), Stdio::inherit())?
        .wait()
        .map_err(|e| e.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("ferry-daemon exited with {status}"))
    }
}

fn cmd_daemon_start() -> Result<(), String> {
    if daemon_is_running() {
        println!("ferry-daemon is already running");
        return Ok(());
    }

    let data_dir = ferry_core::config::default_data_dir();
    std::fs::create_dir_all(&data_dir)
        .map_err(|source| format!("failed to create {}: {source}", data_dir.display()))?;
    let log_path = data_dir.join("daemon.log");
    let log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .map_err(|source| format!("failed to open {}: {source}", log_path.display()))?;
    let log_err = log.try_clone().map_err(|e| e.to_string())?;

    let child = spawn_daemon(Stdio::from(log), Stdio::from(log_err))?;
    let pid = child.id();

    for _ in 0..50 {
        if daemon_is_running() {
            println!("ferry-daemon started (pid {pid}), logging to {}", log_path.display());
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Err(format!(
        "ferry-daemon (pid {pid}) did not accept a connection within 5s — see {}",
        log_path.display()
    ))
}

fn cmd_daemon_stop() -> Result<(), String> {
    if !daemon_is_running() {
        println!("ferry-daemon is not running");
        return Ok(());
    }
    cmd_quit()
}

fn looks_like_peer_id(peer: &str) -> bool {
    peer.len() >= 8 && peer.chars().all(|c| c.is_ascii_hexdigit())
}

fn resolve_peer(peer: &str) -> Result<String, String> {
    let peers = match ipc_call(IpcRequest::RosterList)? {
        IpcResult::RosterList(peers) => peers,
        other => return Err(format!("unexpected response to RosterList: {other:?}")),
    };
    if peers.iter().any(|p| p.peer_id == peer) {
        return Ok(peer.to_string());
    }
    let needle = peer.to_lowercase();
    let mut matches = peers
        .iter()
        .filter(|p| p.display_name.to_lowercase() == needle)
        .map(|p| (p.display_name.clone(), p.peer_id.clone()));
    match (matches.next(), matches.next()) {
        (Some((name, id)), None) => {
            if !json_output() {
                eprintln!("ferry: resolved '{name}' to {id}");
            }
            Ok(id)
        }
        (Some(_), Some(_)) => {
            Err(format!("'{peer}' matches more than one peer by name — use the peer id (`ferry roster list`)"))
        }
        (None, _) if looks_like_peer_id(peer) => Ok(peer.to_string()),
        (None, _) => Err(format!(
            "'{peer}' is not a peer id and matches no roster name — pair first or check `ferry roster list`"
        )),
    }
}

fn cmd_send(peer: &str, path: &Path, ttl_secs: u32, is_burn_after_read: bool, notify_on_open: bool) -> Result<(), String> {
    let peer_id = resolve_peer(peer)?;
    let absolute_path = std::fs::canonicalize(path)
        .map_err(|source| format!("failed to read {}: {source}", path.display()))?;
    let name = absolute_path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .ok_or_else(|| format!("{} has no file name", absolute_path.display()))?;

    match ipc_call(IpcRequest::Send {
        peer_id: peer_id.clone(),
        source_path: absolute_path.to_string_lossy().into_owned(),
        name,
        ttl_secs,
        is_burn_after_read,
        notify_on_open,
    })? {
        IpcResult::Send { item_id } => {
            println!("queued {item_id} for {peer_id}");
            Ok(())
        }
        other => Err(format!("unexpected response to Send: {other:?}")),
    }
}

fn cmd_roster_list() -> Result<(), String> {
    match ipc_call(IpcRequest::RosterList)? {
        IpcResult::RosterList(peers) => {
            if peers.is_empty() {
                println!("roster is empty");
                return Ok(());
            }
            for peer in peers {
                println!("{}  {}", peer.peer_id, peer.display_name);
            }
            Ok(())
        }
        other => Err(format!("unexpected response to RosterList: {other:?}")),
    }
}

fn cmd_roster_export(path: &Path) -> Result<(), String> {
    match ipc_call(IpcRequest::RosterExport)? {
        IpcResult::RosterExport { signed_roster_json } => {
            std::fs::write(path, signed_roster_json)
                .map_err(|source| format!("failed to write {}: {source}", path.display()))?;
            println!("exported signed roster to {}", path.display());
            Ok(())
        }
        other => Err(format!("unexpected response to RosterExport: {other:?}")),
    }
}

fn cmd_roster_import(path: &Path, skip_confirm: bool) -> Result<(), String> {
    let signed_roster_json = std::fs::read_to_string(path)
        .map_err(|source| format!("failed to read {}: {source}", path.display()))?;

    let signed: ferry_crypto::roster::SignedRoster = serde_json::from_str(&signed_roster_json)
        .map_err(|source| format!("{} is not a valid roster file: {source}", path.display()))?;
    let entries = signed
        .verify()
        .map_err(|source| format!("roster file failed signature verification: {source}"))?;

    println!("roster signed by {}", signed.signer.0);
    println!("{} peer(s):", entries.len());
    for entry in entries {
        println!("  {}  {}", entry.peer_id.0, entry.display_name);
    }

    if !skip_confirm {
        print!("import these peers? [y/N] ");
        io::stdout().flush().map_err(|e| e.to_string())?;
        let mut answer = String::new();
        io::stdin().read_line(&mut answer).map_err(|e| e.to_string())?;
        if !matches!(answer.trim().to_lowercase().as_str(), "y" | "yes") {
            println!("import cancelled");
            return Ok(());
        }
    }

    match ipc_call(IpcRequest::RosterImport { signed_roster_json })? {
        IpcResult::RosterImport(summary) => {
            println!(
                "imported {} of {} peer(s) ({} already present)",
                summary.added, summary.peer_count, summary.skipped_existing
            );
            Ok(())
        }
        other => Err(format!("unexpected response to RosterImport: {other:?}")),
    }
}

fn confirm_verification_phrase(phrase: &str) -> bool {
    println!("verification phrase: {phrase}");
    print!("does this match what's shown on the other device? [y/N] ");
    if io::stdout().flush().is_err() {
        return false;
    }
    let mut answer = String::new();
    if io::stdin().read_line(&mut answer).is_err() {
        return false;
    }
    matches!(answer.trim().to_lowercase().as_str(), "y" | "yes")
}

fn complete_pairing(outcome: pairing::PairingOutcome) -> Result<(), String> {
    let peer_id = outcome.peer_id.clone();
    let display_name = outcome.display_name.clone();
    match ipc_call(IpcRequest::PairComplete {
        peer_id: outcome.peer_id,
        display_name: outcome.display_name,
        signing_key_hex: outcome.signing_key_hex,
        sealing_key: outcome.sealing_key,
    })? {
        IpcResult::Ack => {
            println!("paired with {display_name} ({peer_id})");
            Ok(())
        }
        other => Err(format!("unexpected response to PairComplete: {other:?}")),
    }
}

fn local_lan_ip() -> Option<std::net::IpAddr> {
    let socket = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("8.8.8.8:80").ok()?;
    socket.local_addr().ok().map(|addr| addr.ip())
}

fn cmd_pair_listen(bind: &str, name: &str) -> Result<(), String> {
    let listener = TcpListener::bind(bind).map_err(|source| format!("failed to bind {bind}: {source}"))?;
    let bound_addr = listener.local_addr().map_err(|e| e.to_string())?;
    let code = pairing::generate_code();

    println!("listening on {bound_addr}");
    println!("on the other device, run:");
    if bound_addr.ip().is_unspecified() {
        match local_lan_ip() {
            Some(ip) => {
                let reachable = std::net::SocketAddr::new(ip, bound_addr.port());
                println!("  ferry pair connect {reachable} {code} --name <this-device-name>");
            }
            None => {
                println!("  ferry pair connect <this-machine's-lan-ip>:{} {code} --name <this-device-name>", bound_addr.port());
                println!("(could not determine a reachable LAN address automatically — use `ip addr`/`ifconfig` to find one)");
            }
        }
    } else {
        println!("  ferry pair connect {bound_addr} {code} --name <this-device-name>");
    }
    println!("waiting for a connection...");

    let (stream, peer_addr) = listener
        .accept()
        .map_err(|source| format!("failed to accept a connection: {source}"))?;
    println!("connection from {peer_addr}");

    let identity = ferry_crypto::identity::Identity::load_or_generate().map_err(|e| e.to_string())?;
    let outcome = pairing::run_pairing_exchange(stream, &code, &identity, name, confirm_verification_phrase)?;
    complete_pairing(outcome)
}

fn cmd_pair_connect(addr: &str, code: &str, name: &str) -> Result<(), String> {
    let stream = TcpStream::connect(addr).map_err(|source| format!("failed to connect to {addr}: {source}"))?;
    let identity = ferry_crypto::identity::Identity::load_or_generate().map_err(|e| e.to_string())?;
    let outcome = pairing::run_pairing_exchange(stream, code, &identity, name, confirm_verification_phrase)?;
    complete_pairing(outcome)
}

fn cmd_receive_list() -> Result<(), String> {
    match ipc_call(IpcRequest::InboxList)? {
        IpcResult::InboxList(items) => {
            if items.is_empty() {
                println!("inbox is empty");
                return Ok(());
            }
            for item in items {
                println!(
                    "{}  {}  from {}  {:?}  {:?}",
                    item.item_id, item.name, item.peer_id, item.kind, item.state
                );
            }
            Ok(())
        }
        other => Err(format!("unexpected response to InboxList: {other:?}")),
    }
}

fn cmd_receive_open(item_id: &str, out: Option<&Path>) -> Result<(), String> {
    let content_base64 = match ipc_call(IpcRequest::Open { item_id: item_id.to_string() })? {
        IpcResult::Open { content_base64 } => content_base64,
        other => return Err(format!("unexpected response to Open: {other:?}")),
    };
    let content = base64::engine::general_purpose::STANDARD
        .decode(&content_base64)
        .map_err(|e| e.to_string())?;

    match out {
        Some(path) => {
            std::fs::write(path, &content)
                .map_err(|source| format!("failed to write {}: {source} — item left unopened, safe to retry", path.display()))?;
            println!("wrote {} bytes to {}", content.len(), path.display());
        }
        None => {
            io::stdout()
                .write_all(&content)
                .map_err(|source| format!("failed to write to stdout: {source} — item left unopened, safe to retry"))?;
        }
    }

    match ipc_call(IpcRequest::ConfirmOpened { item_id: item_id.to_string() })? {
        IpcResult::Ack => Ok(()),
        other => Err(format!("content was delivered but the daemon failed to confirm the open: {other:?}")),
    }
}

fn cmd_export(item_id: &str, path: &Path) -> Result<(), String> {
    match ipc_call(IpcRequest::ExportSealed { item_id: item_id.to_string() })? {
        IpcResult::ExportSealed { blob_base64 } => {
            let blob = base64::engine::general_purpose::STANDARD
                .decode(&blob_base64)
                .map_err(|e| e.to_string())?;
            std::fs::write(path, &blob)
                .map_err(|source| format!("failed to write {}: {source}", path.display()))?;
            println!("sealed {} bytes to {}", blob.len(), path.display());
            println!("carry it to the recipient by any means, then run `ferry import` there");
            Ok(())
        }
        other => Err(format!("unexpected response to ExportSealed: {other:?}")),
    }
}

fn cmd_import(path: &Path, skip_confirm: bool) -> Result<(), String> {
    let blob = std::fs::read(path)
        .map_err(|source| format!("failed to read {}: {source}", path.display()))?;
    let blob_base64 = base64::engine::general_purpose::STANDARD.encode(&blob);

    if !skip_confirm {
        print!("import the sealed item in {}? [y/N] ", path.display());
        io::stdout().flush().map_err(|e| e.to_string())?;
        let mut answer = String::new();
        io::stdin().read_line(&mut answer).map_err(|e| e.to_string())?;
        if !matches!(answer.trim().to_lowercase().as_str(), "y" | "yes") {
            println!("import cancelled");
            return Ok(());
        }
    }

    match ipc_call(IpcRequest::ImportSealed { blob_base64 })? {
        IpcResult::ImportSealed(view) => {
            println!(
                "imported {} ({:?}, {} bytes) from {}",
                view.name, view.kind, view.size_bytes, view.origin_peer_id
            );
            println!("it is now in your inbox — `ferry receive open {}`", view.item_id);
            Ok(())
        }
        other => Err(format!("unexpected response to ImportSealed: {other:?}")),
    }
}

fn confirm(prompt: &str, skip: bool) -> Result<bool, String> {
    if skip {
        return Ok(true);
    }
    if json_output() {
        return Err("refusing an interactive prompt in --json mode; pass --yes".into());
    }
    print!("{prompt} [y/N] ");
    io::stdout().flush().map_err(|e| e.to_string())?;
    let mut answer = String::new();
    io::stdin().read_line(&mut answer).map_err(|e| e.to_string())?;
    Ok(matches!(answer.trim().to_lowercase().as_str(), "y" | "yes"))
}

fn cmd_identity() -> Result<(), String> {
    match ipc_call(IpcRequest::Identity)? {
        IpcResult::Identity(id) => {
            emit(&id, || {
                println!("fingerprint:  {}", id.fingerprint);
                println!("display name: {}", id.display_name);
                println!("signing key:  {}", id.signing_key_hex);
                println!("sealing key:  {}", id.sealing_key);
                println!("listen port:  {}", id.listen_port);
                println!("data dir:     {}", id.data_dir);
                println!("protocol:     v{}", id.protocol_version);
                println!("auto-accept:  {}", id.auto_accept_from_roster);
            });
            Ok(())
        }
        other => Err(format!("unexpected response to Identity: {other:?}")),
    }
}

fn cmd_peer_list() -> Result<(), String> {
    match ipc_call(IpcRequest::RosterList)? {
        IpcResult::RosterList(peers) => {
            emit(&peers, || {
                if peers.is_empty() {
                    println!("roster is empty");
                    return;
                }
                for p in &peers {
                    println!(
                        "{}  {:<20}  {}  {}",
                        p.fingerprint_short,
                        p.display_name,
                        if p.reachable { "online " } else { "offline" },
                        p.peer_id
                    );
                }
            });
            Ok(())
        }
        other => Err(format!("unexpected response to RosterList: {other:?}")),
    }
}

fn cmd_peer_remove(peer_id: &str, yes: bool) -> Result<(), String> {
    if !confirm(&format!("remove peer {peer_id}?"), yes)? {
        println!("cancelled");
        return Ok(());
    }
    match ipc_call(IpcRequest::PeerRemove { peer_id: peer_id.to_string() })? {
        IpcResult::Ack => {
            emit(&serde_json::json!({ "removed": peer_id }), || println!("removed {peer_id}"));
            Ok(())
        }
        other => Err(format!("unexpected response to PeerRemove: {other:?}")),
    }
}

fn cmd_inbox_accept(item_id: &str) -> Result<(), String> {
    match ipc_call(IpcRequest::InboxAccept { item_id: item_id.to_string() })? {
        IpcResult::Ack => {
            emit(&serde_json::json!({ "accepted": item_id }), || println!("accepted {item_id}"));
            Ok(())
        }
        other => Err(format!("unexpected response to InboxAccept: {other:?}")),
    }
}

fn cmd_inbox_reject(item_id: &str, yes: bool) -> Result<(), String> {
    if !confirm(&format!("reject item {item_id}?"), yes)? {
        println!("cancelled");
        return Ok(());
    }
    match ipc_call(IpcRequest::InboxReject { item_id: item_id.to_string() })? {
        IpcResult::Ack => {
            emit(&serde_json::json!({ "rejected": item_id }), || println!("rejected {item_id}"));
            Ok(())
        }
        other => Err(format!("unexpected response to InboxReject: {other:?}")),
    }
}

fn cmd_inbox_show(item_id: &str) -> Result<(), String> {
    let items = match ipc_call(IpcRequest::InboxList)? {
        IpcResult::InboxList(items) => items,
        other => return Err(format!("unexpected response to InboxList: {other:?}")),
    };
    let item = items
        .into_iter()
        .find(|i| i.item_id == item_id)
        .ok_or_else(|| format!("protocol: ItemNotFound: no such inbox item: {item_id}"))?;
    emit(&item, || {
        println!("item:     {}", item.item_id);
        println!("name:     {}", item.name);
        println!("from:     {} ({})", item.origin_display_name, item.peer_id);
        println!("kind:     {:?}", item.kind);
        println!("state:    {:?}", item.state);
        println!("size:     {} bytes", item.size_bytes);
        println!("burn:     {}", item.is_burn_after_read);
        if let Some(h) = &item.hash_hex {
            println!("sha-256:  {h}");
        }
    });
    Ok(())
}

fn cmd_sent_list(state: Option<&str>) -> Result<(), String> {
    match ipc_call(IpcRequest::SentList)? {
        IpcResult::SentList(items) => {
            let filtered: Vec<_> = items
                .into_iter()
                .filter(|i| state.map(|s| format!("{:?}", i.state).to_lowercase() == s.to_lowercase()).unwrap_or(true))
                .collect();
            emit(&filtered, || {
                if filtered.is_empty() {
                    println!("outbox is empty");
                    return;
                }
                for i in &filtered {
                    println!(
                        "{}  {:<24}  @{:<16}  {:<11}  {}",
                        i.item_id,
                        i.name,
                        i.peer_display_name,
                        format!("{:?}", i.state).to_lowercase(),
                        i.last_error.clone().unwrap_or_default()
                    );
                }
            });
            Ok(())
        }
        other => Err(format!("unexpected response to SentList: {other:?}")),
    }
}

fn cmd_sent_abort(item_id: &str, yes: bool) -> Result<(), String> {
    if !confirm(&format!("abort queued item {item_id}?"), yes)? {
        println!("cancelled");
        return Ok(());
    }
    match ipc_call(IpcRequest::SentAbort { item_id: item_id.to_string() })? {
        IpcResult::Ack => {
            emit(&serde_json::json!({ "aborted": item_id }), || println!("aborted {item_id}"));
            Ok(())
        }
        other => Err(format!("unexpected response to SentAbort: {other:?}")),
    }
}

fn cmd_sent_retry(item_id: &str) -> Result<(), String> {
    match ipc_call(IpcRequest::SentRetry { item_id: item_id.to_string() })? {
        IpcResult::Ack => {
            emit(&serde_json::json!({ "retried": item_id }), || println!("re-queued {item_id}"));
            Ok(())
        }
        other => Err(format!("unexpected response to SentRetry: {other:?}")),
    }
}

fn cmd_activity(limit: u32, before_millis: Option<i64>) -> Result<(), String> {
    match ipc_call(IpcRequest::AuditList { limit, before_millis })? {
        IpcResult::AuditList(events) => {
            emit(&events, || {
                for e in &events {
                    println!(
                        "{:>16}  {:<12}  {:<22}  {:<12}  {}",
                        e.occurred_at_millis,
                        e.actor,
                        e.kind,
                        e.item_id.clone().unwrap_or_else(|| "—".into()),
                        e.outcome
                    );
                }
            });
            Ok(())
        }
        other => Err(format!("unexpected response to AuditList: {other:?}")),
    }
}

fn cmd_watch() -> Result<(), String> {
    let socket_path = match SOCKET_OVERRIDE.get().and_then(|o| o.clone()) {
        Some(path) => path,
        None => ferry_core::config::ipc_socket_path(&ferry_core::config::default_data_dir()),
    };
    let mut stream = ferry_net::local_ipc::connect(&socket_path)
        .map_err(|e| format!("unreachable: could not connect to the ferry daemon: {e}"))?;
    let envelope = IpcEnvelope {
        ipc_protocol_version: IPC_PROTOCOL_VERSION,
        request_id: RequestId(uuid::Uuid::now_v7().to_string()),
        request: IpcRequest::Subscribe,
    };
    let bytes = serde_json::to_vec(&envelope).map_err(|e| e.to_string())?;
    ferry_net::framing::write_frame_with_max(&mut stream, &bytes, ferry_net::framing::IPC_MAX_FRAME_BYTES)
        .map_err(|e| e.to_string())?;
    stream.flush().map_err(|e| e.to_string())?;
    eprintln!("watching daemon events — Ctrl-C to stop");
    loop {
        let frame = ferry_net::framing::read_frame_with_max(&mut stream, ferry_net::framing::IPC_MAX_FRAME_BYTES)
            .map_err(|e| e.to_string())?;
        let event: serde_json::Value = serde_json::from_slice(&frame).map_err(|e| e.to_string())?;
        if json_output() {
            println!("{event}");
        } else {
            println!("{}", serde_json::to_string(&event).unwrap_or_default());
        }
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let _ = JSON_OUTPUT.set(cli.json);
    let _ = SOCKET_OVERRIDE.set(cli.socket);

    let result = match cli.command {
        Command::Status => cmd_status(),
        Command::Quit => cmd_quit(),
        Command::Send { peer, path, ttl, burn, notify_on_open } => {
            cmd_send(&peer, &path, ttl, burn, notify_on_open)
        }
        Command::Daemon { command } => match command {
            DaemonCommand::Start => cmd_daemon_start(),
            DaemonCommand::Stop => cmd_daemon_stop(),
            DaemonCommand::Run => cmd_daemon_run(),
            DaemonCommand::Status => cmd_status(),
        },
        Command::Roster { command } => match command {
            RosterCommand::List => cmd_roster_list(),
            RosterCommand::Export { path } => cmd_roster_export(&path),
            RosterCommand::Import { path, yes } => cmd_roster_import(&path, yes),
        },
        Command::Pair { command } => match command {
            PairCommand::Listen { bind, name } => cmd_pair_listen(&bind, &name),
            PairCommand::Connect { addr, code, name } => cmd_pair_connect(&addr, &code, &name),
        },
        Command::Receive { command } => match command {
            ReceiveCommand::List => cmd_receive_list(),
            ReceiveCommand::Open { item_id, out } => cmd_receive_open(&item_id, out.as_deref()),
        },
        Command::Export { item_id, path } => cmd_export(&item_id, &path),
        Command::Import { path, yes } => cmd_import(&path, yes),
        Command::Identity => cmd_identity(),
        Command::Peer { command } => match command {
            PeerCommand::List => cmd_peer_list(),
            PeerCommand::Remove { peer_id, yes } => cmd_peer_remove(&peer_id, yes),
        },
        Command::Inbox { command } => match command {
            InboxCommand::List => cmd_receive_list(),
            InboxCommand::Accept { item_id } => cmd_inbox_accept(&item_id),
            InboxCommand::Reject { item_id, yes } => cmd_inbox_reject(&item_id, yes),
            InboxCommand::Open { item_id, out } => cmd_receive_open(&item_id, out.as_deref()),
            InboxCommand::Show { item_id } => cmd_inbox_show(&item_id),
        },
        Command::Sent { command } => match command {
            SentCommand::List { state } => cmd_sent_list(state.as_deref()),
            SentCommand::Abort { item_id, yes } => cmd_sent_abort(&item_id, yes),
            SentCommand::Retry { item_id } => cmd_sent_retry(&item_id),
        },
        Command::Activity { limit, before_millis } => cmd_activity(limit, before_millis),
        Command::Watch => cmd_watch(),
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("ferry: {}", user_facing_error(&message));
            ExitCode::from(exit_code_for_error(&message))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_connection_failure_maps_to_the_unreachable_exit_code() {
        let msg = "unreachable: could not connect to the ferry daemon at /x — is it running? (nope)";
        assert_eq!(exit_code_for_error(msg), EXIT_UNREACHABLE);
        assert_eq!(user_facing_error(msg), "could not connect to the ferry daemon at /x — is it running? (nope)");
    }

    #[test]
    fn a_protocol_error_maps_to_the_protocol_exit_code() {
        let msg = "protocol: RosterInvalid: signature did not verify";
        assert_eq!(exit_code_for_error(msg), EXIT_PROTOCOL);
        assert_eq!(user_facing_error(msg), "RosterInvalid: signature did not verify");
    }

    #[test]
    fn any_other_error_maps_to_a_plain_failure_and_is_shown_verbatim() {
        let msg = "the roster file does not exist";
        assert_eq!(exit_code_for_error(msg), EXIT_FAILURE);
        assert_eq!(user_facing_error(msg), msg);
    }

    #[test]
    fn the_four_exit_codes_are_distinct() {
        let codes = [0u8, EXIT_FAILURE, EXIT_UNREACHABLE, EXIT_PROTOCOL];
        let unique: std::collections::BTreeSet<u8> = codes.iter().copied().collect();
        assert_eq!(unique.len(), codes.len(), "exit codes 0/1/3/4 must stay distinct");
    }

    fn parse(args: &[&str]) -> Cli {
        match Cli::try_parse_from(args) {
            Ok(cli) => cli,
            Err(e) => panic!("expected {args:?} to parse: {e}"),
        }
    }

    #[test]
    fn bad_usage_is_rejected_by_the_parser_with_the_usage_exit_class() {
        match Cli::try_parse_from(["ferry", "no-such-command"]) {
            Ok(_) => panic!("an unknown subcommand must not parse"),
            Err(e) => assert_eq!(e.exit_code(), 2, "clap reserves exit code 2 for usage errors"),
        }
    }

    #[test]
    fn global_json_and_socket_flags_parse_after_the_subcommand() {
        let cli = parse(&["ferry", "status", "--json", "--socket", "/tmp/f.sock"]);
        assert!(cli.json);
        assert_eq!(cli.socket.as_deref(), Some(Path::new("/tmp/f.sock")));
        assert!(matches!(cli.command, Command::Status));
    }

    #[test]
    fn send_parses_its_flags_and_defaults_the_ttl() {
        let cli = parse(&["ferry", "send", "peer-1", "/tmp/secrets.env", "--burn"]);
        assert!(matches!(
            cli.command,
            Command::Send { ref peer, ref path, ttl, burn: true, notify_on_open: false }
                if peer == "peer-1" && path == Path::new("/tmp/secrets.env") && ttl == DEFAULT_TTL_SECS
        ));
    }

    #[test]
    fn daemon_subcommands_parse() {
        assert!(matches!(
            parse(&["ferry", "daemon", "start"]).command,
            Command::Daemon { command: DaemonCommand::Start }
        ));
        assert!(matches!(
            parse(&["ferry", "daemon", "run"]).command,
            Command::Daemon { command: DaemonCommand::Run }
        ));
    }

    #[test]
    fn nested_subcommands_round_trip_through_the_parser() {
        let cli = parse(&["ferry", "inbox", "reject", "item-7", "--yes"]);
        assert!(matches!(
            cli.command,
            Command::Inbox { command: InboxCommand::Reject { item_id, yes: true } } if item_id == "item-7"
        ));
    }
}
