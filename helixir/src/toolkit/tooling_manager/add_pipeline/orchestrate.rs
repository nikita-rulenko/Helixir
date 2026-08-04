//! `add_memory` orchestrator: extract → embed → search → decide → enrich
//! → resolve cross-memory relations → preserve raw source. Every step
//! lives in a sibling module; this file is the conductor.

use std::collections::HashMap;

use tracing::{debug, info, warn};

use crate::core::rbac::{RbacManager, RbacMemoryScope};
use crate::llm::decision::{MemoryOperation, SimilarMemory};
use crate::llm::extractor::{ExtractedEntity, ExtractedMemory, ExtractedRelation};

use super::super::ToolingManager;
use super::super::types::{AddMemoryResult, ToolingError};
use crate::safe_truncate;

type RelationInferenceJob = (String, String, Vec<(String, String)>);

mod entry;
mod finalization;
mod pipeline;
