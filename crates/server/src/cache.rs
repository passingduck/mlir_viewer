use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};

use engine::{FunctionDiff, ParsedModule, ResolvedFunction, TimelineStage};
use trace_format::BlobId;

/// Process-lifetime memo of parsed modules and per-function diffs.
pub struct EngineCache {
    parsed: Mutex<HashMap<i64, Arc<ParsedModule>>>,
    diffs: Mutex<HashMap<(i64, i64, String), Arc<FunctionDiff>>>,
    timelines: Mutex<TimelineCache>,
    resolved: Mutex<HistoryCache>,
}

struct TimelineCache {
    entries: HashMap<String, Arc<Vec<TimelineStage>>>,
    order: VecDeque<String>,
    capacity: usize,
}

struct HistoryCache {
    entries: HashMap<String, Arc<ResolvedFunction>>,
    order: VecDeque<String>,
    chain_count: usize,
    chain_capacity: usize,
}

fn touch(order: &mut VecDeque<String>, key: &str) {
    order.retain(|entry| entry != key);
    order.push_back(key.to_string());
}

impl Default for EngineCache {
    fn default() -> Self {
        Self::new(128, 2048)
    }
}

impl EngineCache {
    #[cfg(test)]
    fn with_capacities(timeline_capacity: usize, history_capacity: usize) -> Self {
        Self::new(timeline_capacity, history_capacity)
    }

    fn new(timeline_capacity: usize, history_capacity: usize) -> Self {
        Self {
            parsed: Mutex::new(HashMap::new()),
            diffs: Mutex::new(HashMap::new()),
            timelines: Mutex::new(TimelineCache {
                entries: HashMap::new(),
                order: VecDeque::new(),
                capacity: timeline_capacity,
            }),
            resolved: Mutex::new(HistoryCache {
                entries: HashMap::new(),
                order: VecDeque::new(),
                chain_count: 0,
                chain_capacity: history_capacity,
            }),
        }
    }

    pub fn parsed(&self, blob: BlobId, text: &str) -> Arc<ParsedModule> {
        if let Some(hit) = self.parsed.lock().unwrap().get(&blob.0).cloned() {
            return hit;
        }
        let module = Arc::new(engine::parse_module(text));
        self.parsed.lock().unwrap().insert(blob.0, module.clone());
        module
    }

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

    pub fn timeline(&self, function: &str) -> Option<Arc<Vec<TimelineStage>>> {
        let mut cache = self.timelines.lock().unwrap();
        let value = cache.entries.get(function).cloned();
        if value.is_some() {
            touch(&mut cache.order, function);
        }
        value
    }

    pub fn put_timeline(&self, function: &str, value: Arc<Vec<TimelineStage>>) {
        let mut cache = self.timelines.lock().unwrap();
        if cache.capacity == 0 {
            return;
        }
        cache.entries.insert(function.to_string(), value);
        touch(&mut cache.order, function);
        while cache.entries.len() > cache.capacity {
            if let Some(evicted) = cache.order.pop_front() {
                cache.entries.remove(&evicted);
            }
        }
    }

    pub fn resolved(&self, function: &str) -> Option<Arc<ResolvedFunction>> {
        let mut cache = self.resolved.lock().unwrap();
        let value = cache.entries.get(function).cloned();
        if value.is_some() {
            touch(&mut cache.order, function);
        }
        value
    }

    pub fn put_resolved(&self, function: &str, value: Arc<ResolvedFunction>) {
        let mut cache = self.resolved.lock().unwrap();
        if cache.chain_capacity == 0 {
            return;
        }
        if let Some(previous) = cache.entries.insert(function.to_string(), value.clone()) {
            cache.chain_count = cache.chain_count.saturating_sub(previous.histories.len());
        }
        cache.chain_count += value.histories.len();
        touch(&mut cache.order, function);
        while cache.chain_count > cache.chain_capacity && cache.entries.len() > 1 {
            let Some(evicted) = cache.order.pop_front() else {
                break;
            };
            if let Some(removed) = cache.entries.remove(&evicted) {
                cache.chain_count = cache.chain_count.saturating_sub(removed.histories.len());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    use engine::{OpHistory, OpUid, ResolvedFunction, TimelineStage};

    fn timeline(pass_id: i64) -> Arc<Vec<TimelineStage>> {
        Arc::new(vec![TimelineStage {
            pass_id,
            pass_name: format!("pass-{pass_id}"),
            before: None,
            after: None,
            events: Vec::new(),
        }])
    }

    fn resolved(function: &str, uid: &str) -> Arc<ResolvedFunction> {
        let uid = OpUid::parse(uid).unwrap();
        Arc::new(ResolvedFunction {
            function: function.into(),
            selectable: HashMap::new(),
            histories: HashMap::from([(
                uid.clone(),
                OpHistory {
                    uid,
                    first_name: "test.op".into(),
                    last_name: "test.op".into(),
                    steps: Vec::new(),
                },
            )]),
        })
    }

    #[test]
    fn provenance_caches_evict_least_recently_used_entries() {
        let cache = super::EngineCache::with_capacities(2, 2);
        cache.put_timeline("a", timeline(1));
        cache.put_timeline("b", timeline(2));
        assert!(cache.timeline("a").is_some());
        cache.put_timeline("c", timeline(3));
        assert!(cache.timeline("a").is_some());
        assert!(cache.timeline("b").is_none());
        assert!(cache.timeline("c").is_some());

        cache.put_resolved("a", resolved("a", "op1.YQ.1.b.0"));
        cache.put_resolved("b", resolved("b", "op1.Yg.1.b.0"));
        assert!(cache.resolved("a").is_some());
        cache.put_resolved("c", resolved("c", "op1.Yw.1.b.0"));
        assert!(cache.resolved("a").is_some());
        assert!(cache.resolved("b").is_none());
        assert!(cache.resolved("c").is_some());
    }
}
