//! Input simulation for game interaction.
//!
//! This module provides keyboard input simulation and window management
//! for navigating the INFINITAS song select screen.

pub mod keyboard;
pub mod navigator;
pub mod window;

pub use keyboard::{GameKey, send_key_press};
pub use navigator::{NavigationResult, SongNavigator};
