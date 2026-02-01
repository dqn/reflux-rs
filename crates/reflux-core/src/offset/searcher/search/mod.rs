//! Search strategy implementations.
//!
//! This module contains different search strategies for finding memory offsets:
//! - `signature`: AOB/code signature scanning (legacy, currently unused)
//! - The main pattern and relative search logic remains in parent module

#[cfg(feature = "legacy-signatures")]
pub mod signature;

#[cfg(feature = "legacy-signatures")]
pub use signature::*;
