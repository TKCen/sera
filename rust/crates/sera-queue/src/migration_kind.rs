/// Describes how a database migration may be rolled back.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MigrationKind {
    /// Has both an up and a down file.
    Reversible,
    /// Has an up file and a paired "out" file but no traditional down file.
    ForwardOnlyWithPairedOut,
    /// Has only an up file; cannot be rolled back.
    Irreversible,
}

impl MigrationKind {
    /// Returns `true` if this migration kind requires a down (or paired-out) file.
    pub fn requires_down_file(&self) -> bool {
        match self {
            MigrationKind::Reversible => true,
            MigrationKind::ForwardOnlyWithPairedOut => true,
            MigrationKind::Irreversible => false,
        }
    }
}
