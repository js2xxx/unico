#![no_std]
#![feature(slice_ptr_get)]
#![feature(strict_provenance)]

cfg_if::cfg_if! {
    if #[cfg(feature = "boost")] {
        pub mod boost;
    }
}

use core::ptr::NonNull;

#[repr(C)]
pub struct Transfer<T> {
    pub context: T,
    pub data: *mut (),
}

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
        entry: extern "C" fn(t: Transfer<Self::Context>) -> !,
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
        map: extern "C" fn(t: Transfer<Self::Context>) -> Transfer<Self::Context>,
    ) -> Transfer<Self::Context> {
        map(self.resume(t))
    }
}

unsafe fn stack_top<T>(stack: NonNull<[u8]>, sub: usize) -> NonNull<T> {
    let ptr = stack.as_non_null_ptr();
    ptr.map_addr(|addr| (addr.get() + stack.len() - sub).try_into().unwrap())
        .cast()
}
