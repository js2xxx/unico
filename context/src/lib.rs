#![no_std]
#![feature(slice_ptr_get)]

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

#[cfg(feature = "boost")]
mod boost {
    use core::{convert::Infallible, ffi::c_void, ptr::NonNull};

    use crate::{Transfer, Yield};

    #[repr(transparent)]
    pub struct Fcx(NonNull<c_void>);

    #[link(name = "boost_context")]
    extern "C" {
        /// Creates a new `Context` on top of some stack.
        #[link_name = "make_fcontext"]
        fn new_on(
            stack: NonNull<u8>,
            size: usize,
            entry: extern "C" fn(t: FcxTransfer) -> !,
        ) -> Fcx;

        /// Yields the execution to another `Context`.
        #[link_name = "jump_fcontext"]
        fn resume(target: Fcx, data: *mut ()) -> FcxTransfer;

        /// Yields the execution to another `Context` and executes a function on
        /// top of that stack.
        #[link_name = "ontop_fcontext"]
        fn resume_with(
            target: Fcx,
            data: *mut (),
            map: extern "C" fn(t: FcxTransfer) -> FcxTransfer,
        ) -> FcxTransfer;
    }

    pub type FcxTransfer = Transfer<Fcx>;

    #[derive(Debug, Copy, Clone, Default)]
    pub struct Boost;

    unsafe impl Yield for Boost {
        type Context = Fcx;

        type NewError = Infallible;

        unsafe fn new_on(
            &self,
            stack: NonNull<[u8]>,
            entry: extern "C" fn(t: Transfer<Self::Context>) -> !,
        ) -> Result<Self::Context, Infallible> {
            Ok(self::new_on(stack.as_non_null_ptr(), stack.len(), entry))
        }

        unsafe fn resume(&self, t: Transfer<Self::Context>) -> Transfer<Self::Context> {
            self::resume(t.context, t.data)
        }

        unsafe fn resume_with(
            &self,
            t: Transfer<Self::Context>,
            map: extern "C" fn(t: Transfer<Self::Context>) -> Transfer<Self::Context>,
        ) -> Transfer<Self::Context> {
            self::resume_with(t.context, t.data, map)
        }
    }
}
#[cfg(feature = "boost")]
pub use crate::boost::*;
