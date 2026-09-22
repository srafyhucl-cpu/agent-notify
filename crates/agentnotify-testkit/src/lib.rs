//! 假适配器与契约测试工具，只依赖核心边界，不访问真实网络或用户目录。

mod clock;
mod fake_agent;
mod fake_channel;
mod memory_store;
mod temp;

pub use clock::{FakeClock, SequenceIdGenerator};
pub use fake_agent::{FakeAgent, FakeAgentMode};
pub use fake_channel::FakeChannel;
pub use fake_channel::fake_skip_reason;
pub use memory_store::MemoryStore;
pub use temp::{TEST_TEMP_DIR_ENV, test_temp_root};
