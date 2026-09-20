//! Reading the committed frame corpus.
//!
//! Each entry records which node was expected to match when the frame was
//! captured, so asset changes fail here instead of mid-battle.

use crate::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FrameRecord {
    pub file: String,
    pub node: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub digest: String,
    pub width: u32,
    pub height: u32,
}

/// PNG header carries width/height at fixed offsets; avoids an image decode.
pub fn png_size(png: &[u8]) -> (u32, u32) {
    if png.len() < 24 || &png[..8] != b"\x89PNG\r\n\x1a\n" {
        return (0, 0);
    }
    let read = |i: usize| u32::from_be_bytes([png[i], png[i + 1], png[i + 2], png[i + 3]]);
    (read(16), read(20))
}

pub struct FrameStore {
    root: PathBuf,
}

impl FrameStore {
    pub fn open(root: impl AsRef<Path>) -> Self {
        Self { root: root.as_ref().to_path_buf() }
    }

    pub fn frames(&self) -> Result<Vec<FrameRecord>> {
        let index = self.root.join("index.jsonl");
        let text = std::fs::read_to_string(&index)
            .map_err(|e| format!("读不到 {} : {e}", index.display()))?;
        let mut out = Vec::new();
        for (line_no, line) in text.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            let record: FrameRecord = serde_json::from_str(line)
                .map_err(|e| format!("第 {} 行索引损坏: {e}", line_no + 1))?;
            if self.root.join(&record.file).is_file() {
                out.push(record);
            }
        }
        Ok(out)
    }

    pub fn load_png(&self, record: &FrameRecord) -> Result<Vec<u8>> {
        Ok(std::fs::read(self.root.join(&record.file))?)
    }

    /// Append one frame plus its index line, so a real session can grow the
    /// offline corpus without a second capture racing the animation.
    pub fn save(&self, png: &[u8], node: &str, label: &str) -> Result<PathBuf> {
        std::fs::create_dir_all(&self.root)?;
        let existing = self.frames().unwrap_or_default().len();
        let safe: String = node
            .chars()
            .map(|c| if c.is_alphanumeric() || matches!(c, '-' | '_' | '.') { c } else { '_' })
            .collect();
        let file = format!("{:04}-{safe}.png", existing + 1);
        let path = self.root.join(&file);
        std::fs::write(&path, png)?;
        let (width, height) = png_size(png);
        let record = FrameRecord {
            file,
            node: node.to_string(),
            label: label.to_string(),
            digest: String::new(),
            width,
            height,
        };
        let line = serde_json::to_string(&record)?;
        use std::io::Write;
        let mut handle = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.root.join("index.jsonl"))?;
        handle.write_all(line.as_bytes())?;
        handle.write_all(b"\n")?;
        Ok(path)
    }
}
