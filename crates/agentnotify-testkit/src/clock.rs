use std::sync::{Arc, RwLock};

use agentnotify_application::Clock;
use agentnotify_domain::Timestamp;

#[derive(Clone, Debug)]
pub struct FakeClock {
    now: Arc<RwLock<Timestamp>>,
}

impl FakeClock {
    pub fn new(start: Timestamp) -> Self {
        Self {
            now: Arc::new(RwLock::new(start)),
        }
    }

    pub fn advance(&self, duration: time::Duration) {
        let mut now = self.now.write().expect("假时钟写锁不应失败");
        *now = now.checked_add(duration).expect("假时钟推进不能溢出");
    }
}

impl Clock for FakeClock {
    fn now(&self) -> Timestamp {
        *self.now.read().expect("假时钟读锁不应失败")
    }
}

#[derive(Clone, Debug)]
pub struct SequenceIdGenerator {
    next: Arc<std::sync::atomic::AtomicU64>,
}

impl Default for SequenceIdGenerator {
    fn default() -> Self {
        Self {
            next: Arc::new(std::sync::atomic::AtomicU64::new(1)),
        }
    }
}

impl agentnotify_application::IdGenerator for SequenceIdGenerator {
    fn next_id(&self) -> String {
        let value = self.next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        format!("generated-{value}")
    }
}
