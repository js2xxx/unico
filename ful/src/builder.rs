use unico_context::{boost::Boost, Resume};

use crate::{raw::RawCo, stack::RawStack, AbortHook, Co, NewError, PanicHook};

/// The generic builder for the initialization of some coroutine.
pub struct Builder<R, S, P> {
    rs: R,
    stack: S,
    panic_hook: P,
}

impl Builder<Boost, (), AbortHook> {
    /// Initiate an empty builder for a coroutine with some defaults.
    pub const fn new() -> Self {
        Builder {
            rs: Boost,
            stack: (),
            panic_hook: AbortHook,
        }
    }

    /// Initiate a builder for a coroutine with some defaults.
    pub const fn with<R>(rs: R) -> Builder<R, (), AbortHook> {
        Builder {
            rs,
            stack: (),
            panic_hook: AbortHook,
        }
    }
}

impl<R, S, P> Builder<R, S, P> {
    /// Set the resuming mechanism, a.k.a. a implementation of [`Resume`] for
    /// the coroutine. Defaults to [`Boost`]'s `fcontext_t` implementation.
    pub fn impl_by<R2>(self, rs: R2) -> Builder<R2, S, P> {
        Builder {
            rs,
            stack: self.stack,
            panic_hook: self.panic_hook,
        }
    }

    /// Set the stack that the coroutine will be run on. This is a compulsory
    /// option.
    pub fn on<S2>(self, stack: S2) -> Builder<R, S2, P> {
        Builder {
            rs: self.rs,
            stack,
            panic_hook: self.panic_hook,
        }
    }

    /// Set the panic hook which give a continuation to pass on when the
    /// coroutine panics. Defaults to [`AbortHook`], which simply aborts the
    /// whole process.
    #[cfg(feature = "alloc")]
    pub fn hook_panic_with<P2>(self, hook: P2) -> Builder<R, S, P2> {
        Builder {
            rs: self.rs,
            stack: self.stack,
            panic_hook: hook,
        }
    }
}

impl<R: Resume, P: PanicHook<R>> Builder<R, RawStack, P> {
    /// Create a symmetric stackful coroutine.
    ///
    /// Unlike [`callcc`], the function will not be executed upon creation.
    pub fn spawn(
        self,
        func: impl FnOnce(Option<Co<R>>) -> Co<R> + Send + 'static,
    ) -> Result<Co<R>, NewError<R>> {
        // SAFETY: The function is `Send` and `'static`.
        unsafe { self.spawn_unchecked(func) }
    }

    /// Like [`Builder::spawn`], but leave some checks on the function to the
    /// caller.
    ///
    /// # Safety
    ///
    /// - The function must be [`Send`], or the caller must not send the
    ///   coroutine to another thread.
    /// - The function must be `'static`, or it must outlive the returned
    ///   [`Co`].
    pub unsafe fn spawn_unchecked(
        self,
        func: impl FnOnce(Option<Co<R>>) -> Co<R>,
    ) -> Result<Co<R>, NewError<R>> {
        RawCo::new_on(self.stack, &self.rs, self.panic_hook, func)
            .map(|context| Co::from_inner(context, self.rs))
    }
}

/// Create a symmetric stackful coroutine.
///
/// Unlike [`callcc`], the function will not be executed upon creation.
pub fn spawn<F>(stack: RawStack, func: F) -> Co<Boost>
where
    F: FnOnce(Option<Co<Boost>>) -> Co<Boost> + Send + 'static,
{
    Builder::new()
        .on(stack)
        .spawn(func)
        .expect("failed to create a symmetric coroutine")
}

/// Like [`spawn`], but leave some checks on the function to the caller.
///
/// # Safety
///
/// - The function must be [`Send`], or the caller must not send the coroutine
///   to another thread.
/// - The function must be `'static`, or it must outlive the returned [`Co`].
pub unsafe fn spawn_unchecked<F>(stack: RawStack, func: F) -> Co<Boost>
where
    F: FnOnce(Option<Co<Boost>>) -> Co<Boost>,
{
    Builder::new()
        .on(stack)
        .spawn_unchecked(func)
        .expect("failed to create a symmetric coroutine")
}

/// Call the target function with current continuation.
///
/// This function creates a symmetric stackful coroutine and immediately resume
/// it once.
pub fn callcc<F>(stack: RawStack, func: F) -> Option<Co<Boost>>
where
    F: FnOnce(Option<Co<Boost>>) -> Co<Boost> + Send + 'static,
{
    spawn(stack, func).resume()
}
