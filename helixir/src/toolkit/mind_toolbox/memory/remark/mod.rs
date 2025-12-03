

pub mod models;
pub mod single;
pub mod batch;
pub mod pipeline;

pub use models::{RemarkResult, RemarkStats, UnmarkedMemory};
pub use pipeline::ReMarkupPipeline;
pub use batch::{get_unmarked_memories, remark_batch, remark_all_unmarked};
pub use single::remark_single_memory;

