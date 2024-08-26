#[cfg(any(feature = "unwind", feature = "std"))]
use alloc::boxed::Box;
#[cfg(any(feature = "unwind", feature = "std"))]
use core::any::Any;
use core::ptr::NonNull;

#[cfg(any(feature = "unwind", feature = "std"))]
use unico_context::Transfer;

#[cfg(any(feature = "unwind", feature = "std"))]
use crate::{sym::Co, unwind};

#[cfg(any(feature = "unwind", feature = "std"))]
#[repr(transparent)]
pub(in crate::sym) struct HandleDrop<C>(pub NonNull<C>);

// SAFETY: If the actual context is not `Send`, then the coroutine will also not
// be `Send`, thus, preventing sending the context to another thread and unwinds
// there.
#[cfg(any(feature = "unwind", feature = "std"))]
unsafe impl<C> Send for HandleDrop<C> {}

#[cfg(any(feature = "unwind", feature = "std"))]
#[allow(improper_ctypes_definitions)]
pub(in crate::sym) extern "C" fn unwind(cx: NonNull<()>, _: *mut ()) -> Transfer<()> {
    unwind::resume_unwind(Box::new(HandleDrop(cx)))
}

#[cfg(any(feature = "unwind", feature = "std"))]
pub(in crate::sym) fn resume_unwind(
    _: &Co,
    payload: Box<dyn Any + Send>,
) -> Box<dyn Any + Send> {
    match payload.downcast::<HandleDrop<()>>() {
        Ok(data) => unwind::resume_unwind(data),
        Err(p) => p,
    }
}

/// The source of some continuation if the current continuation panics.
///
/// The coroutine is built with some implementation of this trait. if the
/// coroutine panics, the panic payload will be handled to it, which then either
/// gives back a continuation to be passed on, or simply aborts the whole
/// control flow, depending on its implementation.
pub trait PanicHook {
    /// The actual process of handling the panic. See [`PanicHook`] for more
    /// information.
    ///
    /// Every function whose signature matches this method automatically
    /// implements this trait. So no need to create some unit structure if the
    /// actual type is not used.
    #[cfg(any(feature = "unwind", feature = "std"))]
    fn rewind(self, payload: Box<dyn Any + Send>) -> Co;
}

/// Aborts the whole control flow if a panic is caught in the current
/// coroutine/continuation.
///
/// See [`PanicHook`] for more information.
pub struct AbortHook;

impl PanicHook for AbortHook {
    #[cfg(any(feature = "unwind", feature = "std"))]
    fn rewind(self, _: Box<dyn Any + Send>) -> Co {
        unreachable!("Uncaught panic in the root of a symmetric coroutine. Aborting.")
    }
}

#[cfg(any(feature = "unwind", feature = "std"))]
impl<T> PanicHook for T
where
    T: FnOnce(Box<dyn Any + Send>) -> Co,
{
    fn rewind(self, payload: Box<dyn Any + Send>) -> Co {
        self(payload)
    }
}
