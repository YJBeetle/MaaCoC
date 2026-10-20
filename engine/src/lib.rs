//! MaaCoC engine: device access, task lifecycle and recognition tooling.

pub mod army;
pub mod events;
pub mod frames;
pub mod loot;
pub mod pipeline;
pub mod runner;
pub mod storage;
pub mod strategy;
pub mod tactics;
pub mod trial;

pub use events::{EventBus, NodeEvent};
pub use runner::{DeviceTarget, Runner};
pub use strategy::Strategy;

/// The one logical coordinate space: MaaFramework matches against a screenshot
/// downscaled to short side 720, and every template/ROI in `assets/pipeline`
/// is authored there. Never switch the controller to raw screenshots.
pub const MATCH_SIZE: (u32, u32) = (1280, 720);

/// The phone's logical size to set with `wm size`: landscape 1920x1080 is the
/// only 16:9 panel that downscales to MATCH_SIZE exactly. Taller stock sizes
/// (20:9 → 1600x720) leave the game letterboxed inside the frame.
pub const DEVICE_MATCH_SIZE: &str = "1080x1920";

/// Where MaaFramework writes its own log file. Left unset it writes to
/// `./debug`, which is neither writable nor findable from a bundled app.
pub fn set_log_dir(dir: &std::path::Path) -> Result<()> {
    let text = dir.to_str().ok_or("日志路径不是有效 UTF-8")?;
    maa_framework::configure_logging(text).map_err(|e| e.to_string())?;
    Ok(())
}

/// A device the shell can offer the user.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceInfo {
    /// What to show in a picker: the model when adb can read it, the serial
    /// otherwise. MaaFramework's own `name` is "serial-adb path", which is
    /// useless in a dropdown.
    pub label: String,
    pub address: String,
    pub adb_path: std::path::PathBuf,
}

/// MaaFramework's own finder only runs `search_path("adb")`, so it sees exactly
/// what PATH holds. A `.app` started by Finder gets `/usr/bin:/bin:/usr/sbin:/sbin`
/// with no shell profile behind it, discovers no adb, and then reports an empty
/// device list even though the phone is connected — indistinguishable from "phone
/// not authorised". Probe the usual install locations instead.
pub fn discover_adb_devices() -> Result<Vec<maa_framework::toolkit::AdbDevice>> {
    let found = maa_framework::toolkit::Toolkit::find_adb_devices()?;
    if !found.is_empty() {
        return Ok(found);
    }
    for adb in installed_adbs() {
        let devices = maa_framework::toolkit::Toolkit::find_adb_devices_with_adb(&adb.to_string_lossy())?;
        if !devices.is_empty() {
            return Ok(devices);
        }
    }
    Ok(found)
}

/// adb binaries that actually exist on this machine.
pub fn installed_adbs() -> Vec<std::path::PathBuf> {
    let mut candidates = Vec::new();
    for key in ["ANDROID_HOME", "ANDROID_SDK_ROOT"] {
        if let Ok(root) = std::env::var(key) {
            candidates.push(std::path::PathBuf::from(root).join("platform-tools").join("adb"));
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        let home = std::path::PathBuf::from(home);
        candidates.push(home.join("Library/Android/sdk/platform-tools/adb"));
        candidates.push(home.join("Android/Sdk/platform-tools/adb"));
    }
    candidates.push(std::path::PathBuf::from("/opt/homebrew/bin/adb"));
    candidates.push(std::path::PathBuf::from("/usr/local/bin/adb"));
    candidates.retain(|path| path.is_file());
    candidates
}

/// The framework returns an empty list for both "no adb" and "no phone", but the
/// user has to do opposite things about them.
pub fn no_device_hint() -> &'static str {
    if installed_adbs().is_empty() {
        "找不到 adb 可执行文件，请安装 Android SDK platform-tools 或把 adb 加入 PATH"
    } else {
        "没有找到 ADB 设备，请确认已连接并允许调试"
    }
}

pub fn list_devices() -> Result<Vec<DeviceInfo>> {
    Ok(discover_adb_devices()?
        .into_iter()
        .map(|device| DeviceInfo {
            label: device_model(&device).unwrap_or_else(|| device.address.clone()),
            address: device.address.clone(),
            adb_path: device.adb_path.clone(),
        })
        .collect())
}

pub(crate) fn device_model(device: &maa_framework::toolkit::AdbDevice) -> Option<String> {
    let output = std::process::Command::new(&device.adb_path)
        .args([
            "-s",
            &device.address,
            "shell",
            "getprop ro.product.marketname; getprop ro.product.model",
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    // marketname is what a person recognises ("POCO F3"); model is the type code
    // ("M2012K11AG"). Both may be empty depending on the ROM.
    let text = String::from_utf8_lossy(&output.stdout);
    let mut lines = text.lines().map(str::trim);
    let (market, model) = (lines.next().unwrap_or_default(), lines.next().unwrap_or_default());
    let name = if !market.is_empty() {
        market
    } else if !model.is_empty() {
        model
    } else {
        return None;
    };
    Some(name.to_string())
}

pub type Error = Box<dyn std::error::Error + Send + Sync>;
pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::DeviceInfo;

    #[test]
    fn device_info_serializes_for_the_frontend() {
        let json = serde_json::to_string(&DeviceInfo {
            label: "POCO F3".into(),
            address: "f5d66ad2".into(),
            adb_path: "/sdk/platform-tools/adb".into(),
        })
        .unwrap();
        assert!(json.contains("\"label\":\"POCO F3\""), "{json}");
        assert!(json.contains("\"adbPath\""), "{json}");
    }
}
