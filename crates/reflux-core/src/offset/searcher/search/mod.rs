//! Search strategy implementations.
//!
//! This module contains different search strategies for finding memory offsets:
//! - `signature`: AOB/code signature scanning (legacy, currently unused)
//! - The main pattern and relative search logic remains in parent module

pub mod signature;

pub use signature::*;
