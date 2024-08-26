use core::{mem, ptr::NonNull};

use crate::{stack_top, Entry, Map, Resume};

const CONTEXT_SIZE: usize = include!(concat!(env!("OUT_DIR"), "/context_size.txt"));
const CONTEXT_LEN: usize = CONTEXT_SIZE / mem::size_of::<usize>();

/// The `fcontext_t` wrapper type in [`Boost.Context`](https://github.com/boostorg/context/).
#[derive(Debug)]
#[repr(transparent)]
pub struct Fcx(NonNull<[usize; CONTEXT_LEN]>);

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

/// The [`Resume`] implementation with the [`Boost.Context`](https://github.com/boostorg/context/) functionality.
#[derive(Debug, Copy, Clone, Default)]
pub struct Boost;

#[derive(Debug)]
pub enum NewError {
    StackTooSmall,
}

// SAFETY: `Fcx` is created from `stack`. See Boost's assembly file for more
// information.
unsafe impl Resume for Boost {
    type Context = Fcx;

    type NewError = NewError;

    unsafe fn new_on(
        &self,
        stack: NonNull<[u8]>,
        entry: Entry<Fcx>,
    ) -> Result<Fcx, NewError> {
        let top: NonNull<()> = stack_top(stack).ok_or(NewError::StackTooSmall)?;
        // SAFETY: The stack is valid by contract.
        Ok(unsafe { self::new_on(top, stack.len(), entry) })
    }

    unsafe fn resume(&self, cx: Fcx, data: *mut ()) -> Transfer {
        // SAFETY: `cx` is valid by contract.
        unsafe { self::resume(cx, data) }
    }

    unsafe fn resume_with(&self, cx: Fcx, data: *mut (), map: Map<Fcx>) -> Transfer {
        // SAFETY: `cx` and `map` is valid by contract.
        unsafe { self::resume_with(cx, data, map) }
    }
}
