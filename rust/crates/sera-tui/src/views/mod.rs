//! View trait and implementations.

pub mod agents;
pub mod agent_detail;
pub mod knowledge;
pub mod logs;

use ratatui::prelude::*;

/// Trait for renderable views.
pub trait View {
    fn render(&self, frame: &mut Frame, area: Rect);
}
