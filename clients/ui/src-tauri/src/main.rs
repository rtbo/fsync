// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use daemon::Daemon;
use serde::Serialize;

mod daemon;
mod instances;

#[derive(Debug, Clone, Serialize)]
struct AutoConnectDone();

#[tokio::main]
async fn main() {
    tauri::async_runtime::set(tokio::runtime::Handle::current());

    let daemon = Daemon::default();

    let auto_connect = {
        let daemon = daemon.clone();
        tauri::async_runtime::spawn(async move {
            daemon.try_auto_connect().await;
        })
    };

    let app = tauri::Builder::default()
        .manage(daemon)
        .invoke_handler(tauri::generate_handler![
            instances::instances_get_all,
            daemon::daemon_connected
        ])
        .build(tauri::generate_context!())
        .expect("tauri builder should not fail");

    let _ = auto_connect.await;

    app.run(|_, _| ());
}
