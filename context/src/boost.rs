use core::{convert::Infallible, ffi::c_void, ptr::NonNull};

use crate::{stack_top, Context, Entry, Map};

/// [`Boost.Context`](https://github.com/boostorg/context/) with its `fcontext_t` functions.
#[derive(Debug)]
#[repr(transparent)]
pub struct Fcx(NonNull<c_void>);

#[link(name = "boost_context")]
extern "C" {
    /// Creates a new `Context` on top of some stack.
    #[link_name = "make_fcontext"]
    fn new_on(
        stack_top: NonNull<()>,
        size: usize,
        entry: extern "C" fn(t: Transfer) -> !,
    ) -> Fcx;

    /// Yields the execution to another `Context`.
    #[link_name = "jump_fcontext"]
    fn resume(target: Fcx, data: *mut ()) -> Transfer;

    /// Yields the execution to another `Context` and executes a function on
    /// top of that stack.
    #[link_name = "ontop_fcontext"]
    fn resume_with(
        target: Fcx,
        data: *mut (),
        map: extern "C" fn(t: Transfer) -> Transfer,
    ) -> Transfer;
}

pub type Transfer = crate::Transfer<Fcx>;

#[derive(Debug, Copy, Clone, Default)]
pub struct Boost;

unsafe impl Context for Boost {
    type Context = Fcx;

    type NewError = Infallible;

    unsafe fn new_on(
        &self,
        stack: NonNull<[u8]>,
        entry: Entry<Fcx>,
    ) -> Result<Fcx, Infallible> {
        let top: NonNull<()> = unsafe { stack_top(stack).unwrap() };
        Ok(self::new_on(top, stack.len(), entry))
    }

    unsafe fn resume(&self, t: Transfer) -> Transfer {
        self::resume(t.context, t.data)
    }

    unsafe fn resume_with(&self, t: Transfer, map: Map<Fcx>) -> Transfer {
        self::resume_with(t.context, t.data, map)
    }
}
