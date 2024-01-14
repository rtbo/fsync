use std::process::ExitCode;

use systemd_journal_logger::{connected_to_journal, JournalLog};
use tokio::task::JoinHandle;

use crate::{exit_program, ShutdownRef};

pub fn main() -> ExitCode {
    if connected_to_journal() {
        JournalLog::new()
            .unwrap()
            .add_extra_field("VERSION", env!("CARGO_PKG_VERSION"))
            .install()
            .unwrap();

        log::set_max_level(log::LevelFilter::Info);
    } else {
        env_logger::init();
    }

    let shutdown_res = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async move {
            let shutdown_ref = ShutdownRef::new();
            let shutdown = handle_shutdown_signals(shutdown_ref.clone());
            crate::run(std::env::args_os().collect(), shutdown_ref)
                .await
                .unwrap();
            shutdown.await.unwrap()
        });

    exit_program(shutdown_res)
}

fn handle_shutdown_signals(shutdown_ref: ShutdownRef) -> JoinHandle<anyhow::Result<()>> {
    use tokio::signal::unix::{signal, SignalKind};

    let mut sig_term = signal(SignalKind::terminate()).unwrap();
    let mut sig_int = signal(SignalKind::interrupt()).unwrap();

    tokio::spawn(async move {
        tokio::select! {
            _ = sig_term.recv() => {
                log::warn!("received SIGTERM");
            }
            _ = sig_int.recv() => {
                log::warn!("received SIGINT");
            }
        };
        shutdown_ref.shutdown().await
    })
}
