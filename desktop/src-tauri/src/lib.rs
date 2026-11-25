use std::io::{Read, Write};
use std::sync::Mutex;
use std::time::Duration;

use ferry_net::local_ipc;

use serde_json::Value;
use tauri::menu::{MenuBuilder, MenuItemBuilder, PredefinedMenuItem};
use tauri::tray::{TrayIconBuilder, TrayIconEvent};
use tauri::{Emitter, Manager};

const MAX_FRAME_BYTES: usize = ferry_net::framing::IPC_MAX_FRAME_BYTES;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

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

static SPAWNING: Mutex<()> = Mutex::new(());

fn spawn_daemon() -> Result<u32, String> {
    let _only_one = SPAWNING.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
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
    #[cfg(windows)]
    std::os::windows::process::CommandExt::creation_flags(&mut command, CREATE_NO_WINDOW);
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

#[tauri::command(async)]
fn daemon_running() -> bool {
    daemon_reachable()
}

#[derive(serde::Serialize)]
struct PickedFile {
    path: String,
    name: String,
}

fn picked(path: std::path::PathBuf) -> PickedFile {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    PickedFile { path: path.to_string_lossy().into_owned(), name }
}

#[tauri::command]
async fn pick_file(app: tauri::AppHandle) -> Option<PickedFile> {
    use tauri_plugin_dialog::DialogExt;
    tauri::async_runtime::spawn_blocking(move || app.dialog().file().blocking_pick_file())
        .await
        .ok()
        .flatten()
        .and_then(|f| f.into_path().ok())
        .map(picked)
}

#[tauri::command]
async fn pick_save_path(app: tauri::AppHandle, name: String) -> Option<PickedFile> {
    use tauri_plugin_dialog::DialogExt;
    tauri::async_runtime::spawn_blocking(move || app.dialog().file().set_file_name(name).blocking_save_file())
        .await
        .ok()
        .flatten()
        .and_then(|f| f.into_path().ok())
        .map(picked)
}

#[tauri::command(async)]
fn write_file(path: String, content_base64: String) -> Result<(), String> {
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(content_base64)
        .map_err(|e| e.to_string())?;
    std::fs::write(&path, bytes).map_err(|e| format!("could not write {path}: {e}"))
}

#[tauri::command(async)]
fn start_daemon() -> Result<(), String> {
    spawn_daemon().map(|_| ())
}

fn daemon_status_label(reachable: bool) -> &'static str {
    if reachable {
        "Daemon: running"
    } else {
        "Daemon: not running"
    }
}

fn show_main_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn build_tray(app: &tauri::AppHandle) -> tauri::Result<()> {
    let status_item = MenuItemBuilder::with_id("status", daemon_status_label(daemon_reachable()))
        .enabled(false)
        .build(app)?;
    let show_item = MenuItemBuilder::with_id("show", "Show Ferry").build(app)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let quit_item = MenuItemBuilder::with_id("quit", "Quit").build(app)?;
    let menu = MenuBuilder::new(app)
        .items(&[&status_item, &show_item, &separator, &quit_item])
        .build()?;

    let mut builder = TrayIconBuilder::with_id("ferry-tray").tooltip("Ferry");
    if let Some(icon) = app.default_window_icon().cloned() {
        builder = builder.icon(icon);
    }
    builder
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "show" => show_main_window(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: tauri::tray::MouseButton::Left,
                button_state: tauri::tray::MouseButtonState::Up,
                ..
            } = event
            {
                let app = tray.app_handle();
                if let Some(window) = app.get_webview_window("main") {
                    if window.is_visible().unwrap_or(false) && !window.is_minimized().unwrap_or(false) {
                        let _ = window.hide();
                    } else {
                        show_main_window(app);
                    }
                }
            }
        })
        .build(app)?;

    let app_for_poll = app.clone();
    std::thread::spawn(move || {
        let mut last_reachable = None;
        loop {
            let reachable = daemon_reachable();
            if last_reachable != Some(reachable) {
                let _ = status_item.set_text(daemon_status_label(reachable));
                if let Some(tray) = app_for_poll.tray_by_id("ferry-tray") {
                    let _ = tray.set_tooltip(Some(format!(
                        "Ferry — {}",
                        daemon_status_label(reachable)
                    )));
                }
                last_reachable = Some(reachable);
            }
            std::thread::sleep(Duration::from_secs(2));
        }
    });

    Ok(())
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

#[tauri::command(async)]
fn ipc_request(envelope: Value) -> Result<Value, String> {
    let mut stream = local_ipc::connect(&socket_path()).map_err(|e| e.to_string())?;
    let bytes = serde_json::to_vec(&envelope).map_err(|e| e.to_string())?;
    write_frame(&mut stream, &bytes).map_err(|e| e.to_string())?;
    let response = read_frame(&mut stream).map_err(|e| e.to_string())?;
    serde_json::from_slice(&response).map_err(|e| e.to_string())
}

#[tauri::command(async)]
fn ipc_subscribe(app: tauri::AppHandle, state: tauri::State<DaemonSocket>) -> Result<(), String> {
    let mut guard = state.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(handle) = guard.as_ref() {
        if !handle.is_finished() {
            return Ok(());
        }
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
        let _ = app.emit("ferry://subscribe-closed", ());
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

    #[test]
    fn a_picked_path_carries_its_file_name() {
        let p = picked(std::path::PathBuf::from("/home/me/Downloads/report final.pdf"));
        assert_eq!(p.name, "report final.pdf");
        assert_eq!(p.path, "/home/me/Downloads/report final.pdf");
    }

    #[test]
    fn write_file_round_trips_binary_content() {
        let dir = std::env::temp_dir().join(format!("ferry-desktop-write-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("out.bin");
        let bytes: Vec<u8> = (0..=255u8).collect();
        use base64::Engine;
        let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
        write_file(target.to_string_lossy().into_owned(), b64).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), bytes);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn write_file_rejects_bad_base64_without_touching_disk() {
        let dir = std::env::temp_dir().join(format!("ferry-desktop-badb64-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("never.bin");
        assert!(write_file(target.to_string_lossy().into_owned(), "not base64!!".into()).is_err());
        assert!(!target.exists());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn write_file_reports_an_unwritable_destination() {
        let err = write_file("/proc/ferry-cannot-write-here/x".into(), "AA==".into()).unwrap_err();
        assert!(err.contains("could not write"), "{err}");
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            show_main_window(app);
        }))
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .manage(DaemonSocket(Mutex::new(None)))
        .setup(|app| {
            std::thread::spawn(|| {
                if let Err(err) = spawn_daemon() {
                    eprintln!("ferry-desktop: could not auto-start the daemon: {err}");
                }
            });
            if let Err(err) = build_tray(app.handle()) {
                eprintln!("ferry-desktop: no tray icon on this system: {err}");
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            ipc_request,
            ipc_subscribe,
            daemon_running,
            start_daemon,
            pick_file,
            pick_save_path,
            write_file
        ])
        .run(tauri::generate_context!())
        .expect("error while running Ferry");
}
