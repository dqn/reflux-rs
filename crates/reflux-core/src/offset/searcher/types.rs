//! Types for offset searching

use crate::play::PlayType;
use crate::offset::OffsetsCollection;

/// Judge data for interactive offset searching
#[derive(Debug, Clone, Default)]
pub struct JudgeInput {
    pub pgreat: u32,
    pub great: u32,
    pub good: u32,
    pub bad: u32,
    pub poor: u32,
    pub combo_break: u32,
    pub fast: u32,
    pub slow: u32,
}

/// Search result with address and matching pattern index
#[derive(Debug, Clone)]
pub struct SearchResult {
    pub address: u64,
    pub pattern_index: usize,
}

/// Trait for interactive user prompts during offset search
pub trait SearchPrompter {
    /// Prompt user to press enter to continue
    fn prompt_continue(&self, message: &str);

    /// Prompt user to enter a number
    fn prompt_number(&self, prompt: &str) -> u32;

    /// Display a message to the user
    fn display_message(&self, message: &str);

    /// Display a warning message
    fn display_warning(&self, message: &str);
}

/// Interactive offset search result
#[derive(Debug, Clone)]
pub struct InteractiveSearchResult {
    pub offsets: OffsetsCollection,
    pub play_type: PlayType,
}
