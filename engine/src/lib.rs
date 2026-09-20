//! MaaCoC engine: device access, task lifecycle and recognition tooling.

pub mod events;
pub mod frames;
pub mod runner;
pub mod trial;

pub use events::{EventBus, NodeEvent};
pub use runner::{DeviceTarget, Runner};

/// The one logical coordinate space: MaaFramework matches against a screenshot
/// downscaled to short side 720, and every template/ROI in `assets/pipeline`
/// is authored there. Never switch the controller to raw screenshots.
pub const MATCH_SIZE: (u32, u32) = (1280, 720);

pub type Error = Box<dyn std::error::Error + Send + Sync>;
pub type Result<T> = std::result::Result<T, Error>;
