//! Tauri shell: exposes the nevar service to the UI as commands and events.

use std::str::FromStr;

use nevar_core::{
    EndpointId,
    service::{AppState, CallStatsView, Event, Service},
    store::Settings,
};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

/// Commands return errors as plain text for the UI to show.
type CmdResult<T = ()> = Result<T, String>;

fn err(e: anyhow::Error) -> String {
    format!("{e:#}")
}

fn parse_id(id: &str) -> CmdResult<EndpointId> {
    EndpointId::from_str(id).map_err(|_| "invalid ID".to_string())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Devices {
    inputs: Vec<String>,
    outputs: Vec<String>,
    default_input: Option<String>,
    default_output: Option<String>,
}

#[tauri::command]
fn get_state(service: State<Service>) -> AppState {
    service.state()
}

#[tauri::command]
fn get_call_stats(service: State<Service>) -> Option<CallStatsView> {
    service.call_stats()
}

#[tauri::command]
fn list_devices(service: State<Service>) -> CmdResult<Devices> {
    let d = service.devices().map_err(err)?;
    Ok(Devices {
        inputs: d.inputs,
        outputs: d.outputs,
        default_input: d.default_input,
        default_output: d.default_output,
    })
}

#[tauri::command]
fn set_name(service: State<Service>, name: String) -> CmdResult {
    service.set_name(&name).map_err(err)
}

// Commands that may open audio devices run async, i.e. on the runtime's worker threads
// rather than the window thread, which Windows audio (COM) doesn't like.

#[tauri::command]
async fn set_settings(service: State<'_, Service>, settings: Settings) -> CmdResult {
    service.set_settings(settings);
    Ok(())
}

#[tauri::command]
fn set_muted(service: State<Service>, muted: bool) {
    service.set_muted(muted);
}

#[tauri::command]
fn set_deafened(service: State<Service>, deafened: bool) {
    service.set_deafened(deafened);
}

#[tauri::command]
fn add_friend(service: State<Service>, code: String) -> CmdResult {
    service.add_friend(&code).map_err(err)
}

#[tauri::command]
fn accept_request(service: State<Service>, id: String) -> CmdResult {
    service.accept_request(&parse_id(&id)?).map_err(err)
}

#[tauri::command]
fn decline_request(service: State<Service>, id: String) -> CmdResult {
    service.decline_request(&parse_id(&id)?).map_err(err)
}

#[tauri::command]
fn remove_contact(service: State<Service>, id: String) -> CmdResult {
    service.remove_contact(&parse_id(&id)?);
    Ok(())
}

#[tauri::command]
fn start_call(service: State<Service>, id: String) -> CmdResult {
    service.call(&parse_id(&id)?).map_err(err)
}

#[tauri::command]
async fn accept_call(service: State<'_, Service>) -> CmdResult {
    service.accept_call().map_err(err)
}

#[tauri::command]
fn decline_call(service: State<Service>) {
    service.decline_call();
}

#[tauri::command]
fn hang_up(service: State<Service>) {
    service.hang_up();
}

fn forward_event(app: &AppHandle, event: Event) {
    match event {
        Event::StateChanged => {
            let _ = app.emit("state-changed", ());
        }
        Event::Notice(notice) => {
            let _ = app.emit("notice", notice);
        }
    }
}

/// Logs go to `nevar.log` next to the app's data, so a friend with a problem can send it over.
/// Debug builds also log to the terminal.
fn init_logging() {
    use tracing_subscriber::{EnvFilter, fmt, prelude::*};

    const MAX_LOG_BYTES: u64 = 5 * 1024 * 1024;
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| "warn,nevar_core=info,nevar=info".into());

    // Same folder Tauri uses for app data (%APPDATA%\<identifier>); setup hasn't run yet.
    let file = std::env::var_os("APPDATA").and_then(|appdata| {
        let dir = std::path::PathBuf::from(appdata).join("chat.nevar.app");
        std::fs::create_dir_all(&dir).ok()?;
        let path = dir.join("nevar.log");
        let too_big = std::fs::metadata(&path).is_ok_and(|m| m.len() > MAX_LOG_BYTES);
        std::fs::OpenOptions::new()
            .create(true)
            .append(!too_big)
            .write(true)
            .truncate(too_big)
            .open(path)
            .ok()
    });

    let file_layer = file.map(|f| fmt::layer().with_ansi(false).with_writer(std::sync::Mutex::new(f)));
    let stderr_layer = cfg!(debug_assertions).then(fmt::layer);
    tracing_subscriber::registry().with(filter).with(file_layer).with(stderr_layer).init();

    // Without a console a panic would vanish; put it in the log with a backtrace.
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        tracing::error!("crash: {info}
{}", std::backtrace::Backtrace::force_capture());
        default_hook(info);
    }));
    tracing::info!("nevar {} starting", env!("CARGO_PKG_VERSION"));
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    init_logging();

    tauri::Builder::default()
        // A second copy would fight the first over the same identity: focus the first instead.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            let handle = app.handle().clone();
            let service = tauri::async_runtime::block_on(Service::start(&data_dir, move |e| {
                forward_event(&handle, e)
            }))?;
            app.manage(service);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_state,
            get_call_stats,
            list_devices,
            set_name,
            set_settings,
            set_muted,
            set_deafened,
            add_friend,
            accept_request,
            decline_request,
            remove_contact,
            start_call,
            accept_call,
            decline_call,
            hang_up,
        ])
        .build(tauri::generate_context!())
        .expect("error while building nevar")
        .run(|app, event| {
            if let tauri::RunEvent::ExitRequested { .. } = event {
                let service = app.state::<Service>().inner().clone();
                tauri::async_runtime::block_on(service.shutdown());
            }
        });
}
