//! Owning the controller/resource/tasker triple for one session.

use crate::events::{EventBus, NodeEvent};
use crate::Result;
use maa_framework::{common::MaaStatus, controller::Controller, resource::Resource, tasker::Tasker, toolkit::Toolkit};
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
    controller: Option<Controller>,
    resource: Resource,
    tasker: Tasker,
    sink_id: i64,
    bus: EventBus,
    pub label: String,
}

fn check(status: MaaStatus, what: &str) -> Result<()> {
    if status == MaaStatus::SUCCEEDED {
        Ok(())
    } else {
        Err(format!("{what} 失败: {status:?}").into())
    }
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
    pub fn connect(assets: &Path, target: DeviceTarget, preferred: Option<&str>) -> Result<Self> {
        if matches!(target, DeviceTarget::Headless) {
            let resource = Resource::new()?;
            check(
                resource.post_bundle(assets.to_str().ok_or("资源路径无效")?)?.wait(),
                "加载资源",
            )?;
            if !resource.loaded() {
                return Err("资源未就绪".into());
            }
            return Ok(Self::assemble(None, resource, "无设备（仅试跑）".into()));
        }
        let (controller, label) = match target {
            DeviceTarget::Headless => return Err("无设备模式不需要建立连接".into()),
            DeviceTarget::Adb => {
                let mut devices: Vec<_> = Toolkit::find_adb_devices()?;
                if devices.is_empty() {
                    return Err("没有找到 ADB 设备，请确认已连接并允许调试".into());
                }
                let device = match preferred {
                    Some(needle) => devices
                        .into_iter()
                        .find(|d| d.address.contains(needle) || d.name.contains(needle))
                        .ok_or_else(|| format!("未找到设备 {needle}"))?,
                    None => devices.remove(0),
                };
                let label = format!("{} @ {}", device.name, device.address);
                let config = serde_json::to_string(&device.config)?;
                let controller = Controller::new_adb(
                    device.adb_path.to_str().ok_or("adb 路径无效")?,
                    &device.address,
                    &config,
                    "",
                )?;
                (controller, label)
            }
        };

        check(controller.wait(controller.post_connection()?), "连接设备")?;
        if !controller.connected() {
            return Err(format!("{} 未能建立连接", label).into());
        }
        let controller = Some(controller);
        let resource = Resource::new()?;
        check(
            resource.post_bundle(assets.to_str().ok_or("资源路径无效")?)?.wait(),
            "加载资源",
        )?;
        if !resource.loaded() {
            return Err("资源未就绪".into());
        }
        Ok(Self::assemble(controller, resource, label))
    }

    fn assemble(controller: Option<Controller>, resource: Resource, label: String) -> Self {
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
        }
    }

    pub fn inited(&self) -> bool {
        self.tasker.inited()
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
        if !self.inited() {
            return Err("Tasker 未初始化（资源或控制器未绑定）".into());
        }
        self.bus.restart();
        // Never wait on the returned job: the battle loop is a resident task.
        self.tasker.post_task(entry, "{}")?;
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

    /// The PNG the framework actually captured, in the 1280x720 match space.
    pub fn screencap_png(&self) -> Result<Vec<u8>> {
        let controller = self.controller.as_ref().ok_or("当前无设备，无法截图")?;
        check(controller.wait(controller.post_screencap()?), "截图")?;
        let image = controller.cached_image()?;
        image.to_vec().ok_or_else(|| "截图缓冲区为空".into())
    }
}
