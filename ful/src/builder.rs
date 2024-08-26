use unico_context::{boost::Boost, Resume};
use unico_stack::{Global, Stack};

use crate::{
    asym::{Gn, YieldHandle},
    sym::{raw::RawCo, AbortHook, Co, NewError, PanicHook},
};

/// The generic builder for the initialization of some coroutine.
pub struct Builder<R, S, P> {
    rs: R,
    stack: S,
    panic_hook: P,
}

impl Builder<Boost, &'static Global, AbortHook> {
    /// Initiate an empty builder for a coroutine with some defaults.
    pub const fn new() -> Self {
        Builder {
            rs: Boost,
            stack: &Global,
            panic_hook: AbortHook,
        }
    }

    /// Initiate a builder for a coroutine with some defaults.
    pub const fn with<R>(rs: R) -> Builder<R, Global, AbortHook> {
        Builder {
            rs,
            stack: Global,
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

    /// Set the stack that the coroutine will be run on.
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
    #[cfg(feature = "unwind")]
    pub fn hook_panic_with<P2>(self, hook: P2) -> Builder<R, S, P2> {
        Builder {
            rs: self.rs,
            stack: self.stack,
            panic_hook: hook,
        }
    }
}

impl<Z: Resume, S: Into<Stack>, P: PanicHook<Z>> Builder<Z, S, P> {
    /// Create a symmetric stackful coroutine.
    ///
    /// Unlike [`Builder::callcc`], the function will not be executed upon
    /// creation.
    pub fn spawn<F>(self, func: F) -> Result<Co<Z>, NewError<Z>>
    where
        F: FnOnce(Option<Co<Z>>) -> Co<Z> + Send + 'static,
    {
        // SAFETY: The function is `Send` and `'static`.
        unsafe { self.spawn_unchecked(func) }
    }

    /// Like [`Builder::spawn`], but leave some checks on the function to the
    /// caller.
    ///
    /// # Safety
    ///
    /// - `func` must be [`Send`], or the caller must not send the coroutine to
    ///   another thread.
    /// - `func` must be `'static`, or the caller must ensure that the returned
    ///   [`Co`] not escape the lifetime of the function.
    pub unsafe fn spawn_unchecked<F>(self, func: F) -> Result<Co<Z>, NewError<Z>>
    where
        F: FnOnce(Option<Co<Z>>) -> Co<Z>,
    {
        RawCo::new_on(self.stack.into(), &self.rs, self.panic_hook, func)
            .map(|context| Co::from_inner(context, self.rs))
    }

    /// Call the target function with current continuation.
    ///
    /// This function creates a symmetric stackful coroutine and immediately
    /// resume it once.
    pub fn callcc<F>(self, func: F) -> Result<Option<Co<Z>>, NewError<Z>>
    where
        F: FnOnce(Co<Z>) -> Co<Z> + Send + 'static,
    {
        self.spawn(|co| func(co.unwrap())).map(Co::resume)
    }

    /// Like [`Builder::callcc`], but leave some checks on the function to the
    /// caller.
    ///
    /// # Safety
    ///
    /// - `func` must be [`Send`], or the caller must not send the coroutine to
    ///   another thread.
    /// - `func` must be `'static`, or the caller must ensure that the returned
    ///   [`Co`] not escape the lifetime of the function.
    pub unsafe fn callcc_unchecked<F>(self, func: F) -> Result<Option<Co<Z>>, NewError<Z>>
    where
        F: FnOnce(Co<Z>) -> Co<Z>,
    {
        self.spawn_unchecked(|co| func(co.unwrap())).map(Co::resume)
    }

    /// Create a stackful generator, a.k.a. an asymmetric coroutine.
    ///
    /// This structure also implements [`core::ops::Coroutine`] trait.
    pub fn gen<'a, F, Y, C, R>(self, func: F) -> Result<Gn<'a, Y, C, R, Z>, NewError<Z>>
    where
        F: FnOnce(&mut YieldHandle<'a, Y, C, R, Z>, R) -> C + Send + 'a,
    {
        // SAFETY: See `Gn`'s safety notice.
        let wrapper = unsafe { Gn::wrapper(func) };
        // SAFETY: We here constrain the function to be the same lifetime as the
        // generator itself, and the yield handle cannot escape the function as well.
        // Besides, `func` is `Send`. Also see step 0 of `Gn`'s safety notice.
        let co = unsafe { self.callcc_unchecked(wrapper) }?;
        // SAFETY: See step 1 of `Gn`'s safety notice.
        Ok(unsafe { Gn::from_inner(co) })
    }
}

/// Create a symmetric stackful coroutine.
///
/// Unlike [`callcc`], the function will not be executed upon creation.
#[cfg(feature = "global-stack-allocator")]
pub fn spawn<F>(func: F) -> Co
where
    F: FnOnce(Option<Co>) -> Co + Send + 'static,
{
    spawn_on(&Global, func)
}

/// Like [`spawn`], but leave some checks on the function to the caller.
///
/// # Safety
///
/// - `func` must be [`Send`], or the caller must not send the coroutine to
///   another thread.
/// - `func` must be `'static`, or the caller must ensure that the returned
///   [`Co`] not escape the lifetime of the function.
#[cfg(feature = "global-stack-allocator")]
pub unsafe fn spawn_unchecked<F>(func: F) -> Co
where
    F: FnOnce(Option<Co>) -> Co,
{
    spawn_unchecked_on(&Global, func)
}

/// Create a symmetric stackful coroutine on a specific stack.
///
/// Unlike [`callcc`], the function will not be executed upon creation.
pub fn spawn_on<S, F>(stack: S, func: F) -> Co
where
    S: Into<Stack>,
    F: FnOnce(Option<Co>) -> Co + Send + 'static,
{
    Builder::new()
        .on(stack)
        .spawn(func)
        .expect("failed to create a symmetric coroutine")
}

/// Like [`spawn_on`], but leave some checks on the function to the caller.
///
/// # Safety
///
/// - `func` must be [`Send`], or the caller must not send the coroutine to
///   another thread.
/// - `func` must be `'static`, or the caller must ensure that the returned
///   [`Co`] not escape the lifetime of the function.
pub unsafe fn spawn_unchecked_on<S, F>(stack: S, func: F) -> Co
where
    S: Into<Stack>,
    F: FnOnce(Option<Co>) -> Co,
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
#[cfg(feature = "global-stack-allocator")]
pub fn callcc<F>(func: F) -> Option<Co>
where
    F: FnOnce(Co) -> Co + Send + 'static,
{
    spawn(|co| func(co.unwrap())).resume()
}

/// Like [`callcc`], but leave some checks on the function to the caller.
///
/// # Safety
///
/// - `func` must be [`Send`], or the caller must not send the coroutine to
///   another thread.
/// - `func` must be `'static`, or the caller must ensure that the returned
///   [`Co`] not escape the lifetime of the function.
#[cfg(feature = "global-stack-allocator")]
pub unsafe fn callcc_unchecked<F>(func: F) -> Option<Co>
where
    F: FnOnce(Co) -> Co,
{
    spawn_unchecked(|co| func(co.unwrap())).resume()
}

/// Create a stackful generator, a.k.a. an asymmetric coroutine.
///
/// This structure also implements [`core::ops::Coroutine`] trait.
pub fn gen_on<'a, S, F, Y, C, R>(stack: S, func: F) -> Gn<'a, Y, C, R, Boost>
where
    S: Into<Stack>,
    F: FnOnce(&mut YieldHandle<'a, Y, C, R, Boost>, R) -> C + Send + 'a,
{
    Builder::new()
        .on(stack)
        .gen(func)
        .expect("failed to create a generator")
}

/// Create a stackful generator, a.k.a. an asymmetric coroutine, on a specific
/// stack.
///
/// This structure also implements [`core::ops::Coroutine`] trait.
#[cfg(feature = "global-stack-allocator")]
pub fn gen<'a, F, Y, C, R>(func: F) -> Gn<'a, Y, C, R, Boost>
where
    F: FnOnce(&mut YieldHandle<'a, Y, C, R, Boost>, R) -> C + Send + 'a,
{
    gen_on(&Global, func)
}
