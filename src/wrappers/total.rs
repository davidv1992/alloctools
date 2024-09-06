use alloc::alloc::Allocator;
use core::{
    alloc::{AllocError, Layout},
    sync::atomic::{AtomicUsize, Ordering},
};

/// Allocator that imposes a limit on the amount of memory allocated through
/// its structure over it's lifetime.
///
/// It is important to keep in mind that this allocator does not keep track of
/// the overhead caused by the implementation choices of the underlying
/// allocator. Depending on implementation of this allocator, this can be a
/// significant additional memory use.
///
/// Note that freeing memory does not increase the future amount that can be
/// allocated through this allocator. If that is desired, consider a
/// [`BoundedAllocator`](super::BoundedAllocator) instead.

#[derive(Debug)]
pub struct BoundedTotalAllocator<A: Allocator> {
    capacity: usize,
    remaining: AtomicUsize,
    inner: A,
}

impl<A: Allocator> BoundedTotalAllocator<A> {
    /// Create a new [`BoundedTotalAllocator`] that may be used to allocate
    /// `bound` bytes over its existence. Some of these bytes will be used to
    /// allocate state for the allocator.
    pub const fn new(inner: A, bound: usize) -> Self {
        Self {
            capacity: bound,
            remaining: AtomicUsize::new(bound),
            inner,
        }
    }

    fn acquire(&self, amount: usize) -> Result<(), AllocError> {
        let mut remaining = self.remaining.load(Ordering::Relaxed);
        loop {
            match remaining.checked_sub(amount) {
                Some(postremaining) => match self.remaining.compare_exchange_weak(
                    remaining,
                    postremaining,
                    Ordering::Relaxed,
                    Ordering::Relaxed,
                ) {
                    Ok(_) => break Ok(()),
                    Err(rem) => remaining = rem,
                },
                None => return Err(AllocError),
            }
        }
    }

    // On error we haven't allocated, so still available for future use.
    fn release_err(&self, amount: usize) {
        self.remaining.fetch_add(amount, Ordering::Relaxed);
        debug_assert!(self.remaining.load(Ordering::Relaxed) <= self.capacity);
    }
}

// Safety:
// 1) The validity of allocated blocks is guaranteed by the fact that we
//   contain the inner allocator, and hence the inner allocator is around at
//   least as long as we are, and has a lifetime greater than or equal to ours.
// 2) Clone behaviour is guaranteed by the fact that this allocator is not
//   clone.
// 3) Any block can be passed to any function, as that is guaranteed by the
//   underlying allocator.
unsafe impl<A: Allocator> Allocator for BoundedTotalAllocator<A> {
    fn allocate(&self, layout: Layout) -> Result<core::ptr::NonNull<[u8]>, AllocError> {
        self.acquire(layout.size())?;

        self.inner.allocate(layout).map_err(|e| {
            self.release_err(layout.size());
            e
        })
    }

    unsafe fn deallocate(&self, ptr: core::ptr::NonNull<u8>, layout: Layout) {
        // Safety: our caller guarantees the contract on the parameters.
        unsafe { self.inner.deallocate(ptr, layout) }
    }

    fn allocate_zeroed(
        &self,
        layout: Layout,
    ) -> Result<core::ptr::NonNull<[u8]>, core::alloc::AllocError> {
        self.acquire(layout.size())?;

        self.inner.allocate_zeroed(layout).map_err(|e| {
            self.release_err(layout.size());
            e
        })
    }

    unsafe fn grow(
        &self,
        ptr: core::ptr::NonNull<u8>,
        old_layout: Layout,
        new_layout: Layout,
    ) -> Result<core::ptr::NonNull<[u8]>, core::alloc::AllocError> {
        let additional = new_layout.size() - old_layout.size();
        self.acquire(additional)?;

        // Safety: our caller guarantees the contract on the parameters.
        unsafe { self.inner.grow(ptr, old_layout, new_layout) }.map_err(|e| {
            self.release_err(additional);
            e
        })
    }

    unsafe fn grow_zeroed(
        &self,
        ptr: core::ptr::NonNull<u8>,
        old_layout: Layout,
        new_layout: Layout,
    ) -> Result<core::ptr::NonNull<[u8]>, core::alloc::AllocError> {
        let additional = new_layout.size() - old_layout.size();
        self.acquire(additional)?;

        // Safety: our caller guarantees the contract on the parameters.
        unsafe { self.inner.grow_zeroed(ptr, old_layout, new_layout) }.map_err(|e| {
            self.release_err(additional);
            e
        })
    }

    unsafe fn shrink(
        &self,
        ptr: core::ptr::NonNull<u8>,
        old_layout: Layout,
        new_layout: Layout,
    ) -> Result<core::ptr::NonNull<[u8]>, core::alloc::AllocError> {
        // Safety: our caller guarantees the contract on the parameters.
        unsafe { self.inner.shrink(ptr, old_layout, new_layout) }
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;
    use alloc::{alloc::Global, vec::Vec};

    #[test]
    fn bounded_total_allocator_basic() {
        let alloc = BoundedTotalAllocator::new(Global, 1024);

        let mut data: Vec<u8, _> = Vec::with_capacity_in(128, alloc);
        data.extend(core::iter::repeat(0).take(128));
        // should not crash
    }

    #[test]
    #[should_panic]
    fn bounded_total_allocator_enforces_limit() {
        std::alloc::set_alloc_error_hook(|layout| {
            panic!("memory allocation of {} bytes failed", layout.size())
        });
        let alloc = BoundedTotalAllocator::new(Global, 128);

        let mut data: Vec<u8, _> = Vec::with_capacity_in(129, alloc);
        data.extend(core::iter::repeat(0).take(129));
    }

    #[test]
    #[should_panic]
    fn bounded_total_allocator_free_doesnt_return_capacity() {
        std::alloc::set_alloc_error_hook(|layout| {
            panic!("memory allocation of {} bytes failed", layout.size())
        });
        let alloc = BoundedTotalAllocator::new(Global, 1024);

        for _ in 0..16 {
            let mut data: Vec<u8, _> = Vec::with_capacity_in(128, &alloc);
            data.extend(core::iter::repeat(0).take(128));
        }
        // should not crash
    }
}
