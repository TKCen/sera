//! View modules for the SERA TUI.
//!
//! Each view owns its own selection + scroll state and a `render` method
//! that takes a `ratatui::Frame`.  Input dispatch happens in
//! [`crate::app`] — views stay pure presentation + local UI state.

pub mod agent_list;
pub mod evolve_status;
pub mod hitl_queue;
pub mod session;
