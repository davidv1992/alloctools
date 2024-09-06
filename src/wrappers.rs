//! Wrappers to provide additional functionality for allocators.
//!
//! These wrappers can be used to enforce bounds or gather statistics about
//! the allocator use.
mod bounded;
mod total;

pub use bounded::BoundedAllocator;
pub use total::BoundedTotalAllocator;
