//! Allocator that fails at link time.
//!
//! The NoAlloc allocator can be registered as the global allocator in order
//! to check at link time that the global allocator isn't used anywhere in the
//! program. This can be useful when using the alloc crate in a no-std context
//! where no global allocator is desired.

/// Allocator which causes linker errors on use.
pub struct NoAlloc;

// Safety: This allocator causes link errors at build time, so it can never
// violate the globalalloc contract
unsafe impl core::alloc::GlobalAlloc for NoAlloc {
    unsafe fn alloc(&self, layout: core::alloc::Layout) -> *mut u8 {
        extern "Rust" {
            fn error_no_global_alloc_present_alloc(layout: core::alloc::Layout) -> *mut u8;
        }
        // Safety: will not exist, so linker will not allow this to be compiled in final binary
        unsafe { error_no_global_alloc_present_alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: core::alloc::Layout) {
        extern "Rust" {
            fn error_no_global_alloc_present_dealloc(ptr: *mut u8, layout: core::alloc::Layout);
        }
        // Safety: will not exist, so linker will not allow this to be compiled in final binary
        unsafe { error_no_global_alloc_present_dealloc(ptr, layout) }
    }
}
