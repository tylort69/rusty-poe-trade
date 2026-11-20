//! Port of upstream `trade/Cache.ts`: a small TTL cache keyed by JSON.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde::Serialize;

pub const CACHE_TTL_SECS: u64 = 300;

#[derive(Debug)]
pub struct Cache<T> {
    cached: Mutex<HashMap<String, (Instant, T)>>,
}

impl<T> Default for Cache<T> {
    fn default() -> Self {
        Self { cached: Mutex::new(HashMap::new()) }
    }
}

impl<T: Clone> Cache<T> {
    pub fn new() -> Self {
        Self::default()
    }

    fn key(key: &impl Serialize) -> String {
        serde_json::to_string(key).unwrap_or_default()
    }

    pub fn get(&self, key: &impl Serialize) -> Option<T> {
        let k = Self::key(key);
        let mut map = self.cached.lock();
        let now = Instant::now();
        map.retain(|_, (exp, _)| *exp > now);
        map.get(&k).map(|(_, v)| v.clone())
    }

    pub fn set(&self, key: &impl Serialize, value: T, ttl: Duration) {
        if ttl.is_zero() {
            return;
        }
        let k = Self::key(key);
        self.cached.lock().insert(k, (Instant::now() + ttl, value));
    }

    pub fn clear(&self) {
        self.cached.lock().clear();
    }

    /// Upstream `Cache.deriveTtl`.
    pub fn derive_ttl(limiter_count: usize) -> Duration {
        if limiter_count > 0 { Duration::from_secs(CACHE_TTL_SECS) } else { Duration::ZERO }
    }
}
