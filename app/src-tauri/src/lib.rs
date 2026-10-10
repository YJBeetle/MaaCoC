//! Tauri shell: a thin command layer over the engine.
//!
//! Deliberately dumb — no game logic and no layout decisions live here, so
//! everything that matters is either unit-tested in Rust or verifiable in a
//! browser against the same commands.

use base64::Engine as _;
use maacoc_engine::{frames::FrameStore, DeviceTarget, NodeEvent, Runner, Strategy, DEVICE_MATCH_SIZE};
use serde::{Deserialize, Serialize};
use std::{
    collections::VecDeque,
    path::PathBuf,
    sync::Mutex,
    time::{Duration, Instant},
};
use tauri::{path::BaseDirectory, AppHandle, Manager, Runtime, State};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub entry: String,
    pub preferred_device: Option<String>,
    pub auto_start: bool,
    pub frame_interval_ms: u64,
    pub show_misses: bool,
    pub overlay_hits: bool,
    pub record_frames: bool,
    pub theme_mode: String,
    pub strategy: Strategy,
    pub tactics: maacoc_engine::tactics::Tactics,
    pub stop_when_full: bool,
    pub loot_filter: maacoc_engine::loot::LootFilter,
    pub army: maacoc_engine::army::ArmyPlan,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            entry: "Main".into(),
            preferred_device: None,
            auto_start: false,
            frame_interval_ms: 1000,
            show_misses: false,
            overlay_hits: false,
            record_frames: false,
            theme_mode: "system".into(),
            strategy: Strategy::default(),
            tactics: Default::default(),
            stop_when_full: true,
            loot_filter: Default::default(),
            army: Default::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    Idle,
    Connecting,
    Ready,
    Running,
    Error,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub phase: Phase,
    pub detail: String,
    pub battles: usize,
    pub uptime_ms: u64,
    pub current_node: String,
    /// The panel's physical and current size, as `wm size` reports them.
    pub panel: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DeviceItem {
    pub label: String,
    pub address: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Frame {
    pub data_url: String,
    pub width: u32,
    pub height: u32,
    /// False when the device is in portrait, i.e. not showing the game at all.
    pub landscape: bool,
}

struct Inner {
    runner: Option<Runner>,
    settings: Settings,
    recorder: Option<FrameStore>,
    last_reco: i64,
    log_dir: String,
    events: VecDeque<NodeEvent>,
    started: Option<Instant>,
    battles: usize,
    phase: Phase,
    detail: String,
    panel: String,
    last_connection_check: Option<Instant>,
    retiring: Vec<std::thread::JoinHandle<()>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Paths {
    pub log_dir: String,
}

pub struct AppState {
    inner: Mutex<Inner>,
}

impl AppState {
    fn new(settings: Settings, log_dir: String) -> Self {
        Self {
            inner: Mutex::new(Inner {
                runner: None,
                settings,
                recorder: None,
                last_reco: 0,
                log_dir,
                events: VecDeque::new(),
                started: None,
                battles: 0,
                phase: Phase::Idle,
                detail: String::new(),
                panel: String::new(),
                last_connection_check: None,
                retiring: Vec::new(),
            }),
        }
    }
}

/// Where the pipeline/image assets live. Bundled apps resolve them next to the
/// resources; `tauri dev` has no resource dir, so fall back to the source tree.
fn assets_dir<R: Runtime>(app: &AppHandle<R>) -> Result<PathBuf, String> {
    // Tauri also stages resources beside debug binaries. Those copies can lag
    // behind template edits, so development runs must use the source assets.
    #[cfg(debug_assertions)]
    {
        let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets");
        if source.join("pipeline").is_dir() {
            return Ok(source);
        }
    }
    let candidates = [
        app.path().resolve("assets", BaseDirectory::Resource).ok(),
        std::env::current_dir().ok().map(|dir| dir.join("assets")),
        Some(PathBuf::from("../../assets")),
        Some(PathBuf::from("assets")),
    ];
    candidates
        .into_iter()
        .flatten()
        .find(|dir| dir.join("pipeline").is_dir())
        .ok_or_else(|| "找不到资源目录 assets/pipeline，请确认安装完整".to_string())
}

fn settings_file<R: Runtime>(app: &AppHandle<R>) -> Result<PathBuf, String> {
    let dir = app.path().app_config_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir.join("settings.json"))
}

fn load_settings<R: Runtime>(app: &AppHandle<R>) -> Settings {
    settings_file(app)
        .ok()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

/// Where recorded frames go. Created only when the user turns recording on.
fn frames_dir<R: Runtime>(app: &AppHandle<R>) -> Result<PathBuf, String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?.join("frames");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

/// Follow the 记录节点画面 switch: the framework only keeps the frame behind a
/// recognition when debug mode is on, so the toggle drives both the store and
/// the global option.
fn apply_recording<R: Runtime>(inner: &mut Inner, app: &AppHandle<R>) -> Result<(), String> {
    let on = inner.settings.record_frames;
    if let Some(runner) = inner.runner.as_ref() {
        // Debug mode is what makes "the frame a recognition saw" readable, which
        // is the only live view available while a task owns the controller. The
        // cache limit keeps it to a few frames instead of the 4096 default.
        runner.set_debug_mode(true).map_err(|e| e.to_string())?;
        maacoc_engine::runner::Runner::set_reco_cache_limit(4).map_err(|e| e.to_string())?;
    }
    inner.recorder = if on {
        Some(FrameStore::open(frames_dir(app)?))
    } else {
        None
    };
    Ok(())
}

#[tauri::command]
async fn settings(state: State<'_, AppState>) -> Result<Settings, String> {
    Ok(state.inner.lock().unwrap().settings.clone())
}

/// Where the app keeps its own files, for the 关于 panel and for bug reports.
#[tauri::command]
async fn paths(state: State<'_, AppState>) -> Result<Paths, String> {
    let inner = state.inner.lock().unwrap();
    Ok(Paths {
        log_dir: inner.log_dir.clone(),
    })
}

#[tauri::command]
async fn save_settings<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    next: Settings,
) -> Result<Settings, String> {
    next.loot_filter.validate().map_err(|e| e.to_string())?;
    next.tactics.validate().map_err(|e| e.to_string())?;
    if !next.army.entries.is_empty() {
        next.army.validate(&read_army_cards(&app)?).map_err(|e| e.to_string())?;
    }
    {
        let mut inner = state.inner.lock().unwrap();
        inner.settings = next.clone();
        apply_recording(&mut inner, &app)?;
    }
    let path = settings_file(&app)?;
    std::fs::write(path, serde_json::to_string_pretty(&next).unwrap()).map_err(|e| e.to_string())?;
    Ok(next)
}

#[tauri::command]
async fn devices() -> Result<Vec<DeviceItem>, String> {
    let found = maacoc_engine::list_devices().map_err(|e| e.to_string())?;
    Ok(found
        .into_iter()
        .map(|d| DeviceItem {
            label: format!("{} · {}", d.label, d.address),
            address: d.address,
        })
        .collect())
}

/// Pipeline node names, for the entry selector. Empty until resources load.
#[tauri::command]
async fn nodes(state: State<'_, AppState>) -> Result<Vec<String>, String> {
    let inner = state.inner.lock().unwrap();
    match inner.runner.as_ref().map(|r| r.resource().node_list()) {
        Some(list) => list.map_err(|e| e.to_string()),
        None => Ok(Vec::new()),
    }
}

fn read_army_cards<R: Runtime>(app: &AppHandle<R>) -> Result<Vec<maacoc_engine::army::ArmyCard>, String> {
    let assets = assets_dir(app)?;
    let source = std::fs::read_to_string(assets.join("army.json")).map_err(|e| e.to_string())?;
    serde_json::from_str(&source).map_err(|e| e.to_string())
}

#[tauri::command]
async fn army_cards<R: Runtime>(app: AppHandle<R>) -> Result<Vec<maacoc_engine::army::ArmyCard>, String> {
    let assets = assets_dir(&app)?;
    let mut cards = read_army_cards(&app)?;
    for card in &mut cards {
        let bytes = std::fs::read(assets.join("image").join(&card.preview)).map_err(|e| e.to_string())?;
        card.preview = format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(bytes)
        );
    }
    Ok(cards)
}

#[tauri::command]
async fn read_army<R: Runtime>(app: AppHandle<R>, state: State<'_, AppState>) -> Result<Status, String> {
    let cards = read_army_cards(&app)?;
    let mut inner = state.inner.lock().unwrap();
    let runner = inner.runner.as_ref().ok_or("请先连接手机")?;
    runner.start_read_army(&cards).map_err(|e| e.to_string())?;
    inner.phase = Phase::Running;
    inner.started = Some(Instant::now());
    inner.events.clear();
    Ok(status_of(&inner))
}

#[tauri::command]
async fn read_army_result<R: Runtime>(app: AppHandle<R>, state: State<'_, AppState>) -> Result<Settings, String> {
    let next = {
        let inner = state.inner.lock().unwrap();
        if inner.detail != "已读取手机卡组" {
            return Err("读取卡组尚未成功".into());
        }
        let runner = inner.runner.as_ref().ok_or("手机已断开")?;
        if runner.running() {
            return Err("读取尚未结束".into());
        }
        let mut next = inner.settings.clone();
        next.army = runner.take_read_army().ok_or("没有读取结果")?;
        next
    };
    save_settings(app, state, next).await
}

#[tauri::command]
async fn apply_army<R: Runtime>(app: AppHandle<R>, state: State<'_, AppState>) -> Result<Status, String> {
    let cards = read_army_cards(&app)?;
    let mut inner = state.inner.lock().unwrap();
    let runner = inner.runner.as_ref().ok_or("请先连接手机")?;
    runner
        .start_army(&inner.settings.army, &cards)
        .map_err(|e| e.to_string())?;
    inner.phase = Phase::Running;
    inner.started = Some(Instant::now());
    inner.events.clear();
    Ok(status_of(&inner))
}

#[tauri::command]
async fn connect<R: Runtime>(app: AppHandle<R>, state: State<'_, AppState>) -> Result<Status, String> {
    let preferred = state.inner.lock().unwrap().settings.preferred_device.clone();
    {
        let mut inner = state.inner.lock().unwrap();
        inner.phase = Phase::Connecting;
        inner.detail = String::new();
    }
    let outcome = assets_dir(&app).and_then(|assets| {
        Runner::connect(&assets, DeviceTarget::Adb, preferred.as_deref()).map_err(|e| e.to_string())
    });
    let mut inner = state.inner.lock().unwrap();
    match outcome {
        Ok(runner) => {
            // Replacing a live Runner drops it; Drop stops the task first, but do
            // it visibly here so a reconnect never races the old worker thread.
            if let Some(old) = inner.runner.take() {
                old.stop(Duration::from_secs(10));
            }
            inner.detail = runner.label.clone();
            inner.phase = Phase::Ready;
            inner.runner = Some(runner);
            inner.last_connection_check = None;
            if let Some(runner) = inner.runner.as_ref() {
                inner.panel = panel_sizes(runner).unwrap_or_default();
            }
            apply_recording(&mut inner, &app)?;
            if inner.settings.auto_start {
                // 连接成功就开跑；起不来仍然算连上了，把原因显示出来供手动重试。
                if let Err(err) = begin(&mut inner) {
                    let label = std::mem::take(&mut inner.detail);
                    inner.detail = format!("{label}（自动开始失败: {err}）");
                }
            }
            Ok(status_of(&inner))
        }
        Err(err) => {
            inner.phase = Phase::Error;
            inner.detail = err.clone();
            Err(err)
        }
    }
}

fn begin(inner: &mut Inner) -> Result<(), String> {
    let entry = inner.settings.entry.clone();
    let runner = inner.runner.as_ref().ok_or("尚未连接设备")?;
    runner
        .start_farming_with_tactics(
            &entry,
            inner.settings.strategy,
            inner.settings.stop_when_full,
            &inner.settings.loot_filter,
            &inner.settings.tactics,
        )
        .map_err(|e| e.to_string())?;
    inner.phase = Phase::Running;
    inner.started = Some(Instant::now());
    inner.events.clear();
    Ok(())
}

#[tauri::command]
async fn start(state: State<'_, AppState>) -> Result<Status, String> {
    let mut inner = state.inner.lock().unwrap();
    begin(&mut inner)?;
    Ok(status_of(&inner))
}

#[tauri::command]
async fn stop(state: State<'_, AppState>) -> Result<Status, String> {
    let mut inner = state.inner.lock().unwrap();
    if let Some(runner) = inner.runner.as_ref() {
        runner.stop(Duration::from_secs(15));
    }
    inner.phase = if inner.runner.is_some() { Phase::Ready } else { Phase::Idle };
    inner.started = None;
    Ok(status_of(&inner))
}

#[tauri::command]
async fn disconnect(state: State<'_, AppState>) -> Result<Status, String> {
    let mut inner = state.inner.lock().unwrap();
    if let Some(runner) = inner.runner.take() {
        runner.stop(Duration::from_secs(10));
    }
    inner.phase = Phase::Idle;
    inner.detail = String::new();
    inner.panel = String::new();
    inner.started = None;
    Ok(status_of(&inner))
}

/// Read `wm size` and render it as "物理 W×H · 当前 W×H". A device with no
/// override reports the physical size once, so current equals physical.
fn panel_sizes(runner: &Runner) -> Result<String, String> {
    let out = runner.shell("wm size").map_err(|e| e.to_string())?;
    let grab = |key: &str| {
        out.lines()
            .find_map(|line| line.strip_prefix(key))
            .map(|rest| rest.trim().replace('x', "×"))
    };
    let physical = grab("Physical size:").unwrap_or_default();
    let current = grab("Override size:").unwrap_or_else(|| physical.clone());
    if physical.is_empty() {
        return Ok(String::new());
    }
    Ok(format!("物理 {physical} · 当前 {current}"))
}

/// `wm size` through the framework's own channel: pin the panel to the 16:9
/// logical size the assets were cropped for, or hand the phone its stock
/// geometry back. Re-reads the panel so the stage badge shows the new size.
#[tauri::command]
async fn screen_size(state: State<'_, AppState>, reset: bool) -> Result<(), String> {
    let mut inner = state.inner.lock().unwrap();
    let runner = inner.runner.as_ref().ok_or("尚未连接设备")?;
    if runner.running() {
        // The panel re-layouts mid-battle, so every tap in flight is wrong.
        return Err("战斗进行中，请先停止再改分辨率".into());
    }
    let cmd = if reset {
        "wm size reset".to_owned()
    } else {
        format!("wm size {DEVICE_MATCH_SIZE}")
    };
    runner.shell(&cmd).map_err(|e| e.to_string())?;
    inner.panel = panel_sizes(runner)?;
    Ok(())
}

fn status_of(inner: &Inner) -> Status {
    Status {
        phase: inner.phase,
        detail: inner.detail.clone(),
        battles: inner.battles,
        uptime_ms: inner.started.map(|t| t.elapsed().as_millis() as u64).unwrap_or(0),
        current_node: inner.events.back().map(|e| e.label().to_string()).unwrap_or_default(),
        panel: inner.panel.clone(),
    }
}

#[tauri::command]
async fn status(state: State<'_, AppState>) -> Result<Status, String> {
    let mut inner = state.inner.lock().unwrap();
    if inner.runner.is_some()
        && inner.phase != Phase::Connecting
        && inner.last_connection_check.is_none_or(|at| at.elapsed() >= Duration::from_secs(2))
    {
        inner.last_connection_check = Some(Instant::now());
        if !inner.runner.as_ref().unwrap().device_online() {
            let runner = inner.runner.take();
            mark_device_disconnected(&mut inner);
            let status = status_of(&inner);
            // Keep cleanup handles so process exit waits until MAA has stopped
            // using its libraries; detached cleanup can race static destructors.
            inner.retiring.retain(|worker| !worker.is_finished());
            if let Some(runner) = runner {
                inner.retiring.push(std::thread::spawn(move || {
                    runner.stop(Duration::from_secs(5));
                    drop(runner);
                }));
            }
            return Ok(status);
        }
    }
    Ok(status_of(&inner))
}

fn shutdown(state: &AppState) {
    let (runner, retiring) = {
        let mut inner = state.inner.lock().unwrap();
        let runner = inner.runner.take();
        let retiring = std::mem::take(&mut inner.retiring);
        mark_device_disconnected(&mut inner);
        (runner, retiring)
    };
    if let Some(runner) = runner {
        runner.stop(Duration::from_secs(5));
        drop(runner);
    }
    for worker in retiring {
        let _ = worker.join();
    }
}

fn mark_device_disconnected(inner: &mut Inner) {
    inner.phase = Phase::Idle;
    inner.detail = "设备已断开，请连接设备后重试".into();
    inner.panel.clear();
    inner.started = None;
    inner.last_reco = 0;
    inner.last_connection_check = None;
}

#[tauri::command]
async fn events(state: State<'_, AppState>) -> Result<Vec<NodeEvent>, String> {
    let mut inner = state.inner.lock().unwrap();
    let fresh = match inner.runner.as_ref() {
        Some(runner) => runner.poll(),
        None => Vec::new(),
    };
    for event in &fresh {
        if event.kind == "action"
            && event.hit
            && !event.error
            && (event.node == "StorageContinue" || (event.node == "AttackStart" && !inner.settings.stop_when_full))
        {
            inner.battles += 1;
        }
        if event.kind == "recognition" && event.reco_id > 0 {
            inner.last_reco = event.reco_id;
        }
        inner.events.push_back(event.clone());
        while inner.events.len() > 300 {
            inner.events.pop_front();
        }
    }
    if let (Some(store), Some(runner)) = (inner.recorder.as_ref(), inner.runner.as_ref()) {
        for event in &fresh {
            if !(event.hit && event.kind == "recognition") {
                continue;
            }
            // Best effort: a failed frame write must never break the timeline.
            if let Some(png) = runner.recognition_png(event.reco_id) {
                let _ = store.save(&png, &event.node, &event.focus);
            }
        }
    }
    if inner.phase == Phase::Running
        && !inner.events.is_empty()
        && inner.runner.as_ref().is_some_and(|runner| !runner.running())
    {
        inner.phase = Phase::Ready;
        inner.started = None;
        inner.detail = match inner.events.iter().rev().find(|e| e.hit && e.kind == "action").cloned() {
            Some(e) if e.node == "StorageFullStop" => "金币、圣水和黑油全部满仓，挂机刷资源已停止".into(),
            Some(e) if e.node == "ArmyReadComplete" => "已读取手机卡组".into(),
            Some(e) if e.node == "ArmyComplete" => "配卡完成，兵种、法术和战车数量已核对".into(),
            Some(e)
                if matches!(
                    e.node.as_str(),
                    "StorageReadFailed" | "LootReadFailed" | "LootSkipLimit" | "ArmyFailed"
                ) =>
            {
                inner.phase = Phase::Error;
                e.focus.clone()
            }
            _ => "任务已结束".into(),
        };
    }
    Ok(fresh)
}

#[tauri::command]
async fn frame(state: State<'_, AppState>) -> Result<Option<Frame>, String> {
    let inner = state.inner.lock().unwrap();
    let Some(runner) = inner.runner.as_ref() else {
        return Ok(None);
    };
    // While a task runs the controller is owned by it, and a direct screencap
    // comes back empty (observed on every poll during a battle). The frame the
    // last recognition actually matched on is both available and more honest, so
    // show that; debug mode keeps it alive for exactly one frame worth of work.
    let png = if runner.running() {
        let reco = inner.last_reco;
        runner.recognition_png(reco).ok_or("任务运行中，等待下一次识别")?
    } else {
        runner.screencap_png().map_err(|e| e.to_string())?
    };
    let (width, height) = maacoc_engine::frames::png_size(&png);
    Ok(Some(Frame {
        data_url: format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(&png)
        ),
        width,
        height,
        landscape: runner.is_landscape(width, height),
    }))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            // MaaFramework would otherwise log next to the working directory,
            // which for a double-clicked .app is somewhere unwritable.
            let log_dir = app
                .path()
                .app_log_dir()
                .ok()
                .filter(|dir| std::fs::create_dir_all(dir).is_ok())
                .map(|dir| {
                    let _ = maacoc_engine::set_log_dir(&dir);
                    dir.to_string_lossy().into_owned()
                })
                .unwrap_or_default();
            let settings = load_settings(app.handle());
            app.manage(AppState::new(settings, log_dir));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            settings,
            save_settings,
            paths,
            devices,
            nodes,
            army_cards,
            apply_army,
            read_army,
            read_army_result,
            connect,
            start,
            stop,
            disconnect,
            screen_size,
            status,
            events,
            frame
        ])
        // A battle is a resident task that keeps tapping the phone. Stopping it
        // on the way out is the difference between closing a window and a
        // device that clicks forever.
        .build(tauri::generate_context!())
        .expect("启动 MaaCoC 失败")
        .run(|app, event| {
            // Native macOS Quit (Cmd+Q) can deliver Exit directly, without
            // ExitRequested. Release MAA before AppKit calls C++ destructors.
            if matches!(event, tauri::RunEvent::ExitRequested { .. } | tauri::RunEvent::Exit) {
                if let Some(state) = app.try_state::<AppState>() {
                    shutdown(&state);
                }
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shutdown_releases_loaded_framework_without_a_phone() {
        let state = AppState::new(Settings::default(), String::new());
        let assets = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets");
        let runner = Runner::connect(&assets, DeviceTarget::Headless, None).unwrap();
        state.inner.lock().unwrap().runner = Some(runner);
        shutdown(&state);
        assert!(state.inner.lock().unwrap().runner.is_none());
    }

    #[test]
    fn shutdown_joins_retiring_connections_and_is_idempotent() {
        use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
        let state = AppState::new(Settings::default(), String::new());
        let finished = Arc::new(AtomicBool::new(false));
        let flag = finished.clone();
        state.inner.lock().unwrap().retiring.push(std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(30));
            flag.store(true, Ordering::SeqCst);
        }));
        shutdown(&state);
        assert!(finished.load(Ordering::SeqCst));
        assert!(state.inner.lock().unwrap().retiring.is_empty());
        shutdown(&state);
        assert_eq!(state.inner.lock().unwrap().phase, Phase::Idle);
    }

    #[test]
    fn unplugging_clears_device_state_and_stops_uptime() {
        let state = AppState::new(Settings::default(), String::new());
        let mut inner = state.inner.lock().unwrap();
        inner.phase = Phase::Running;
        inner.started = Some(Instant::now());
        inner.panel = "物理 1080×2400".into();
        inner.last_reco = 123;
        mark_device_disconnected(&mut inner);
        let status = status_of(&inner);
        assert_eq!(status.phase, Phase::Idle);
        assert_eq!(status.uptime_ms, 0);
        assert!(status.panel.is_empty());
        assert_eq!(inner.last_reco, 0);
        assert!(status.detail.contains("设备已断开"));
    }

    #[test]
    fn settings_defaults_are_conservative() {
        let settings = Settings::default();
        assert_eq!(settings.entry, "Main");
        assert_eq!(settings.strategy, Strategy::SpellsFirst);
        assert_eq!(settings.theme_mode, "system", "外观默认跟随系统，不预设深浅");
        assert!(!settings.auto_start, "打开界面不应自动开始战斗");
        assert!(!settings.overlay_hits);
        assert!(!settings.record_frames);
    }

    #[test]
    fn settings_round_trip_through_json() {
        let settings = Settings {
            auto_start: true,
            theme_mode: "dark".into(),
            strategy: Strategy::UnitsFirst,
            ..Default::default()
        };
        let text = serde_json::to_string(&settings).unwrap();
        let back: Settings = serde_json::from_str(&text).unwrap();
        assert!(back.auto_start);
        assert_eq!(back.entry, "Main");
        assert_eq!(back.theme_mode, "dark");
        assert_eq!(back.strategy, Strategy::UnitsFirst);
    }

    #[test]
    fn older_settings_files_still_load_with_defaults() {
        let back: Settings = serde_json::from_str(r#"{"entry":"Main"}"#).unwrap();
        assert_eq!(back.theme_mode, "system");
        assert!(!back.show_misses);
        assert_eq!(back.strategy, Strategy::SpellsFirst);
    }

    #[test]
    fn loot_settings_are_backward_compatible_and_persist_thresholds() {
        let old: Settings = serde_json::from_str("{\"entry\":\"Main\"}").unwrap();
        assert!(!old.loot_filter.enabled);
        assert_eq!(old.loot_filter.min_gold_elixir, 1_000_000);
        let configured: Settings = serde_json::from_str(r#"{"lootFilter":{"enabled":true,"minGold":600000,"minElixir":700000,"minDark":9000,"minGoldElixir":0,"matchAny":false,"maxSkips":20}}"#).unwrap();
        let back: Settings = serde_json::from_value(serde_json::to_value(configured).unwrap()).unwrap();
        assert!(back.loot_filter.enabled);
        assert_eq!(back.loot_filter.min_gold, 600000);
        assert_eq!(back.loot_filter.min_elixir, 700000);
        assert_eq!(back.loot_filter.min_dark, 9000);
        assert!(!back.loot_filter.match_any);
        assert_eq!(back.loot_filter.max_skips, 20);
    }

    /// The frontend's `Settings` interface must carry exactly these keys. A
    /// field that only exists on one side is silently dropped by serde, which
    /// is how the theme setting stopped working.
    #[test]
    fn settings_wire_format_is_the_contract() {
        let value = serde_json::to_value(Settings::default()).unwrap();
        let mut keys: Vec<_> = value.as_object().unwrap().keys().cloned().collect();
        keys.sort();
        assert_eq!(
            keys,
            [
                "army",
                "autoStart",
                "entry",
                "frameIntervalMs",
                "lootFilter",
                "overlayHits",
                "preferredDevice",
                "recordFrames",
                "showMisses",
                "stopWhenFull",
                "strategy",
                "tactics",
                "themeMode"
            ]
        );
    }
}
