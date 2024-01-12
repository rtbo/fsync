use tokio::task::JoinHandle;

use crate::ShutdownRef;

pub fn main() {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async move {
            let shutdown_ref = ShutdownRef::new();
            let shutdown = handle_shutdown_signals(shutdown_ref.clone());
            crate::run(std::env::args_os().collect(), shutdown_ref)
                .await
                .unwrap();
            shutdown.await.unwrap();
        });
}

fn handle_shutdown_signals(shutdown_ref: ShutdownRef) -> JoinHandle<()> {
    use tokio::signal::unix::{signal, SignalKind};

    let mut sig_term = signal(SignalKind::terminate()).unwrap();
    let mut sig_int = signal(SignalKind::interrupt()).unwrap();

    tokio::spawn(async move {
        tokio::select! {
            _ = sig_term.recv() => (),
            _ = sig_int.recv() => (),
        };
        shutdown_ref.shutdown().await;
    })
}
