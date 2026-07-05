use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use engine::{FunctionDiff, ParsedModule};
use trace_format::BlobId;

/// Process-lifetime memo of parsed modules and per-function diffs.
#[derive(Default)]
pub struct EngineCache {
    parsed: Mutex<HashMap<i64, Arc<ParsedModule>>>,
    #[allow(dead_code)] // Used by the diff endpoint added in the next task.
    diffs: Mutex<HashMap<(i64, i64, String), Arc<FunctionDiff>>>,
}

impl EngineCache {
    pub fn parsed(&self, blob: BlobId, text: &str) -> Arc<ParsedModule> {
        if let Some(hit) = self.parsed.lock().unwrap().get(&blob.0).cloned() {
            return hit;
        }
        let module = Arc::new(engine::parse_module(text));
        self.parsed.lock().unwrap().insert(blob.0, module.clone());
        module
    }

    #[allow(dead_code)] // Used by the diff endpoint added in the next task.
    pub fn diff<F: FnOnce() -> FunctionDiff>(
        &self,
        before: BlobId,
        after: BlobId,
        func: &str,
        compute: F,
    ) -> Arc<FunctionDiff> {
        let key = (before.0, after.0, func.to_string());
        if let Some(hit) = self.diffs.lock().unwrap().get(&key).cloned() {
            return hit;
        }
        let value = Arc::new(compute());
        self.diffs.lock().unwrap().insert(key, value.clone());
        value
    }
}
