#![no_std]
#![cfg_attr(feature = "ucx", feature(new_uninit))]
#![feature(pointer_byte_offsets)]
#![feature(slice_ptr_get)]
#![feature(strict_provenance)]

cfg_if::cfg_if! {
    if #[cfg(feature = "boost")] {
        pub mod boost;
    }
}
cfg_if::cfg_if! {
    if #[cfg(feature = "ucx")] {
        extern crate std;
        pub mod ucx;
    }
}

use core::{alloc::Layout, ptr::NonNull};

#[repr(C)]
pub struct Transfer<T> {
    pub context: T,
    pub data: *mut (),
}

pub type Entry<C> = extern "C" fn(t: Transfer<C>) -> !;
pub type Map<C> = extern "C" fn(t: Transfer<C>) -> Transfer<C>;

/// The generic symmetric context-switching trait.
///
/// # Safety
///
/// `Context` must have a proper lifetime bound to a specific stack returned by
/// [`Yield::new_on`].
pub unsafe trait Yield {
    /// The context reference bound to a specfic stack.
    type Context: 'static;

    /// The error type returned during creation of some context.
    type NewError: Sized;

    /// Creates a new context on top of some stack.
    ///
    /// # Safety
    ///
    /// `stack` must reference to a valid block of enough, fresh memory so that
    /// the context will be created on it.
    unsafe fn new_on(
        &self,
        stack: NonNull<[u8]>,
        entry: Entry<Self::Context>,
    ) -> Result<Self::Context, Self::NewError>;

    /// Yields the execution to the target context in transfer structure `t`.
    ///
    /// # Safety
    ///
    /// The `t` must contains a valid context bound to a specific stack.
    unsafe fn resume(&self, t: Transfer<Self::Context>) -> Transfer<Self::Context>;

    /// Yields the execution to the target context in transfer structure `t` and
    /// executes a function on top of that stack.
    ///
    /// # Safety
    ///
    /// The `t` must contains a valid context bound to a specific stack, and the
    /// `map` function must returns a valid transfer structure.
    unsafe fn resume_with(
        &self,
        t: Transfer<Self::Context>,
        map: Map<Self::Context>,
    ) -> Transfer<Self::Context> {
        map(self.resume(t))
    }
}

unsafe fn stack_top<T>(stack: NonNull<[u8]>) -> Option<NonNull<T>> {
    let layout = Layout::new::<T>();
    let ptr = stack.as_non_null_ptr();
    let addr = ptr.addr().get();
    if stack.len() < layout.size() {
        return None;
    }
    let ret = (addr + stack.len() - layout.size()) & !(layout.align() - 1);
    if ret < addr {
        return None;
    }
    ret.try_into().map(|addr| ptr.with_addr(addr).cast()).ok()
}
