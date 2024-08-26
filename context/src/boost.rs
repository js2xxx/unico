use core::{ffi::c_void, ptr::NonNull};

use crate::{stack_top, Entry, Map, Resume};

/// [`Boost.Context`](https://github.com/boostorg/context/) with its `fcontext_t` functions.
#[derive(Debug)]
#[repr(transparent)]
pub struct Fcx(NonNull<c_void>);

#[link(name = "boost_context")]
extern "C" {
    /// Creates a new `Context` on top of some stack.
    #[link_name = "make_fcontext"]
    fn new_on(stack_top: NonNull<()>, size: usize, entry: Entry<Fcx>) -> Fcx;

    /// Yields the execution to another `Context`.
    #[link_name = "jump_fcontext"]
    fn resume(target: Fcx, data: *mut ()) -> Transfer;

    /// Yields the execution to another `Context` and executes a function on
    /// top of that stack.
    #[link_name = "ontop_fcontext"]
    fn resume_with(target: Fcx, data: *mut (), map: Map<Fcx>) -> Transfer;
}

pub type Transfer = crate::Transfer<Fcx>;

#[derive(Debug, Copy, Clone, Default)]
pub struct Boost;

#[derive(Debug)]
pub enum NewError {
    StackTooSmall,
}

unsafe impl Resume for Boost {
    type Context = Fcx;

    type NewError = NewError;

    unsafe fn new_on(
        &self,
        stack: NonNull<[u8]>,
        entry: Entry<Fcx>,
    ) -> Result<Fcx, NewError> {
        let top: NonNull<()> =
            unsafe { stack_top(stack) }.ok_or(NewError::StackTooSmall)?;
        Ok(self::new_on(top, stack.len(), entry))
    }

    unsafe fn resume(&self, cx: Fcx, data: *mut ()) -> Transfer {
        unsafe { self::resume(cx, data) }
    }

    unsafe fn resume_with(&self, cx: Fcx, data: *mut (), map: Map<Fcx>) -> Transfer {
        unsafe { self::resume_with(cx, data, map) }
    }
}
