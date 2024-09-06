use alloc::alloc::Allocator;
use core::{
    alloc::{AllocError, Layout},
    ptr::NonNull,
    sync::atomic::{AtomicUsize, Ordering},
};

/// Allocator that imposes a limit on the amount of memory allocated through
/// its structure. Note that this allocator needs a small amount of space for
/// bookkeeping.
///
/// It is important to keep in mind that this allocator does not keep track of
/// the overhead caused by the implementation choices of the underlying
/// allocator. Depending on implementation of this allocator, this can be a
/// significant additional memory use.
///
/// This allocator simply limits the maximum of bytes allocated at any one
/// moment in time. This may not always be what is desired. If instead the
/// maximum number of total bytes allocated through the allocator must be
/// limited, consider a [`BoundedTotalAllocator`](super::BoundedTotalAllocator).
///
/// Performance implications:
/// This allocator needs to do some bookkeeping to keep track of the ammount of
/// memory used. To simplify this, it only ever returns exactly fitting
/// allocations, even if the underlying allocator provides larger blocks. This
/// may limit performance of containers that normally utilize this additional
/// capacity.
#[derive(Debug)]
pub struct BoundedAllocator<A: Allocator> {
    capacity: usize,
    remaining: AtomicUsize,
    inner: A,
}

impl<A: Allocator> BoundedAllocator<A> {
    /// Create a new [`BoundedAllocator`] that may be used to allocate `bound`
    /// bytes at any one time. Some of these bytes will be used to allocate
    /// state for the allocator.
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
                    Ordering::Acquire,
                    Ordering::Relaxed,
                ) {
                    Ok(_) => break Ok(()),
                    Err(rem) => remaining = rem,
                },
                None => return Err(AllocError),
            }
        }
    }

    // On error, there is no memory free for a happens-before bound to matter
    fn release_err(&self, amount: usize) {
        self.remaining.fetch_add(amount, Ordering::Relaxed);
        debug_assert!(self.remaining.load(Ordering::Relaxed) <= self.capacity);
    }

    // On actual free, ensure a happens before on the actual free operation.
    fn release(&self, amount: usize) {
        self.remaining.fetch_add(amount, Ordering::Release);
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
unsafe impl<A: Allocator> Allocator for BoundedAllocator<A> {
    fn allocate(&self, layout: Layout) -> Result<core::ptr::NonNull<[u8]>, AllocError> {
        self.acquire(layout.size())?;

        match self.inner.allocate(layout) {
            Ok(allocation) => {
                // Must shrink to request to ensure bookkeeping works
                Ok(NonNull::slice_from_raw_parts(
                    allocation.cast(),
                    layout.size(),
                ))
            }
            Err(e) => {
                // undo the reservation
                self.release_err(layout.size());
                Err(e)
            }
        }
    }

    unsafe fn deallocate(&self, ptr: core::ptr::NonNull<u8>, layout: Layout) {
        // Safety: our caller guarantees the contract on the parameters.
        unsafe { self.inner.deallocate(ptr, layout) };

        self.release(layout.size());
    }

    fn allocate_zeroed(
        &self,
        layout: Layout,
    ) -> Result<core::ptr::NonNull<[u8]>, core::alloc::AllocError> {
        self.acquire(layout.size())?;

        match self.inner.allocate_zeroed(layout) {
            Ok(allocation) => {
                // Must shrink to request to ensure bookkeeping works
                Ok(NonNull::slice_from_raw_parts(
                    allocation.cast(),
                    layout.size(),
                ))
            }
            Err(e) => {
                // undo the reservation
                self.release_err(layout.size());
                Err(e)
            }
        }
    }

    unsafe fn grow(
        &self,
        ptr: core::ptr::NonNull<u8>,
        old_layout: Layout,
        new_layout: Layout,
    ) -> Result<core::ptr::NonNull<[u8]>, core::alloc::AllocError> {
        let additional = new_layout.size() - old_layout.size();
        self.acquire(additional)?;

        // Safety: the contract on the parameters is guaranteed by our caller
        match unsafe { self.inner.grow(ptr, old_layout, new_layout) } {
            Ok(allocation) => {
                // Must shrink to request to ensure bookkeeping works
                Ok(NonNull::slice_from_raw_parts(
                    allocation.cast(),
                    new_layout.size(),
                ))
            }
            Err(e) => {
                // undo the reservation
                self.release_err(additional);
                Err(e)
            }
        }
    }

    unsafe fn grow_zeroed(
        &self,
        ptr: core::ptr::NonNull<u8>,
        old_layout: Layout,
        new_layout: Layout,
    ) -> Result<core::ptr::NonNull<[u8]>, core::alloc::AllocError> {
        let additional = new_layout.size() - old_layout.size();
        self.acquire(additional)?;

        // Safety: the contract on the parameters is guaranteed by our caller
        match unsafe { self.inner.grow_zeroed(ptr, old_layout, new_layout) } {
            Ok(allocation) => {
                // Must shrink to request to ensure bookkeeping works
                Ok(NonNull::slice_from_raw_parts(
                    allocation.cast(),
                    new_layout.size(),
                ))
            }
            Err(e) => {
                // undo the reservation
                self.release_err(additional);
                Err(e)
            }
        }
    }

    unsafe fn shrink(
        &self,
        ptr: core::ptr::NonNull<u8>,
        old_layout: Layout,
        new_layout: Layout,
    ) -> Result<core::ptr::NonNull<[u8]>, core::alloc::AllocError> {
        // Safety: the contract on the parameters is guaranteed by our caller
        unsafe { self.inner.shrink(ptr, old_layout, new_layout) }.map(|allocation| {
            self.release(old_layout.size() - new_layout.size());

            // Must shrink to ensure bookkeeping works
            NonNull::slice_from_raw_parts(allocation.cast(), new_layout.size())
        })
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;
    use alloc::{alloc::Global, vec::Vec};

    #[test]
    fn bounded_allocator_basic() {
        let alloc = BoundedAllocator::new(Global, 1024);

        let mut data: Vec<u8, _> = Vec::with_capacity_in(128, alloc);
        data.extend(core::iter::repeat(0).take(128));
        // should not crash
    }

    #[test]
    #[should_panic]
    fn bounded_allocator_enforces_limit() {
        std::alloc::set_alloc_error_hook(|layout| {
            panic!("memory allocation of {} bytes failed", layout.size())
        });
        let alloc = BoundedAllocator::new(Global, 128);

        let mut data: Vec<u8, _> = Vec::with_capacity_in(129, alloc);
        data.extend(core::iter::repeat(0).take(129));
    }

    #[test]
    fn bounded_allocator_free_returns_capacity() {
        let alloc = BoundedAllocator::new(Global, 1024);

        for _ in 0..16 {
            let mut data: Vec<u8, _> = Vec::with_capacity_in(128, &alloc);
            data.extend(core::iter::repeat(0).take(128));
        }
        // should not crash
    }
}
