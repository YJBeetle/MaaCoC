//! Reading the committed frame corpus.
//!
//! Each entry records which node was expected to match when the frame was
//! captured, so asset changes fail here instead of mid-battle.

use crate::Result;
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Deserialize)]
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
}
