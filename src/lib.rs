//! alloctools provides a number of tools for working with custom allocators in
//! rust. It's [`wrappers`] provide useful methods for limiting allocation and
//! deallocation, as well as allocators that can provide runtime statistics.
//!
//! The [`noalloc`] module also provides tooling to use alloc in no_std environments
//! where a global allocator is undesirable.
#![no_std]
#![forbid(missing_docs)]
#![forbid(unsafe_op_in_unsafe_fn)]
#![feature(allocator_api)]
#![cfg_attr(test, feature(alloc_error_hook))]
extern crate alloc;

pub mod noalloc;
pub mod wrappers;
