use crate::ai::AnalysisStatus;
use std::collections::{HashMap, HashSet, VecDeque};
use tokio::sync::Mutex;

pub const MAX_QUEUE: usize = 100;

pub struct AnalysisState {
    pub queue: Mutex<VecDeque<String>>,
    pub queued_ids: Mutex<HashSet<String>>, // O(1) existence check
    pub processing_ids: Mutex<HashSet<String>>, // T-05: Prevent duplicate work
    pub last_hashes: Mutex<HashMap<String, u64>>,
    pub status: Mutex<HashMap<String, AnalysisStatus>>,
}

impl AnalysisState {
    pub fn new() -> Self {
        Self {
            queue: Mutex::new(VecDeque::new()),
            queued_ids: Mutex::new(HashSet::new()),
            processing_ids: Mutex::new(HashSet::new()),
            last_hashes: Mutex::new(HashMap::new()),
            status: Mutex::new(HashMap::new()),
        }
    }

    pub async fn should_analyze(&self, entry_id: &str, content: &str) -> bool {
        let mut hashes = self.last_hashes.lock().await;
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        std::hash::Hash::hash(&content, &mut hasher);
        let current_hash = std::hash::Hasher::finish(&hasher);

        match hashes.get(entry_id) {
            Some(&last_hash) if last_hash == current_hash => false,
            _ => {
                hashes.insert(entry_id.to_string(), current_hash);
                true
            }
        }
    }
}
