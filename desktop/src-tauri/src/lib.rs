use std::io::{Read, Write};
use std::sync::Mutex;
use std::time::Duration;

use ferry_net::local_ipc;

use serde_json::Value;
use tauri::{Emitter, Manager};

const MAX_FRAME_BYTES: usize = ferry_net::framing::IPC_MAX_FRAME_BYTES;

struct DaemonSocket(Mutex<Option<std::thread::JoinHandle<()>>>);

fn socket_path() -> std::path::PathBuf {
    let data_dir = ferry_core::config::default_data_dir();
    ferry_core::config::ipc_socket_path(&data_dir)
}

fn daemon_bin() -> std::path::PathBuf {
    let name = if cfg!(windows) { "ferry-daemon.exe" } else { "ferry-daemon" };
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join(name)))
        .filter(|sibling| sibling.is_file())
        .unwrap_or_else(|| std::path::PathBuf::from(name))
}

fn daemon_reachable() -> bool {
    local_ipc::connect(&socket_path()).is_ok()
}

fn spawn_daemon() -> Result<u32, String> {
    if daemon_reachable() {
        return Ok(0);
    }
    let bin = daemon_bin();
    let mut command = std::process::Command::new(&bin);
    command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut command, 0);
    let child = command
        .spawn()
        .map_err(|e| format!("could not start {}: {e}", bin.display()))?;
    let pid = child.id();
    for _ in 0..50 {
        if daemon_reachable() {
            return Ok(pid);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Err(format!("ferry-daemon (pid {pid}) did not accept a connection within 5s"))
}

#[tauri::command]
fn daemon_running() -> bool {
    daemon_reachable()
}

#[tauri::command]
fn start_daemon() -> Result<(), String> {
    spawn_daemon().map(|_| ())
}

fn write_frame<W: Write>(stream: &mut W, payload: &[u8]) -> std::io::Result<()> {
    stream.write_all(&(payload.len() as u32).to_be_bytes())?;
    stream.write_all(payload)?;
    stream.flush()
}

fn read_frame<R: Read>(stream: &mut R) -> std::io::Result<Vec<u8>> {
    let mut len = [0u8; 4];
    stream.read_exact(&mut len)?;
    let n = u32::from_be_bytes(len) as usize;
    if n > MAX_FRAME_BYTES {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "oversize frame"));
    }
    let mut body = vec![0u8; n];
    stream.read_exact(&mut body)?;
    Ok(body)
}

#[tauri::command]
fn ipc_request(envelope: Value) -> Result<Value, String> {
    let mut stream = local_ipc::connect(&socket_path()).map_err(|e| e.to_string())?;
    let bytes = serde_json::to_vec(&envelope).map_err(|e| e.to_string())?;
    write_frame(&mut stream, &bytes).map_err(|e| e.to_string())?;
    let response = read_frame(&mut stream).map_err(|e| e.to_string())?;
    serde_json::from_slice(&response).map_err(|e| e.to_string())
}

#[tauri::command]
fn ipc_subscribe(app: tauri::AppHandle, state: tauri::State<DaemonSocket>) -> Result<(), String> {
    let mut guard = state.0.lock().unwrap();
    if guard.is_some() {
        return Ok(());
    }
    let mut stream = local_ipc::connect(&socket_path()).map_err(|e| e.to_string())?;
    let envelope = serde_json::json!({
        "ipc_protocol_version": 1,
        "request_id": "tauri-sub",
        "request": { "method": "subscribe" }
    });
    write_frame(&mut stream, &serde_json::to_vec(&envelope).unwrap()).map_err(|e| e.to_string())?;
    let handle = std::thread::spawn(move || {
        while let Ok(frame) = read_frame(&mut stream) {
            if let Ok(value) = serde_json::from_slice::<Value>(&frame) {
                let _ = app.emit("ferry://event", value);
            }
        }
    });
    *guard = Some(handle);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn a_frame_round_trips_through_the_length_prefix() {
        let mut buf = Vec::new();
        write_frame(&mut buf, b"{\"request\":{\"method\":\"status\"}}").unwrap();
        assert_eq!(u32::from_be_bytes(buf[..4].try_into().unwrap()) as usize, buf.len() - 4);

        let mut cursor = Cursor::new(buf);
        assert_eq!(read_frame(&mut cursor).unwrap(), b"{\"request\":{\"method\":\"status\"}}");
    }

    #[test]
    fn an_oversize_declared_frame_is_rejected_before_the_body_is_read() {
        let mut framed = Vec::new();
        framed.extend_from_slice(&((MAX_FRAME_BYTES as u32) + 1).to_be_bytes());
        let mut cursor = Cursor::new(framed);
        let err = read_frame(&mut cursor).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
    }

    #[test]
    fn the_ipc_socket_path_follows_the_shared_config_helper() {
        assert_eq!(
            socket_path(),
            ferry_core::config::ipc_socket_path(&ferry_core::config::default_data_dir())
        );
    }

    #[test]
    fn the_daemon_binary_resolves_to_a_sibling_or_bare_name() {
        let bin = daemon_bin();
        let name = if cfg!(windows) { "ferry-daemon.exe" } else { "ferry-daemon" };
        assert_eq!(bin.file_name().unwrap(), name);
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .manage(DaemonSocket(Mutex::new(None)))
        .setup(|_app| {
            std::thread::spawn(|| {
                if let Err(err) = spawn_daemon() {
                    eprintln!("ferry-desktop: could not auto-start the daemon: {err}");
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            ipc_request,
            ipc_subscribe,
            daemon_running,
            start_daemon
        ])
        .run(tauri::generate_context!())
        .expect("error while running Ferry");
}
