//! Grafeo schema entities: `VertexEntity`, `EdgeEntity`, `OrderedCollection`.
//!
//! These entities are bound to `lorosurgeon`'s `Hydrate`/`Reconcile` derives
//! and the `LoroProperty` value type. They are available whenever `bridge`
//! is on (which is the minimal useful feature).

pub mod edge;
pub mod tree;
pub mod vertex;

pub use edge::EdgeEntity;
pub use tree::{OrderedCollection, TreeNode};
pub use vertex::VertexEntity;
