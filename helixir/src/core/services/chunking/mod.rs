

mod config;
mod events;
mod splitter;
mod service;

pub use config::{ChunkingConfig, ChunkingStrategy};
pub use events::{
    ChunkCreatedEvent, ChunkingCompleteEvent, ChunkingFailedEvent,
    ChunkingStartedEvent, ChunkLinkedEvent, ChunkChainedEvent,
    MemoryCreatedEvent,
};
pub use splitter::{ContentSplitter, SentenceSplitter, SemanticSplitter, TextChunk, SplitterError};
pub use service::{ChunkingService, ChunkingEvent};

