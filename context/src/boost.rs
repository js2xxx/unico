use core::{convert::Infallible, ffi::c_void, ptr::NonNull};

use crate::{stack_top, Transfer, Yield};

#[repr(transparent)]
pub struct Fcx(NonNull<c_void>);

#[link(name = "boost_context")]
extern "C" {
    /// Creates a new `Context` on top of some stack.
    #[link_name = "make_fcontext"]
    fn new_on(
        stack_top: NonNull<u8>,
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
        let top = unsafe { stack_top(stack, 0) };
        Ok(self::new_on(top, stack.len(), entry))
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
