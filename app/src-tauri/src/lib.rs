//! Tauri shell: a thin command layer over the engine.
//!
//! Deliberately dumb — no game logic and no layout decisions live here, so
//! everything that matters is either unit-tested in Rust or verifiable in a
//! browser against the same commands.

use base64::Engine as _;
use maacoc_engine::{frames::FrameStore, DeviceTarget, NodeEvent, Runner};
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
}

struct Inner {
    runner: Option<Runner>,
    settings: Settings,
    recorder: Option<FrameStore>,
    events: VecDeque<NodeEvent>,
    started: Option<Instant>,
    battles: usize,
    phase: Phase,
    detail: String,
}

pub struct AppState {
    inner: Mutex<Inner>,
}

impl AppState {
    fn new(settings: Settings) -> Self {
        Self {
            inner: Mutex::new(Inner {
                runner: None,
                settings,
                recorder: None,
                events: VecDeque::new(),
                started: None,
                battles: 0,
                phase: Phase::Idle,
                detail: String::new(),
            }),
        }
    }
}

/// Where the pipeline/image assets live. Bundled apps resolve them next to the
/// resources; `tauri dev` has no resource dir, so fall back to the source tree.
fn assets_dir<R: Runtime>(app: &AppHandle<R>) -> Result<PathBuf, String> {
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
        runner.set_debug_mode(on).map_err(|e| e.to_string())?;
    }
    inner.recorder = if on {
        Some(FrameStore::open(frames_dir(app)?))
    } else {
        None
    };
    Ok(())
}

#[tauri::command]
fn settings(state: State<'_, AppState>) -> Result<Settings, String> {
    Ok(state.inner.lock().unwrap().settings.clone())
}

#[tauri::command]
fn save_settings<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    next: Settings,
) -> Result<Settings, String> {
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
fn devices() -> Result<Vec<DeviceItem>, String> {
    let found = maacoc_engine::list_devices().map_err(|e| e.to_string())?;
    Ok(found
        .into_iter()
        .map(|(name, address)| DeviceItem { label: name, address })
        .collect())
}

/// Pipeline node names, for the entry selector. Empty until resources load.
#[tauri::command]
fn nodes(state: State<'_, AppState>) -> Result<Vec<String>, String> {
    let inner = state.inner.lock().unwrap();
    match inner.runner.as_ref().map(|r| r.resource().node_list()) {
        Some(list) => list.map_err(|e| e.to_string()),
        None => Ok(Vec::new()),
    }
}

#[tauri::command]
fn connect<R: Runtime>(app: AppHandle<R>, state: State<'_, AppState>) -> Result<Status, String> {
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
            inner.detail = runner.label.clone();
            inner.phase = Phase::Ready;
            inner.runner = Some(runner);
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
    runner.start(&entry).map_err(|e| e.to_string())?;
    inner.phase = Phase::Running;
    inner.started = Some(Instant::now());
    inner.events.clear();
    Ok(())
}

#[tauri::command]
fn start(state: State<'_, AppState>) -> Result<Status, String> {
    let mut inner = state.inner.lock().unwrap();
    begin(&mut inner)?;
    Ok(status_of(&inner))
}

#[tauri::command]
fn stop(state: State<'_, AppState>) -> Result<Status, String> {
    let mut inner = state.inner.lock().unwrap();
    if let Some(runner) = inner.runner.as_ref() {
        runner.stop(Duration::from_secs(15));
    }
    inner.phase = Phase::Ready;
    inner.started = None;
    Ok(status_of(&inner))
}

#[tauri::command]
fn disconnect(state: State<'_, AppState>) -> Result<Status, String> {
    let mut inner = state.inner.lock().unwrap();
    if let Some(runner) = inner.runner.take() {
        runner.stop(Duration::from_secs(10));
    }
    inner.phase = Phase::Idle;
    inner.detail = String::new();
    inner.started = None;
    Ok(status_of(&inner))
}

fn status_of(inner: &Inner) -> Status {
    Status {
        phase: inner.phase,
        detail: inner.detail.clone(),
        battles: inner.battles,
        uptime_ms: inner.started.map(|t| t.elapsed().as_millis() as u64).unwrap_or(0),
        current_node: inner.events.back().map(|e| e.label().to_string()).unwrap_or_default(),
    }
}

#[tauri::command]
fn status(state: State<'_, AppState>) -> Result<Status, String> {
    let inner = state.inner.lock().unwrap();
    Ok(status_of(&inner))
}

#[tauri::command]
fn events(state: State<'_, AppState>) -> Result<Vec<NodeEvent>, String> {
    let mut inner = state.inner.lock().unwrap();
    let fresh = match inner.runner.as_ref() {
        Some(runner) => runner.poll(),
        None => Vec::new(),
    };
    for event in &fresh {
        if event.kind == "action" && event.node == "AttackStart" {
            inner.battles += 1;
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
    Ok(fresh)
}

#[tauri::command]
fn frame(state: State<'_, AppState>) -> Result<Option<Frame>, String> {
    let inner = state.inner.lock().unwrap();
    let Some(runner) = inner.runner.as_ref() else {
        return Ok(None);
    };
    let png = runner.screencap_png().map_err(|e| e.to_string())?;
    let (width, height) = maacoc_engine::frames::png_size(&png);
    Ok(Some(Frame {
        data_url: format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(&png)
        ),
        width,
        height,
    }))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let settings = load_settings(app.handle());
            app.manage(AppState::new(settings));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            settings,
            save_settings,
            devices,
            nodes,
            connect,
            start,
            stop,
            disconnect,
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
            if matches!(event, tauri::RunEvent::ExitRequested { .. }) {
                if let Some(state) = app.try_state::<AppState>() {
                    let inner = state.inner.lock().unwrap();
                    if let Some(runner) = inner.runner.as_ref() {
                        runner.stop(Duration::from_secs(5));
                    }
                }
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_defaults_are_conservative() {
        let settings = Settings::default();
        assert_eq!(settings.entry, "Main");
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
            ..Default::default()
        };
        let text = serde_json::to_string(&settings).unwrap();
        let back: Settings = serde_json::from_str(&text).unwrap();
        assert!(back.auto_start);
        assert_eq!(back.entry, "Main");
        assert_eq!(back.theme_mode, "dark");
    }

    #[test]
    fn older_settings_files_still_load_with_defaults() {
        let back: Settings = serde_json::from_str(r#"{"entry":"Main"}"#).unwrap();
        assert_eq!(back.theme_mode, "system");
        assert!(!back.show_misses);
    }
}
