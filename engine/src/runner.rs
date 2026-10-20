//! Owning the controller/resource/tasker triple for one session.

use crate::events::{EventBus, NodeEvent};
use crate::Result;
use maa_framework::{
    common::MaaStatus,
    controller::{AdbControllerBuilder, Controller},
    resource::Resource,
    tasker::Tasker,
};
use std::{
    path::Path,
    thread,
    time::{Duration, Instant},
};

pub enum DeviceTarget {
    /// A real phone/emulator over ADB.
    Adb,
    /// Nothing to talk to: enough for node trials, which take the image directly.
    Headless,
}

pub struct Runner {
    // Fields drop in declaration order. The tasker holds borrowed handles to
    // both resource and controller, so destroy its worker before those handles.
    tasker: Tasker,
    resource: Resource,
    controller: Option<Controller>,
    sink_id: i64,
    bus: EventBus,
    pub label: String,
    army_read: crate::army::ReadResult,
    storage_capacity: crate::storage::CapacityCache,
    adb_target: Option<(std::path::PathBuf, String)>,
}

fn check(status: MaaStatus, what: &str) -> Result<()> {
    if status == MaaStatus::SUCCEEDED {
        Ok(())
    } else {
        Err(format!("{what} 失败: {status:?}").into())
    }
}

fn adb_online(path: &Path, address: &str, timeout: Duration) -> bool {
    use std::process::{Command, Stdio};
    let Ok(mut child) = Command::new(path)
        .args(["-s", address, "get-state"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    else {
        return false;
    };
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => {
                return child.wait_with_output().is_ok_and(|output| {
                    output.status.success() && String::from_utf8_lossy(&output.stdout).trim() == "device"
                });
            }
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return false;
            }
        }
    }
}

/// Resolve both installed resources and a source checkout independently of cwd.
fn agent_dir(assets: &Path) -> Result<std::path::PathBuf> {
    let assets = assets.canonicalize()?;
    let root = assets.parent().ok_or("资源目录没有父目录")?;
    let mut candidates = vec![root.join("MaaAgentBinary"), root.join("vendor/share/MaaAgentBinary")];
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            candidates.push(parent.join("MaaAgentBinary"));
        }
    }
    candidates
        .into_iter()
        .find(|dir| dir.join("maatouch/universal/maatouch").is_file())
        .ok_or_else(|| {
            "找不到 MaaAgentBinary/maatouch/universal/maatouch，请重新运行 scripts/fetch-sdk.sh 或检查安装包".into()
        })
}

impl Drop for Runner {
    /// MAA's worker thread calls the context sink we registered while it is
    /// recognising. Freeing the tasker (and with it the boxed callback) mid-call
    /// is a use-after-free — it showed up as a SIGSEGV inside
    /// `EventDispatcher::notify` when a reconnect replaced a running Runner.
    /// So: ask it to stop, wait for the worker to finish, unregister the sink.
    fn drop(&mut self) {
        if self.tasker.running() {
            let _ = self.tasker.post_stop();
        }
        if self.await_idle(Duration::from_secs(15)) && self.sink_id > 0 {
            self.tasker.remove_context_sink(self.sink_id);
        }
    }
}

impl Runner {
    /// Initialize MaaTouch against the final display size before farming.
    /// Changing wm size on an existing controller leaves its input mapping stale.
    pub fn connect_for_farming(assets: &Path, preferred: Option<&str>) -> Result<Self> {
        let runner = Self::connect(assets, DeviceTarget::Adb, preferred)?;
        let size = runner.shell("wm size")?;
        let current = size.lines().rev().find_map(|line| {
            line.strip_prefix("Override size:").or_else(|| line.strip_prefix("Physical size:"))
        }).ok_or("无法读取设备分辨率")?.trim();
        if current == crate::DEVICE_MATCH_SIZE {
            return Ok(runner);
        }
        let address = runner.device_address().ok_or("当前设备不是 ADB 设备")?.to_owned();
        runner.shell(&format!("wm size {}", crate::DEVICE_MATCH_SIZE))?;
        drop(runner);
        Self::connect(assets, DeviceTarget::Adb, Some(&address))
    }

    pub fn connect(assets: &Path, target: DeviceTarget, preferred: Option<&str>) -> Result<Self> {
        if matches!(target, DeviceTarget::Headless) {
            let resource = Resource::new()?;
            let storage_capacity = crate::storage::register(&resource)?;
            crate::loot::register(&resource)?;
            crate::army::register(&resource)?;
            check(
                resource.post_bundle(assets.to_str().ok_or("资源路径无效")?)?.wait(),
                "加载资源",
            )?;
            if !resource.loaded() {
                return Err("资源未就绪".into());
            }
            return Ok(Self::assemble(None, resource, "无设备（仅试跑）".into(), storage_capacity));
        }
        let (controller, label, adb_target) = match target {
            DeviceTarget::Headless => return Err("无设备模式不需要建立连接".into()),
            DeviceTarget::Adb => {
                let mut devices: Vec<_> = crate::discover_adb_devices()?;
                if devices.is_empty() {
                    return Err(crate::no_device_hint().into());
                }
                let device = match preferred {
                    Some(needle) => devices
                        .into_iter()
                        .find(|d| d.address.contains(needle) || d.name.contains(needle))
                        .ok_or_else(|| format!("未找到设备 {needle}"))?,
                    None => devices.remove(0),
                };
                let name = crate::device_model(&device).unwrap_or_else(|| device.name.clone());
                let label = format!("{name} · {}", device.address);
                let config = serde_json::to_string(&device.config)?;
                let agent = agent_dir(assets)?;
                let controller =
                    AdbControllerBuilder::new(device.adb_path.to_str().ok_or("adb 路径无效")?, &device.address)
                        // LongPress silently degrades to Click with AdbShell. Require
                        // MaaTouch so a successful action really holds the contact.
                        .input_methods(maa_framework::sys::MaaAdbInputMethod_Maatouch as _)
                        .config(&config)
                        .agent_path(agent.to_str().ok_or("MaaTouch 路径无效")?)
                        .build()?;
                (controller, label, (device.adb_path.clone(), device.address.clone()))
            }
        };

        check(controller.wait(controller.post_connection()?), "连接设备")?;
        if !controller.connected() {
            return Err(format!("{} 未能建立连接", label).into());
        }
        let controller = Some(controller);
        let resource = Resource::new()?;
        let storage_capacity = crate::storage::register(&resource)?;
        crate::loot::register(&resource)?;
        crate::army::register(&resource)?;
        check(
            resource.post_bundle(assets.to_str().ok_or("资源路径无效")?)?.wait(),
            "加载资源",
        )?;
        if !resource.loaded() {
            return Err("资源未就绪".into());
        }
        let mut runner = Self::assemble(controller, resource, label, storage_capacity);
        runner.adb_target = Some(adb_target);
        Ok(runner)
    }

    fn assemble(controller: Option<Controller>, resource: Resource, label: String, storage_capacity: crate::storage::CapacityCache) -> Self {
        let army_read = crate::army::register_reader(&resource).expect("注册军队读取识别");
        let tasker = Tasker::new().expect("创建 Tasker 失败");
        let _ = tasker.bind_resource(&resource);
        if let Some(ctrl) = &controller {
            let _ = tasker.bind_controller(ctrl);
        }
        let bus = EventBus::new();
        // Context sinks carry the per-node recognition/action notifications.
        let sink_id = tasker.add_context_sink(bus.handle()).unwrap_or(0);
        Self {
            controller,
            resource,
            sink_id,
            tasker,
            bus,
            label,
            army_read,
            storage_capacity,
            adb_target: None,
        }
    }

    pub fn inited(&self) -> bool {
        self.tasker.inited()
    }

    pub fn storage_readings(&self) -> [Option<crate::storage::ResourceReading>; 3] {
        self.storage_capacity.readings()
    }

    pub fn resource(&self) -> &Resource {
        &self.resource
    }

    pub fn tasker(&self) -> &Tasker {
        &self.tasker
    }

    pub fn bus(&self) -> &EventBus {
        &self.bus
    }

    pub fn start(&self, entry: &str) -> Result<()> {
        self.start_with_strategy(entry, crate::Strategy::default())
    }

    pub fn start_read_army(&self, cards: &[crate::army::ArmyCard]) -> Result<()> {
        if self.running() {
            return Err("请先停止正在运行的流程".into());
        }
        *self.army_read.lock().map_err(|_| "读取状态不可用")? = None;
        self.bus.restart();
        self.tasker
            .post_task("ArmyApply", &crate::army::read_pipeline(cards).to_string())?;
        Ok(())
    }

    pub fn take_read_army(&self) -> Option<crate::army::ArmyPlan> {
        self.army_read.lock().ok()?.take()
    }

    pub fn start_army(&self, plan: &crate::army::ArmyPlan, cards: &[crate::army::ArmyCard]) -> Result<()> {
        if self.running() {
            return Err("请先停止挂机再配卡".into());
        }
        let pipeline = crate::army::pipeline(plan, cards)?;
        self.bus.restart();
        self.tasker.post_task("ArmyApply", &pipeline.to_string())?;
        Ok(())
    }

    pub fn verify_army(&self, plan: &crate::army::ArmyPlan, cards: &[crate::army::ArmyCard]) -> Result<()> {
        if self.running() {
            return Err("请先停止挂机再配卡".into());
        }
        let pipeline = crate::army::verification_pipeline(plan, cards)?;
        self.bus.restart();
        self.tasker.post_task("ArmyApply", &pipeline.to_string())?;
        Ok(())
    }

    pub fn start_with_strategy(&self, entry: &str, strategy: crate::Strategy) -> Result<()> {
        self.start_farming(entry, strategy, true)
    }

    pub fn start_farming(&self, entry: &str, strategy: crate::Strategy, stop_when_full: bool) -> Result<()> {
        self.start_farming_with_loot(entry, strategy, stop_when_full, &crate::loot::LootFilter::default())
    }

    pub fn start_farming_with_loot(
        &self,
        entry: &str,
        strategy: crate::Strategy,
        stop_when_full: bool,
        loot: &crate::loot::LootFilter,
    ) -> Result<()> {
        self.start_farming_with_tactics(entry, strategy, stop_when_full, loot, &crate::tactics::Tactics::default())
    }

    pub fn start_farming_with_tactics(
        &self,
        entry: &str,
        strategy: crate::Strategy,
        stop_when_full: bool,
        loot: &crate::loot::LootFilter,
        tactics: &crate::tactics::Tactics,
    ) -> Result<()> {
        if !self.inited() {
            return Err("Tasker 未初始化（资源或控制器未绑定）".into());
        }
        loot.validate()?;
        tactics.validate()?;
        self.storage_capacity.reset();
        self.bus.restart();
        let mut pipeline = strategy.pipeline_override();
        pipeline.as_object_mut().unwrap().extend(
            crate::storage::pipeline_override(stop_when_full)
                .as_object()
                .unwrap()
                .clone(),
        );
        tactics.apply_override(&mut pipeline)?;
        crate::loot::apply_override(&mut pipeline, loot);
        // Never wait on the returned job: the battle loop is a resident task.
        self.tasker.post_task(entry, &pipeline.to_string())?;
        Ok(())
    }

    pub fn stop(&self, timeout: Duration) {
        if !self.tasker.running() {
            return;
        }
        let _ = self.tasker.post_stop();
        let deadline = Instant::now() + timeout;
        while self.tasker.running() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(50));
        }
    }

    pub fn running(&self) -> bool {
        self.tasker.running()
    }

    /// Stop the resident task and wait for its worker thread to actually leave.
    /// Returns false if the task is still running after the grace period.
    pub fn await_idle(&self, grace: Duration) -> bool {
        let deadline = Instant::now() + grace;
        while self.tasker.running() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(50));
        }
        !self.tasker.running()
    }

    /// The exact frame a recognition ran on. Free in debug mode, and unlike a
    /// second screencap it cannot drift from what was actually matched.
    pub fn recognition_png(&self, reco_id: i64) -> Option<Vec<u8>> {
        let detail = self.tasker.get_recognition_detail(reco_id).ok().flatten()?;
        detail.raw_image
    }

    /// maa-framework 1.25 passes this option as an `i32`; the framework reads a
    /// `bool` and rejects the size mismatch, so set it with the right width.
    /// Upstream issue candidate — remove once the binding is fixed.
    pub fn set_debug_mode(&self, on: bool) -> Result<()> {
        let value: bool = on;
        let ok = unsafe {
            maa_framework::sys::MaaGlobalSetOption(
                maa_framework::sys::MaaGlobalOptionEnum_MaaGlobalOption_DebugMode as i32,
                &value as *const bool as *mut std::ffi::c_void,
                std::mem::size_of::<bool>() as u64,
            )
        };
        if ok == 0 {
            return Err("设置调试模式失败".into());
        }
        Ok(())
    }

    /// Keep only a handful of recognition frames around: the default cache is
    /// 4096 images, which is how the process ends up holding gigabytes.
    pub fn set_reco_cache_limit(limit: usize) -> Result<()> {
        Tasker::set_reco_image_cache_limit(limit).map_err(|e| e.to_string().into())
    }

    /// Query the current ADB transport rather than the controller's cached
    /// connection flag. This does not send input or a shell command to the phone.
    pub fn device_online(&self) -> bool {
        self.adb_target.as_ref().is_some_and(|(path, address)| {
            adb_online(path, address, Duration::from_secs(2))
        })
    }

    pub fn device_address(&self) -> Option<&str> {
        self.adb_target.as_ref().map(|(_, address)| address.as_str())
    }

    pub fn poll(&self) -> Vec<NodeEvent> {
        self.bus.drain()
    }

    /// Events lost because nobody polled fast enough.
    pub fn dropped_events(&self) -> usize {
        self.bus.dropped()
    }

    /// Notifications that are not per-node events and are not surfaced.
    pub fn ignored_events(&self) -> usize {
        self.bus.ignored()
    }

    /// Whether the device is showing a landscape frame. A 20:9 phone legitimately
    /// reports 1600x720, so this must not be an equality test against the 16:9
    /// size the assets were cropped on — only portrait means the game is not on
    /// screen at all (launcher, a crashed app) and nothing can match.
    pub fn is_landscape(&self, width: u32, height: u32) -> bool {
        width > height
    }

    pub fn screencap_png(&self) -> Result<Vec<u8>> {
        let controller = self.controller.as_ref().ok_or("当前无设备，无法截图")?;
        let id = controller.post_screencap()?;
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let status = controller.status(id);
            if status == MaaStatus::SUCCEEDED {
                break;
            }
            if status == MaaStatus::FAILED {
                return Err("截图失败".into());
            }
            if Instant::now() >= deadline {
                return Err("截图超时，请检查设备连接".into());
            }
            thread::sleep(Duration::from_millis(20));
        }
        let image = controller.cached_image()?;
        image.to_vec().ok_or_else(|| "截图缓冲区为空".into())
    }

    /// Run a shell command through the framework's own adb channel and return
    /// whatever it printed. Going through MAA rather than spawning a second adb
    /// client keeps one connection and one set of sockets on the device.
    pub fn shell(&self, cmd: &str) -> Result<String> {
        let controller = self.controller.as_ref().ok_or("当前无设备，无法执行命令")?;
        check(controller.wait(controller.post_shell(cmd, 10_000)?), "执行命令")?;
        Ok(controller.shell_output()?)
    }
}

#[cfg(test)]
mod tests {
    use super::{agent_dir, adb_online};
    use std::{path::Path, time::{Duration, Instant}};

    #[cfg(unix)]
    #[test]
    fn adb_probe_rejects_missing_and_stalled_transport() {
        assert!(!adb_online(Path::new("/nonexistent/adb"), "test", Duration::from_millis(50)));
        // /bin/sleep rejects ADB arguments; it must never be treated as connected.
        assert!(!adb_online(Path::new("/bin/sleep"), "test", Duration::from_millis(50)));
        let start = Instant::now();
        // A blocked process must be killed and reaped rather than blocking UI polling.
        use std::os::unix::fs::PermissionsExt;
        let path = std::env::temp_dir().join(format!("maacoc-adb-stall-{}", std::process::id()));
        std::fs::write(&path, "#!/bin/sh\nexec /bin/sleep 2\n").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert!(!adb_online(&path, "test", Duration::from_millis(50)));
        assert!(start.elapsed() < Duration::from_secs(1));
        std::fs::remove_file(path).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn adb_probe_accepts_only_an_online_device() {
        use std::os::unix::fs::PermissionsExt;
        let path = std::env::temp_dir().join(format!("maacoc-adb-probe-{}", std::process::id()));
        // Simulate adb's actual responses without contacting a phone.
        for (response, exit, expected) in [("device", 0, true), ("offline", 0, false), ("unauthorized", 0, false), ("device", 1, false)] {
            std::fs::write(&path, format!("#!/bin/sh\nprintf '%s\\n' '{response}'\nexit {exit}\n")).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
            assert_eq!(adb_online(&path, "test", Duration::from_secs(1)), expected);
        }
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn resolves_checkout_and_installed_helpers_from_assets() {
        let id = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("maacoc-agents-{}-{id}", std::process::id()));
        let assets = root.join("assets");
        let checkout = root.join("vendor/share/MaaAgentBinary");
        let installed = root.join("MaaAgentBinary");
        std::fs::create_dir_all(&assets).unwrap();
        for dir in [&checkout, &installed] {
            std::fs::create_dir_all(dir.join("maatouch/universal")).unwrap();
            std::fs::write(dir.join("maatouch/universal/maatouch"), b"test helper").unwrap();
        }
        assert_eq!(agent_dir(&assets).unwrap(), installed.canonicalize().unwrap());
        std::fs::remove_dir_all(&installed).unwrap();
        assert_eq!(agent_dir(&assets).unwrap(), checkout.canonicalize().unwrap());
        std::fs::remove_dir_all(root).unwrap();
    }
}
