use std::ffi::OsString;
use std::process::ExitCode;
use std::sync::Arc;

use tokio::task::JoinHandle;
use windows_service::{
    define_windows_service,
    service::ServiceControl,
    service_control_handler::{self, ServiceControlHandlerResult},
    service_dispatcher,
};

use crate::{exit_program, ShutdownRef};

pub fn main() -> ExitCode {
    use windows_service::Error;
    // Register generated `ffi_fsyncd_main` with the system and start the service, blocking
    // this thread until the service is stopped.
    match service_dispatcher::start("fsyncd", ffi_service_main) {
        Ok(()) => ExitCode::SUCCESS,
        Err(Error::Winapi(err)) if err.raw_os_error() == Some(1063) => {
            // 1063 is "can't connect to service controller"
            // we are apparently not in a service, start the regular console handler
            console_main()
        }
        Err(err) => {
            log::error!("Windows service error: {err}");
            ExitCode::FAILURE
        }
    }
}

define_windows_service!(ffi_service_main, service_main);

fn service_main(args: Vec<OsString>) {
    // The entry point where execution will start on a background thread after a call to
    // `service_dispatcher::start` from `main`.

    eventlog::init("FSyncd", log::Level::Info).unwrap();

    let shutdown_ref = ShutdownRef::new();

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let rt = Arc::new(rt);

    {
        let rt = rt.clone();
        let shutdown_ref = shutdown_ref.clone();

        let event_handler = move |control_event| -> ServiceControlHandlerResult {
            match control_event {
                // All services must accept Interrogate even if it's a no-op.
                ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
                ServiceControl::Stop => {
                    let res = rt.block_on(shutdown_ref.shutdown());
                    if res.is_err() {
                        let err = res.unwrap_err();
                        log::error!("Error during service shutdown: {err}");
                    }
                    // must return NoError anyway
                    ServiceControlHandlerResult::NoError
                }
                _ => ServiceControlHandlerResult::NotImplemented,
            }
        };
        service_control_handler::register("myservice", event_handler).unwrap();
    }

    rt.block_on(async move { crate::run(args, shutdown_ref).await })
        .unwrap();
}

fn console_main() -> ExitCode {
    env_logger::init();

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let shutdown_res = rt.block_on(async {
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
    let mut sig_c = tokio::signal::windows::ctrl_c().unwrap();
    let mut sig_break = tokio::signal::windows::ctrl_break().unwrap();
    let mut sig_close = tokio::signal::windows::ctrl_close().unwrap();
    let mut sig_logoff = tokio::signal::windows::ctrl_logoff().unwrap();
    let mut sig_shutdown = tokio::signal::windows::ctrl_shutdown().unwrap();

    tokio::spawn(async move {
        tokio::select! {
            _ = sig_c.recv() => {
                log::warn!("received CTRL-C");
            },
            _ = sig_break.recv() => {
                log::warn!("received CTRL-BREAK");
            },
            _ = sig_close.recv() => {
                log::warn!("received CLOSE");
            },
            _ = sig_logoff.recv() => {
                log::warn!("received LOGOFF");
            },
            _ = sig_shutdown.recv() => {
                log::warn!("received SHUTDOWN");
            },
        };
        shutdown_ref.shutdown().await
    })
}
