// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use daemon::Daemon;
use fsync::path::FsPathBuf;
use fsync_client::ts;
use serde::Serialize;

mod daemon;

#[tauri::command]
fn error_message(err: fsync::Error) -> String {
    err.to_string()
}

#[tauri::command]
async fn instance_get_all() -> fsync::Result<Vec<ts::Instance>> {
    ts::Instance::get_all().await
}

#[tauri::command]
async fn instance_create(
    name: String,
    local_dir: FsPathBuf,
    opts: fsync_client::config::ProviderOpts,
) -> fsync::Result<()> {
    fsync_client::config::create(&name, &local_dir, &opts).await?;
    Ok(())
}

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
            error_message,
            instance_get_all,
            instance_create,
            daemon::daemon_connected,
            daemon::daemon_instance_name,
            daemon::daemon_connect,
            daemon::daemon_node_and_children,
            daemon::daemon_operate,
            daemon::daemon_progress,
            daemon::daemon_progresses,
        ])
        .build(tauri::generate_context!())
        .expect("tauri builder should not fail");

    let _ = auto_connect.await;

    app.run(|_, _| ());
}
