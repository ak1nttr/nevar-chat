//! Tauri shell: exposes the nevar service to the UI as commands and events.

mod overlay;

use std::str::FromStr;

use nevar_core::{
    EndpointId,
    hotkeys::Hotkeys,
    service::{AppState, CallStatsView, Event, MessageView, MicTestView, Service},
    store::Settings,
};
use serde::Serialize;
use tauri::{
    AppHandle, Emitter, Manager, State, WindowEvent,
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
};

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
async fn set_settings(app: AppHandle, service: State<'_, Service>, settings: Settings) -> CmdResult {
    service.set_settings(settings);
    apply_shortcuts(&app);
    overlay::sync(&app);
    Ok(())
}

#[derive(Serialize)]
struct MusicApp {
    pid: u32,
    name: String,
    /// Playing right now.
    active: bool,
}

/// Apps making sound, which could be shared into the call.
#[tauri::command]
async fn music_apps(service: State<'_, Service>) -> CmdResult<Vec<MusicApp>> {
    let apps = service.music_apps().map_err(err)?;
    Ok(apps.into_iter().map(|a| MusicApp { pid: a.pid, name: a.name, active: a.active }).collect())
}

/// Shares one app's sound with everyone in the call.
#[tauri::command]
async fn share_app(service: State<'_, Service>, pid: u32) -> CmdResult {
    service.share_app(pid).map_err(err)
}

#[tauri::command]
fn stop_sharing(service: State<Service>) {
    service.stop_sharing();
}

/// Stops shortcuts from firing while the user presses keys to pick a new one.
#[tauri::command]
fn pause_shortcuts(hotkeys: State<Hotkeys<Shortcut>>, paused: bool) {
    hotkeys.pause(paused);
}

/// The overlay page saying whether it has anything to show right now.
#[tauri::command]
fn overlay_has_content(app: AppHandle, has: bool) {
    overlay::set_has_content(&app, has);
}

/// Whether the overlay window is showing the quick menu (rather than the corner panel).
#[tauri::command]
fn overlay_menu_open(app: AppHandle) -> bool {
    overlay::menu_open(&app)
}

#[tauri::command]
fn close_quick_menu(app: AppHandle) {
    overlay::close_menu(&app);
}

/// Opens the main window, e.g. from the quick menu.
#[tauri::command]
fn open_main(app: AppHandle) {
    overlay::close_menu(&app);
    show_main(&app);
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

#[tauri::command]
fn set_volume(service: State<Service>, id: String, volume: f32) -> CmdResult {
    service.set_volume(&parse_id(&id)?, volume);
    Ok(())
}

#[tauri::command]
fn send_message(service: State<Service>, id: String, text: String) -> CmdResult {
    service.send_message(&parse_id(&id)?, &text).map_err(err)
}

#[tauri::command]
fn get_messages(service: State<Service>, id: String) -> CmdResult<Vec<MessageView>> {
    Ok(service.messages(&parse_id(&id)?))
}

#[tauri::command]
fn mark_read(service: State<Service>, id: String) -> CmdResult {
    service.mark_read(&parse_id(&id)?);
    Ok(())
}

#[tauri::command]
async fn start_mic_test(service: State<'_, Service>) -> CmdResult {
    service.start_mic_test().map_err(err)
}

#[tauri::command]
fn stop_mic_test(service: State<Service>) {
    service.stop_mic_test();
}

#[tauri::command]
fn get_mic_test(service: State<Service>) -> Option<MicTestView> {
    service.mic_test()
}

/// Brings the window back from the tray (or from behind other windows).
fn show_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// The tray icon keeps nevar reachable while its window is closed: click to open,
/// right-click for the menu with the real quit.
fn build_tray(app: &tauri::App) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "nevar'ı aç", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Çıkış", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &quit])?;
    let mut tray = TrayIconBuilder::with_id("main")
        .tooltip("nevar")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => show_main(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                show_main(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(())
}

/// What the global shortcuts do.
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
enum Shortcut {
    QuickMenu,
    ToggleMute,
    ToggleDeafen,
    ToggleOverlay,
}

/// Told to the UI after a shortcut changed something, so it can click and show it.
#[derive(Clone, Serialize)]
struct ShortcutUsed {
    shortcut: Shortcut,
    /// The new state: muted, deafened, overlay on.
    on: bool,
}

fn on_shortcut(app: &AppHandle, shortcut: Shortcut) {
    let service = app.state::<Service>();
    let on = match shortcut {
        Shortcut::QuickMenu => {
            overlay::toggle_menu(app);
            return;
        }
        Shortcut::ToggleMute => {
            let muted = !service.muted();
            service.set_muted(muted);
            muted
        }
        Shortcut::ToggleDeafen => {
            let deafened = !service.deafened();
            service.set_deafened(deafened);
            deafened
        }
        Shortcut::ToggleOverlay => {
            let mut settings = service.settings();
            settings.overlay.enabled = !settings.overlay.enabled;
            let on = settings.overlay.enabled;
            service.set_settings(settings);
            overlay::sync(app);
            on
        }
    };
    let _ = app.emit("shortcut", ShortcutUsed { shortcut, on });
}

/// Hands the saved shortcuts to the key watcher.
fn apply_shortcuts(app: &AppHandle) {
    let s = app.state::<Service>().settings().shortcuts;
    let bindings = [
        (Shortcut::QuickMenu, s.quick_menu),
        (Shortcut::ToggleMute, s.toggle_mute),
        (Shortcut::ToggleDeafen, s.toggle_deafen),
        (Shortcut::ToggleOverlay, s.toggle_overlay),
    ];
    let bindings = bindings.into_iter().filter_map(|(action, key)| Some((action, key?))).collect();
    app.state::<Hotkeys<Shortcut>>().set(bindings);
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
            show_main(app);
        }))
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_notification::init())
        // Closing the window keeps nevar running in the tray, so friends can still reach you.
        // Quitting is in the tray menu.
        .on_window_event(|window, event| match event {
            WindowEvent::CloseRequested { api, .. } => {
                api.prevent_close();
                let _ = window.hide();
            }
            WindowEvent::Focused(focused) if window.label() == overlay::LABEL => {
                overlay::on_focus(window.app_handle(), *focused);
            }
            _ => {}
        })
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            let handle = app.handle().clone();
            let service = tauri::async_runtime::block_on(Service::start(&data_dir, move |e| {
                forward_event(&handle, e)
            }))?;
            app.manage(service);
            app.manage(overlay::Overlay::default());
            let handle = app.handle().clone();
            app.manage(Hotkeys::start(move |shortcut| on_shortcut(&handle, shortcut)));
            apply_shortcuts(app.handle());
            overlay::sync(app.handle());
            build_tray(app)?;
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
            set_volume,
            send_message,
            get_messages,
            mark_read,
            start_mic_test,
            stop_mic_test,
            get_mic_test,
            pause_shortcuts,
            music_apps,
            share_app,
            stop_sharing,
            overlay_has_content,
            overlay_menu_open,
            close_quick_menu,
            open_main,
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
