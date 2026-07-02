//! Builder Pro AI — Tauri core (broker). S0 scaffold: plugin init + a smoke command.
//! The daemon-broker command surface (spec §6) is added in T14–T18.

pub mod broker;
pub mod commands;
pub mod launchd;
pub mod paths;
pub mod socket_client;

/// Trivial invoke smoke command; proves the JS⇄Rust IPC round-trip works.
#[tauri::command]
fn ping() -> String {
    "pong".to_string()
}

/// Build and run the Tauri application.
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_store::Builder::default().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_shell::init())
        .invoke_handler(tauri::generate_handler![ping])
        .run(tauri::generate_context!())
        .expect("error while running Builder Pro AI");
}
