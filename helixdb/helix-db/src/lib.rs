pub mod helix_engine;
pub mod helix_gateway;
#[cfg(feature = "compiler")]
pub mod helixc;
pub mod protocol;
pub mod utils;

#[cfg(feature = "profiling-dhat")]
#[global_allocator]
static GLOBAL: dhat::Alloc = dhat::Alloc;

#[cfg(not(feature = "profiling-dhat"))]
use mimalloc::MiMalloc;

#[cfg(not(feature = "profiling-dhat"))]
#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;
