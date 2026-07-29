// Re-export from the shared value resolver in the generate module.
// This module exists for backward compatibility — new code should
// import directly from crate::generate::value_resolver.
pub use crate::generate::value_resolver::*;
