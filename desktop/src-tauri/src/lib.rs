use std::io::{Read, Write};
use std::sync::Mutex;

use ferry_net::local_ipc::{self, Stream};

use serde_json::Value;
use tauri::{Emitter, Manager};

const MAX_FRAME_BYTES: usize = 65_535;

struct DaemonSocket(Mutex<Option<std::thread::JoinHandle<()>>>);

fn socket_path() -> std::path::PathBuf {
    let data_dir = ferry_core::config::default_data_dir();
    ferry_core::config::ipc_socket_path(&data_dir)
}

fn write_frame(stream: &mut Stream, payload: &[u8]) -> std::io::Result<()> {
    stream.write_all(&(payload.len() as u32).to_be_bytes())?;
    stream.write_all(payload)?;
    stream.flush()
}

fn read_frame(stream: &mut Stream) -> std::io::Result<Vec<u8>> {
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
        .invoke_handler(tauri::generate_handler![ipc_request, ipc_subscribe])
        .run(tauri::generate_context!())
        .expect("error while running Ferry");
}
