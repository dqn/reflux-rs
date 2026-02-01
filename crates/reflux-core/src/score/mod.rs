//! Score-related types and data structures.
//!
//! This module contains types for representing scores and results:
//! - `Grade` - letter grades (F, E, D, C, B, A, AA, AAA)
//! - `Lamp` - clear lamps (NO PLAY, FAILED, ASSIST, EASY, CLEAR, HARD, EX HARD, FC, PFC)
//! - `Judge` - judge data from a play
//! - `ScoreData`, `ScoreMap` - score storage

mod grade;
mod judge;
mod lamp;
mod score_map;

pub use grade::*;
pub use judge::*;
pub use lamp::*;
pub use score_map::*;
