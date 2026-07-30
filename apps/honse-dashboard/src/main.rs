//! honse-dashboard entry point.
//!
//! Startup order: CLI → logging → single-instance guard → WebView2 policy →
//! storage → auth token → ingest runtime (own thread + tokio runtime) →
//! Dioxus window on the main thread. Shutdown reverses it with bounded waits.

// Hide the console window in release builds; logs go to the data-root files.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;

use std::net::{Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use honse_dashboard::ingest::{self, IngestConfig};
use honse_dashboard::{platform, storage, APP_NAME, APP_VERSION, INGEST_PROTOCOL};

/// Default ingest port (matches the DLL transport default).
const DEFAULT_PORT: u16 = 8716;
/// Windows named-mutex identifier for the single-instance guard.
const SINGLE_INSTANCE_NAME: &str = "Local\\dreki-gg-honse-dashboard";
/// Bounded wait for the ingest runtime to drain on shutdown.
const SHUTDOWN_WAIT: Duration = Duration::from_secs(3);

struct CliArgs {
    data_root: Option<PathBuf>,
    port: u16,
}

fn parse_args() -> Result<Option<CliArgs>> {
    let mut args = std::env::args().skip(1);
    let mut parsed = CliArgs {
        data_root: None,
        port: DEFAULT_PORT,
    };
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--version" | "-V" => {
                println!("{APP_NAME} {APP_VERSION} (ingest protocol v{INGEST_PROTOCOL})");
                return Ok(None);
            }
            "--data-root" => {
                let value = args.next().ok_or_else(|| anyhow!("--data-root requires a path"))?;
                parsed.data_root = Some(PathBuf::from(value));
            }
            "--port" => {
                let value = args.next().ok_or_else(|| anyhow!("--port requires a number"))?;
                parsed.port = value.parse().context("--port must be a valid port number")?;
            }
            "--help" | "-h" => {
                println!(
                    "{APP_NAME} {APP_VERSION}\n\nUSAGE:\n  {APP_NAME} [--data-root <dir>] [--port <port>] [--version]\n\nThe auth token is read from the {} environment variable or install.json in the data root.",
                    platform::TOKEN_ENV
                );
                return Ok(None);
            }
            other => return Err(anyhow!("unknown argument: {other}")),
        }
    }
    Ok(Some(parsed))
}

fn init_logging(data_root: &std::path::Path) -> Option<tracing_appender::non_blocking::WorkerGuard> {
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;
    use tracing_subscriber::{fmt, EnvFilter};

    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let stderr_layer = fmt::layer().with_writer(std::io::stderr).with_target(false);

    let log_dir = platform::log_dir(data_root);
    let file_layer = std::fs::create_dir_all(&log_dir).ok().map(|()| {
        let appender = tracing_appender::rolling::daily(&log_dir, "honse-dashboard.log");
        tracing_appender::non_blocking(appender)
    });

    match file_layer {
        Some((writer, guard)) => {
            tracing_subscriber::registry()
                .with(filter)
                .with(stderr_layer)
                .with(fmt::layer().with_writer(writer).with_ansi(false).with_target(false))
                .init();
            Some(guard)
        }
        None => {
            tracing_subscriber::registry().with(filter).with(stderr_layer).init();
            None
        }
    }
}

/// Release builds use the windows subsystem (no console), which would swallow
/// `--version`/`--help` output. Attaching to the parent console restores CLI
/// behavior when launched from a terminal; a no-op when double-clicked.
#[cfg(windows)]
fn attach_parent_console() {
    use windows_sys::Win32::System::Console::{AttachConsole, ATTACH_PARENT_PROCESS};
    // SAFETY: plain Win32 call; failure (no parent console) is fine to ignore.
    unsafe { AttachConsole(ATTACH_PARENT_PROCESS) };
}

#[cfg(not(windows))]
fn attach_parent_console() {}

fn main() -> ExitCode {
    attach_parent_console();
    let args = match parse_args() {
        Ok(Some(args)) => args,
        Ok(None) => return ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("{err}");
            return ExitCode::from(2);
        }
    };

    let data_root = match platform::resolve_data_root(args.data_root.as_deref()) {
        Ok(root) => root,
        Err(err) => {
            eprintln!("{err}");
            return ExitCode::from(2);
        }
    };
    if let Err(err) = std::fs::create_dir_all(&data_root) {
        eprintln!("cannot create data root {}: {err}", data_root.display());
        return ExitCode::from(2);
    }

    let _log_guard = init_logging(&data_root);
    tracing::info!(version = APP_VERSION, data_root = %data_root.display(), "starting");

    // Single instance: the second copy logs and exits cleanly so the plugin's
    // launcher never stacks windows.
    let _instance_guard = match platform::acquire_single_instance(SINGLE_INSTANCE_NAME) {
        Ok(Some(guard)) => guard,
        Ok(None) => {
            tracing::info!("another instance is already running; exiting");
            return ExitCode::SUCCESS;
        }
        Err(err) => {
            tracing::error!(error = %err, "single-instance guard failed");
            return ExitCode::from(2);
        }
    };

    // Explicit WebView2 policy: without the runtime we show a native dialog
    // pointing at the Microsoft installer and exit with a distinct code.
    match platform::detect_webview2() {
        platform::WebViewRuntime::Available(v) => tracing::info!(webview2 = %v, "webview2 runtime found"),
        platform::WebViewRuntime::Missing => {
            tracing::error!("webview2 runtime missing");
            platform::show_error_dialog(
                "Honse Tracker — WebView2 required",
                "The Microsoft Edge WebView2 runtime is not installed, so the dashboard window cannot be created.\n\nInstall it from https://developer.microsoft.com/microsoft-edge/webview2/ and start Honse Tracker again.\n\nTurn ingestion is unaffected the next time the sidecar runs.",
            );
            return ExitCode::from(3);
        }
    }

    match run(&data_root, args.port) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            tracing::error!(error = %err, "fatal");
            ExitCode::FAILURE
        }
    }
}

fn run(data_root: &std::path::Path, port: u16) -> Result<()> {
    let storage = storage::open(&platform::db_path(data_root))?;
    let token = platform::load_or_create_token(data_root)?;

    let bind = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let (events_tx, events_rx) = tokio::sync::mpsc::unbounded_channel();
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();

    // The ingest runtime lives on its own thread with its own tokio runtime so
    // the Dioxus event loop and database work never contend with it.
    let ingest_storage = storage.clone();
    let ingest_config = IngestConfig {
        bind,
        auth_token: token,
    };
    let ingest_thread = std::thread::Builder::new()
        .name("honse-ingest".to_string())
        .spawn(move || -> Result<()> {
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .context("build ingest tokio runtime")?;
            runtime.block_on(ingest::serve_with_shutdown(
                ingest_config,
                ingest_storage,
                events_tx,
                async {
                    let _ = shutdown_rx.await;
                },
            ))?;
            // Give in-flight handlers a bounded window to finish.
            runtime.shutdown_timeout(SHUTDOWN_WAIT);
            Ok(())
        })
        .context("spawn ingest thread")?;

    // Blocks until the window closes.
    app::run(app::LaunchContext {
        storage,
        events: events_rx,
        listen_addr: bind,
        data_root: data_root.to_path_buf(),
    });

    // Graceful, bounded shutdown of the ingest runtime.
    tracing::info!("window closed; stopping ingest");
    let _ = shutdown_tx.send(());
    match ingest_thread.join() {
        Ok(Ok(())) => {}
        Ok(Err(err)) => tracing::warn!(error = %err, "ingest exited with error"),
        Err(_) => tracing::warn!("ingest thread panicked"),
    }
    tracing::info!("shutdown complete");
    Ok(())
}
