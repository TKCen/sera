//! Generation marker — binary identity for event context.

use crate::envelope::GenerationMarker;

/// Construct the generation marker for the current binary.
pub fn current_generation() -> GenerationMarker {
    GenerationMarker::current()
}
