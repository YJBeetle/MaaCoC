//! Reading and surgically editing the pipeline JSONC files.
//!
//! `assets/pipeline/main.json` is parsed by meojson, which tolerates comments
//! and trailing commas; strict JSON readers do not. Rather than reformatting a
//! hand-tuned file (which destroys diffs), comments and trailing commas are
//! blanked with spaces so every byte offset stays valid, and edits are applied
//! at those offsets — changing one number changes one line.

use crate::Result;
use serde_json::{Map, Value};
use std::{collections::HashMap, path::Path};

type PathKey = Vec<String>;

#[derive(Debug, Default)]
struct Spans {
    values: HashMap<PathKey, (usize, usize)>,
    keys: HashMap<PathKey, (usize, usize)>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Edit {
    pub node: String,
    pub field: String,
    pub value: Value,
}

impl Edit {
    pub fn new(node: impl Into<String>, field: impl Into<String>, value: Value) -> Self {
        Self {
            node: node.into(),
            field: field.into(),
            value,
        }
    }
}

/// Replace comments and trailing commas with spaces, keeping length identical.
pub fn blank_jsonc(text: &str) -> String {
    let bytes: Vec<u8> = text.bytes().collect();
    let mut out = bytes.clone();
    let n = bytes.len();
    let mut in_string = vec![false; n];
    let mut i = 0usize;
    let mut inside = false;
    while i < n {
        let ch = bytes[i];
        if inside {
            in_string[i] = true;
            if ch == b'\\' {
                if i + 1 < n {
                    in_string[i + 1] = true;
                }
                i += 2;
                continue;
            }
            if ch == b'"' {
                inside = false;
            }
            i += 1;
            continue;
        }
        if ch == b'"' {
            inside = true;
            in_string[i] = true;
            i += 1;
            continue;
        }
        if ch == b'/' && i + 1 < n && bytes[i + 1] == b'/' {
            let mut j = i;
            while j < n && bytes[j] != b'\n' {
                if bytes[j] != b'\n' {
                    out[j] = b' ';
                }
                j += 1;
            }
            i = j;
            continue;
        }
        if ch == b'/' && i + 1 < n && bytes[i + 1] == b'*' {
            let mut j = i + 2;
            while j + 1 < n && !(bytes[j] == b'*' && bytes[j + 1] == b'/') {
                j += 1;
            }
            for byte in &mut out[i..(j + 2).min(n)] {
                if *byte != b'\n' {
                    *byte = b' ';
                }
            }
            i = j + 2;
            continue;
        }
        i += 1;
    }

    let blanked: Vec<u8> = out.clone();
    for i in 0..n {
        if blanked[i] != b',' || in_string[i] {
            continue;
        }
        let mut j = i + 1;
        while j < n && (blanked[j] as char).is_whitespace() {
            j += 1;
        }
        if j < n && (blanked[j] == b'}' || blanked[j] == b']') {
            out[i] = b' ';
        }
    }
    String::from_utf8(out).unwrap_or_default()
}

pub fn load(text: &str) -> Result<Value> {
    Ok(serde_json::from_str(&blank_jsonc(text))?)
}

struct Scanner<'a> {
    text: &'a [u8],
    i: usize,
    spans: Spans,
}

impl<'a> Scanner<'a> {
    fn ws(&mut self) {
        while self.i < self.text.len() && (self.text[self.i] as char).is_whitespace() {
            self.i += 1;
        }
    }

    fn string(&mut self) -> Result<String> {
        let start = self.i;
        self.i += 1; // opening quote
        while self.i < self.text.len() {
            match self.text[self.i] {
                b'\\' => self.i += 2,
                b'"' => {
                    self.i += 1;
                    return Ok(serde_json::from_str(std::str::from_utf8(&self.text[start..self.i])?)?);
                }
                _ => self.i += 1,
            }
        }
        Err("未闭合的字符串".into())
    }

    fn value(&mut self, path: PathKey) -> Result<Value> {
        self.ws();
        if self.i >= self.text.len() {
            return Err("文档意外结束".into());
        }
        let start = self.i;
        let value = match self.text[self.i] {
            b'{' => self.object(path.clone())?,
            b'[' => self.array(path.clone())?,
            b'"' => Value::String(self.string()?),
            _ => self.literal()?,
        };
        self.spans.values.insert(path, (start, self.i));
        Ok(value)
    }

    fn literal(&mut self) -> Result<Value> {
        let start = self.i;
        while self.i < self.text.len() {
            let ch = self.text[self.i] as char;
            if ch == ',' || ch == '}' || ch == ']' || ch.is_whitespace() {
                break;
            }
            self.i += 1;
        }
        let token = std::str::from_utf8(&self.text[start..self.i])?;
        Ok(match token {
            "true" => Value::Bool(true),
            "false" => Value::Bool(false),
            "null" => Value::Null,
            other => serde_json::from_str(other)?,
        })
    }

    fn object(&mut self, path: PathKey) -> Result<Value> {
        self.i += 1;
        let mut map = Map::new();
        self.ws();
        if self.i < self.text.len() && self.text[self.i] == b'}' {
            self.i += 1;
            return Ok(Value::Object(map));
        }
        loop {
            self.ws();
            let key_start = self.i;
            let key = self.string()?;
            let key_end = self.i;
            let mut child = path.clone();
            child.push(key.clone());
            self.spans.keys.insert(child.clone(), (key_start, key_end));
            self.ws();
            self.i += 1; // ':'
            let value = self.value(child)?;
            map.insert(key, value);
            self.ws();
            if self.i < self.text.len() && self.text[self.i] == b',' {
                self.i += 1;
                self.ws();
                if self.i < self.text.len() && self.text[self.i] == b'}' {
                    self.i += 1;
                    return Ok(Value::Object(map));
                }
                continue;
            }
            if self.i < self.text.len() && self.text[self.i] == b'}' {
                self.i += 1;
                return Ok(Value::Object(map));
            }
            return Err("期望 ',' 或 '}'".into());
        }
    }

    fn array(&mut self, path: PathKey) -> Result<Value> {
        self.i += 1;
        let mut items = Vec::new();
        self.ws();
        if self.i < self.text.len() && self.text[self.i] == b']' {
            self.i += 1;
            return Ok(Value::Array(items));
        }
        loop {
            let mut child = path.clone();
            child.push(items.len().to_string());
            let value = self.value(child)?;
            items.push(value);
            self.ws();
            if self.i < self.text.len() && self.text[self.i] == b',' {
                self.i += 1;
                self.ws();
                if self.i < self.text.len() && self.text[self.i] == b']' {
                    self.i += 1;
                    return Ok(Value::Array(items));
                }
                continue;
            }
            if self.i < self.text.len() && self.text[self.i] == b']' {
                self.i += 1;
                return Ok(Value::Array(items));
            }
            return Err("期望 ',' 或 ']'".into());
        }
    }
}

pub struct PipelineDoc {
    path: Option<std::path::PathBuf>,
    text: String,
}

impl PipelineDoc {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        Ok(Self {
            path: Some(path.clone()),
            text: std::fs::read_to_string(path)?,
        })
    }

    pub fn from_text(text: impl Into<String>) -> Self {
        Self {
            path: None,
            text: text.into(),
        }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn data(&self) -> Result<Value> {
        load(&self.text)
    }

    fn scan(&self) -> Result<(Value, Spans)> {
        let blanked = blank_jsonc(&self.text);
        let mut scanner = Scanner {
            text: blanked.as_bytes(),
            i: 0,
            spans: Spans::default(),
        };
        let value = scanner.value(Vec::new())?;
        Ok((value, scanner.spans))
    }

    pub fn node_names(&self) -> Result<Vec<String>> {
        let (value, spans) = self.scan()?;
        let mut names: Vec<(usize, String)> = Vec::new();
        if let Some(map) = value.as_object() {
            for key in map.keys() {
                let start = spans.keys.get(&vec![key.clone()]).map(|s| s.0).unwrap_or(0);
                names.push((start, key.clone()));
            }
        }
        names.sort();
        Ok(names.into_iter().map(|(_, name)| name).collect())
    }

    pub fn node(&self, name: &str) -> Result<Value> {
        Ok(self.data()?.get(name).cloned().unwrap_or(Value::Null))
    }

    fn line_indent(&self, offset: usize) -> String {
        let start = self.text[..offset].rfind('\n').map(|i| i + 1).unwrap_or(0);
        self.text[start..offset]
            .chars()
            .take_while(|c| *c == ' ' || *c == '\t')
            .collect()
    }

    /// Apply edits at byte offsets, leaving every other byte untouched.
    pub fn patched(&self, edits: &[Edit]) -> Result<String> {
        let (_, spans) = self.scan()?;
        let mut replacements: Vec<(usize, usize, String)> = Vec::new();

        for edit in edits {
            let node_path = vec![edit.node.clone()];
            let field_path = vec![edit.node.clone(), edit.field.clone()];
            if let Some((start, end)) = spans.values.get(&field_path) {
                let original = &self.text[*start..*end];
                let anchor = spans.keys.get(&field_path).map(|key| key.0).unwrap_or(*start);
                let rendered = self.render_value(edit, original, &self.line_indent(anchor))?;
                replacements.push((*start, *end, rendered));
            } else {
                let (_, node_end) = *spans
                    .values
                    .get(&node_path)
                    .ok_or_else(|| format!("未知节点: {}", edit.node))?;
                replacements.push((
                    node_end - 1,
                    node_end - 1,
                    self.render_insert(&spans, &node_path, edit)?,
                ));
            }
        }

        let mut out = self.text.clone();
        for (start, end, rendered) in replacements.into_iter().rev() {
            out.replace_range(start..end, &rendered);
        }
        load(&out)?; // refuse to emit anything that no longer parses
        Ok(out)
    }

    fn render_value(&self, edit: &Edit, original: &str, indent: &str) -> Result<String> {
        if let Value::Array(items) = &edit.value {
            if original.contains('\n') {
                return Ok(Self::multiline(items, indent));
            }
        }
        Ok(serde_json::to_string(&edit.value)?)
    }

    /// Insert a field the node does not have yet, matching the file's style.
    fn render_insert(&self, spans: &Spans, node_path: &PathKey, edit: &Edit) -> Result<String> {
        let value_json = serde_json::to_string(&edit.value)?;
        let mut last_value_end: Option<usize> = None;
        let mut first_key_start: Option<usize> = None;
        for (key, key_span) in &spans.keys {
            if key.len() == 2 && key[0] == node_path[0] {
                if let Some(value_span) = spans.values.get(key) {
                    last_value_end = Some(last_value_end.map_or(value_span.1, |m| m.max(value_span.1)));
                }
                first_key_start = Some(first_key_start.map_or(key_span.0, |m| m.min(key_span.0)));
            }
        }
        let Some(end) = last_value_end else {
            return Ok(format!("\"{}\": {value_json}", edit.field));
        };
        let indent = self.line_indent(first_key_start.unwrap_or(end));
        let bytes = self.text.as_bytes();
        let mut probe = end;
        while probe < bytes.len() && (bytes[probe] as char).is_whitespace() {
            probe += 1;
        }
        // The hand-written files keep a trailing comma, so a leading one is
        // usually unnecessary.
        let already_separated = probe < bytes.len() && bytes[probe] == b',';
        let prefix = if already_separated { "" } else { "," };
        Ok(format!("{prefix}\n{indent}\"{}\": {value_json}", edit.field))
    }

    fn multiline(items: &[Value], indent: &str) -> String {
        let mut out = String::from("[\n");
        for item in items {
            out.push_str(&format!(
                "{indent}    {},\n",
                serde_json::to_string(item).unwrap_or_default()
            ));
        }
        out.push_str(&format!("{indent}]"));
        out
    }

    pub fn write(&mut self, edits: &[Edit]) -> Result<String> {
        let patched = self.patched(edits)?;
        if let Some(path) = &self.path {
            std::fs::write(path, &patched)?;
        }
        self.text = patched.clone();
        Ok(patched)
    }

    pub fn diff(&self, edits: &[Edit]) -> Result<String> {
        let patched = self.patched(edits)?;
        Ok(unified_diff(&self.text, &patched, "a/main.json", "b/main.json"))
    }
}

/// Unified diff of what a patch would change.
pub fn unified_diff(before: &str, after: &str, left: &str, right: &str) -> String {
    use similar::{ChangeTag, TextDiff};
    let diff = TextDiff::from_lines(before, after);
    let mut out = format!("--- {left}\n+++ {right}\n");
    for group in diff.grouped_ops(3) {
        for op in &group {
            for change in diff.iter_changes(op) {
                let marker = match change.tag() {
                    ChangeTag::Delete => '-',
                    ChangeTag::Insert => '+',
                    ChangeTag::Equal => ' ',
                };
                out.push(marker);
                out.push_str(change.value());
                if change.missing_newline() {
                    out.push('\n');
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn repo_pipeline() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("assets/pipeline/main.json")
    }

    #[test]
    fn parses_the_real_hand_written_file() {
        let text = std::fs::read_to_string(repo_pipeline()).unwrap();
        let doc = PipelineDoc::from_text(text);
        let names = doc.node_names().unwrap();
        assert_eq!(names[..3], ["Main", "SetScreen", "Launch"]);
        assert!(names.contains(&"AttackLoop".to_string()));
        assert_eq!(doc.node("FindSoldier").unwrap()["threshold"], serde_json::json!(0.95));
    }

    #[test]
    fn blanking_preserves_length_and_offsets() {
        let text = "{\"a\": 1, // hi\n}";
        let blanked = blank_jsonc(text);
        assert_eq!(blanked.len(), text.len());
        assert_eq!(blanked.matches('\n').count(), text.matches('\n').count());
        assert_eq!(load(text).unwrap()["a"], serde_json::json!(1));
    }

    #[test]
    fn commas_inside_strings_are_not_trailing() {
        assert_eq!(load("{\"a\": \"x,y\", }").unwrap()["a"], serde_json::json!("x,y"));
    }

    #[test]
    fn scalar_edit_changes_exactly_one_line() {
        let doc = PipelineDoc::from_text(std::fs::read_to_string(repo_pipeline()).unwrap());
        let patched = doc
            .patched(&[Edit::new("FindSoldier", "threshold", serde_json::json!(0.8))])
            .unwrap();
        let changed: Vec<_> = doc
            .text()
            .lines()
            .zip(patched.lines())
            .filter(|(a, b)| a != b)
            .collect();
        assert_eq!(changed.len(), 1, "只该有一行不同，实际 {changed:?}");
        assert_eq!(
            load(&patched).unwrap()["FindSoldier"]["threshold"],
            serde_json::json!(0.8)
        );
    }

    #[test]
    fn list_edit_keeps_multiline_layout() {
        let doc = PipelineDoc::from_text(std::fs::read_to_string(repo_pipeline()).unwrap());
        let before = doc.node("AttackLoop").unwrap()["next"].as_array().unwrap().clone();
        let mut after = before.clone();
        after.push(serde_json::json!("Deploy"));
        let patched = doc
            .patched(&[Edit::new("AttackLoop", "next", serde_json::json!(after))])
            .unwrap();
        let diff = unified_diff(doc.text(), &patched, "a", "b");
        assert_eq!(
            diff.lines()
                .filter(|l| l.starts_with('-') && !l.starts_with("---"))
                .count(),
            0,
            "不该有删除行:\n{diff}"
        );
        assert_eq!(
            diff.lines()
                .filter(|l| l.starts_with('+') && !l.starts_with("+++"))
                .count(),
            1
        );
    }

    #[test]
    fn missing_field_is_inserted_after_the_last_one() {
        let doc = PipelineDoc::from_text("{\n  \"Node\": {\n    \"action\": \"Click\",\n  },\n}\n");
        let patched = doc
            .patched(&[Edit::new("Node", "threshold", serde_json::json!(0.9))])
            .unwrap();
        let node = load(&patched).unwrap()["Node"].clone();
        assert_eq!(node["threshold"], serde_json::json!(0.9));
        assert_eq!(node["action"], serde_json::json!("Click"));
    }

    #[test]
    fn unknown_node_is_an_error_not_a_silent_write() {
        let doc = PipelineDoc::from_text(std::fs::read_to_string(repo_pipeline()).unwrap());
        assert!(doc
            .patched(&[Edit::new("NoSuchNode", "threshold", serde_json::json!(0.5))])
            .is_err());
    }

    #[test]
    fn a_broken_result_is_refused() {
        // Guard: the patched text must still parse, or nothing is written.
        let doc = PipelineDoc::from_text("{\n  \"N\": {\"a\": 1},\n}\n");
        assert!(doc.patched(&[Edit::new("N", "a", serde_json::json!([1, 2]))]).is_ok());
    }
}
