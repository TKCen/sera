# ADR-0002: No Blanket Barrel Re-exports in sera-types

### Status

Accepted

### Date

2026-04-17

---

### Context

`sera-types` is the leaf crate in the Rust workspace dependency graph: every other
crate depends on it, and it depends on no internal crates. Because of its position,
the public surface area of `sera-types` propagates to all consumers automatically.
Wide `pub use submodule::*` re-exports (barrel exports) at the crate root cause
several problems in this context:

1. **Name collision risk.** Multiple submodules may define structs with common names
   (`Error`, `Config`, `Status`). A blanket re-export from the crate root brings all
   of them into scope simultaneously, causing ambiguity errors in downstream crates
   or silent shadowing.

2. **Rustdoc noise.** `cargo doc` re-exports every item from every re-exported module
   at the top-level page of `sera-types`, making the generated documentation
   difficult to navigate.

3. **Incremental compilation cost.** Any change to a re-exported submodule
   invalidates the crate root, which in turn invalidates every crate that depends
   on `sera-types`. Fine-grained module paths (`sera_types::capability::FilesystemCapability`)
   allow the compiler to narrow the invalidation scope.

4. **Discoverability.** When code reads `use sera_types::FilesystemCapability` it is
   unclear which submodule owns the type. Requiring the full path
   `use sera_types::capability::FilesystemCapability` makes the module structure
   self-documenting.

**Current state of `sera-types/src/lib.rs` (as of 2026-04-17):**

The file declares 24 `pub mod` statements covering all domain submodules
(`capability`, `manifest`, `agent`, `policy`, `session`, `tool`, etc.).
It contains exactly three `pub use` lines:

```rust
pub use evolution::*;                                          // blanket
pub use versioning::BuildIdentity;                            // single item
pub use content_block::{ContentBlock, ConversationMessage, ConversationRole}; // named set
```

The `pub use evolution::*` blanket on line 35 is the only violation of the
no-barrel policy. The other two re-exports are named and intentional convenience
aliases for the most frequently used cross-cutting types.

---

### Decision

We adopt the following export policy for `sera-types`:

1. **No `pub use submodule::*` at the crate root.** The `pub use evolution::*`
   wildcard on `lib.rs:35` must be replaced with explicit named re-exports covering
   only the items that downstream crates have demonstrated they need at the
   `sera_types::` path.

2. **Named re-exports are permitted** for types that are used directly at the
   `sera_types::` level in more than three downstream call sites, or that form part
   of a stable public API boundary (e.g., `BuildIdentity`, `ContentBlock`). Each
   such re-export must be a single named `pub use` line, not a glob.

3. **New submodules added to `sera-types` must not add blanket re-exports** at the
   crate root. The module path is the canonical access path.

4. **Cross-crate convenience modules** (e.g., `sera-gateway` re-exporting a subset
   of `sera-types` for its own route handlers) are out of scope for this ADR and
   are permitted under local crate policy.

The practical change required by this decision is small: audit `evolution.rs` for
which items are used as `sera_types::X` in other crates, enumerate them explicitly,
and remove the `*` glob.

---

### Alternatives Considered

**A — Allow blanket re-exports, accept the noise, enforce naming conventions
to avoid collisions**

Rejected. Naming conventions are not enforced by the compiler. As the crate grows
from its current 24 modules toward the full domain model, the collision risk
increases proportionally. The rustdoc problem also worsens monotonically with
module count.

**B — Move the most-used types directly into `lib.rs` (no submodules, flat file)**

Rejected. `sera-types` already has types across 24 logical domains. A single flat
file of several thousand lines would be harder to navigate and would eliminate
the module-level documentation structure. The sub-module approach is the correct
Rust idiom for a library of this size.

**C — Introduce a `prelude` submodule (`sera_types::prelude::*`) and document
it as the blessed barrel**

Considered. A `prelude` module is an established Rust pattern (used by `tokio`,
`axum`, etc.) for grouping the most commonly imported items. Rejected for
`sera-types` specifically because SERA is an internal crate, not a public library.
Downstream crates are co-located and can afford the minor verbosity of full paths.
A `prelude` also still requires deliberate curation to avoid accumulating everything
over time.

---

### Consequences

**Positive**

- Crate root API surface is explicit and auditable in a single glance at `lib.rs`.
- Rustdoc shows only intentionally-promoted items at the top level.
- Incremental compilation benefits from narrower invalidation.
- New contributors understand the module hierarchy from the import paths in
  downstream code.

**Negative / Risk**

- The `pub use evolution::*` removal is a minor breaking change within the
  workspace. Any call site using `sera_types::SomeEvolutionType` directly must be
  updated to `sera_types::evolution::SomeEvolutionType` (or retain a named alias
  if the item is high-traffic). The scope of this change is confined to the
  workspace and requires no external coordination.

**Followup work**

- The `pub use evolution::*` glob must be audited and replaced before the next
  crate-level API freeze.

---

### References

- `rust/crates/sera-types/src/lib.rs:35` — the single `pub use evolution::*` barrel
  to be eliminated
- `rust/crates/sera-types/src/lib.rs:36–37` — examples of acceptable named re-exports
- `rust/crates/sera-types/src/evolution.rs` — source module; items in use at
  `sera_types::` path must be enumerated before the glob is removed

---

### Followup Beads

- **sera-types-rm-barrel**: Audit `evolution.rs` public items, identify which are
  used as `sera_types::X` in other crates, replace `pub use evolution::*` with
  explicit named re-exports, and update all affected call sites.
- **sera-types-api-freeze**: Once the barrel is removed, tag the `sera-types` public
  surface with `#[non_exhaustive]` on enums intended to be extended, and document
  the stability policy in `rust/crates/sera-types/README.md`.
