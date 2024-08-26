use unico_stack::{Global, Stack};

use crate::{
    asym::{Gn, YieldHandle},
    sym::{AbortHook, Co, PanicHook},
    NewError,
};

/// The generic builder for the initialization of some coroutine.
pub struct Builder<S, P> {
    pub stack: S,
    pub panic_hook: P,
}

/// Build a stackful-coroutine-type object from the builder.
pub trait Build<F, S, P>: BuildUnchecked<F, S, P> {
    /// Build a stackful-coroutine-type object from the builder.
    fn build(builder: Builder<S, P>, arg: F) -> Result<Self, Self::Error>;
}

/// Similar to [`Build`], but leave some check of the arguments to the caller.
#[allow(private_bounds)]
pub trait BuildUnchecked<F, S, P>: Sized {
    type Error;

    /// # Safety
    ///
    /// The caller must ensures the type checks that the implementor insists.
    /// Please see the safety requirements at implementation.
    unsafe fn build_unchecked(
        builder: Builder<S, P>,
        arg: F,
    ) -> Result<Self, Self::Error>;
}

impl Builder<&'static Global, AbortHook> {
    /// Initiate an empty builder for a coroutine with some defaults.
    pub const fn new() -> Self {
        Builder {
            stack: &Global,
            panic_hook: AbortHook,
        }
    }
}

impl<S, P> Builder<S, P> {
    /// Set the stack that the coroutine will be run on.
    pub fn on<S2>(self, stack: S2) -> Builder<S2, P> {
        Builder {
            stack,
            panic_hook: self.panic_hook,
        }
    }

    /// Set the panic hook which give a continuation to pass on when the
    /// coroutine panics. Defaults to [`AbortHook`], which simply aborts the
    /// whole process.
    #[cfg(any(feature = "unwind", feature = "std"))]
    pub fn hook_panic_with<P2>(self, hook: P2) -> Builder<S, P2> {
        Builder {
            stack: self.stack,
            panic_hook: hook,
        }
    }

    /// Build a stackful-coroutine-type object from the builder.
    pub fn build<T, F>(self, arg: F) -> Result<T, T::Error>
    where
        T: Build<F, S, P>,
    {
        T::build(self, arg)
    }

    /// Like [`Builder::build`], but leave some checks on the function to the
    /// caller.
    ///
    /// # Safety
    ///
    /// The caller must ensures the type checks that the implementor insists.
    /// Please see the safety requirements at implementation.
    pub unsafe fn build_unchecked<T, F>(self, arg: F) -> Result<T, T::Error>
    where
        T: BuildUnchecked<F, S, P>,
    {
        T::build_unchecked(self, arg)
    }
}

impl<S: Into<Stack>, P: PanicHook> Builder<S, P> {
    /// Create a symmetric stackful coroutine.
    ///
    /// Unlike [`Builder::callcc`], the function will not be executed upon
    /// creation.
    pub fn spawn<F>(self, func: F) -> Result<Co, NewError>
    where
        F: FnOnce(Option<Co>) -> Co + Send + 'static,
    {
        self.build(func)
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
    pub unsafe fn spawn_unchecked<F>(self, func: F) -> Result<Co, NewError>
    where
        F: FnOnce(Option<Co>) -> Co,
    {
        self.build_unchecked(func)
    }

    /// Call the target function with current continuation.
    ///
    /// This function creates a symmetric stackful coroutine and immediately
    /// resume it once.
    pub fn callcc<F>(self, func: F) -> Result<Option<Co>, NewError>
    where
        F: FnOnce(Co) -> Co + Send + 'static,
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
    pub unsafe fn callcc_unchecked<F>(self, func: F) -> Result<Option<Co>, NewError>
    where
        F: FnOnce(Co) -> Co,
    {
        self.spawn_unchecked(|co| func(co.unwrap())).map(Co::resume)
    }

    /// Create a stackful generator, a.k.a. an asymmetric coroutine.
    ///
    /// This structure also implements [`core::ops::Coroutine`] trait.
    pub fn gen<'a, F, C, Y, R>(self, func: F) -> Result<Gn<'a, C, Y, R>, NewError>
    where
        F: FnOnce(&mut YieldHandle<Y, R>, R) -> C + Send + 'a,
    {
        self.build(func)
    }
}

/// Create a symmetric stackful coroutine.
///
/// Unlike [`callcc`], the function will not be executed upon creation.
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
pub unsafe fn callcc_unchecked<F>(func: F) -> Option<Co>
where
    F: FnOnce(Co) -> Co,
{
    spawn_unchecked(|co| func(co.unwrap())).resume()
}

/// Create a stackful generator, a.k.a. an asymmetric coroutine.
///
/// This structure also implements [`core::ops::Coroutine`] trait.
pub fn gen_on<'a, S, F, C, Y, R>(stack: S, func: F) -> Gn<'a, C, Y, R>
where
    S: Into<Stack>,
    F: FnOnce(&mut YieldHandle<Y, R>, R) -> C + Send + 'a,
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
pub fn gen<'a, F, C, Y, R>(func: F) -> Gn<'a, C, Y, R>
where
    F: FnOnce(&mut YieldHandle<Y, R>, R) -> C + Send + 'a,
{
    gen_on(&Global, func)
}
