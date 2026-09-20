//! MaaCoC engine: device access, task lifecycle and recognition tooling.

pub mod events;
pub mod frames;
pub mod pipeline;
pub mod runner;
pub mod trial;

pub use events::{EventBus, NodeEvent};
pub use runner::{DeviceTarget, Runner};

/// The one logical coordinate space: MaaFramework matches against a screenshot
/// downscaled to short side 720, and every template/ROI in `assets/pipeline`
/// is authored there. Never switch the controller to raw screenshots.
pub const MATCH_SIZE: (u32, u32) = (1280, 720);

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

pub fn list_devices() -> Result<Vec<DeviceInfo>> {
    Ok(maa_framework::toolkit::Toolkit::find_adb_devices()?
        .into_iter()
        .map(|device| DeviceInfo {
            label: device_model(&device).unwrap_or_else(|| device.address.clone()),
            address: device.address.clone(),
            adb_path: device.adb_path.clone(),
        })
        .collect())
}

fn device_model(device: &maa_framework::toolkit::AdbDevice) -> Option<String> {
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
