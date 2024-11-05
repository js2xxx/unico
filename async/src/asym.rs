//! The integration of [futures](core::future::Future) based on asymmetric
//! stackful coroutines.

use core::{
    future::{Future, IntoFuture},
    marker::PhantomData,
    ops::CoroutineState,
    pin::Pin,
    ptr::NonNull,
    task::{Context, Poll, Waker},
};

use unico_ful::{
    asym::{Gn, YieldHandle},
    sym::PanicHook,
    Build, BuildUnchecked, Builder, NewError,
};
use unico_stack::Stack;

/// A [`Future`] based on a stackful generator.
///
/// This structure cannot be created directly. [`sync`] should be used instead.
pub struct Asym<'a, T>(Gn<'a, T, (), NonNull<Waker>>);

/// The context of the execution of the current [`Asym`].
///
/// This structure is dual to [`Context`] in futures. Users should pass a
/// mutable reference of this struct to [`AsymWait::wait`].
pub struct AsymContext<'y> {
    y: &'y mut YieldHandle<(), NonNull<Waker>>,
    waker: NonNull<Waker>,
}

impl<'a, F, T, S, P> Build<F, S, P> for Asym<'a, T>
where
    F: FnOnce(AsymContext<'_>) -> T + Send + 'a,
    S: Into<Stack>,
    P: PanicHook,
{
    fn build(builder: Builder<S, P>, arg: F) -> Result<Self, Self::Error> {
        // SAFETY: `arg` is `Send` and `'a`.
        unsafe { Self::build_unchecked(builder, arg) }
    }
}

impl<F, T, S, P> BuildUnchecked<F, S, P> for Asym<'_, T>
where
    F: FnOnce(AsymContext<'_>) -> T,
    S: Into<Stack>,
    P: PanicHook,
{
    type Error = NewError;

    /// # Safety
    ///
    /// See [`Gn::build_unchecked`] for more information.
    unsafe fn build_unchecked(
        builder: Builder<S, P>,
        arg: F,
    ) -> Result<Self, Self::Error> {
        // SAFETY: The contract is the same.
        Ok(Asym(unsafe {
            Gn::build_unchecked(builder, |y, waker| arg(AsymContext { y, waker }))?
        }))
    }
}

impl<T> Future for Asym<'_, T> {
    type Output = T;

    fn poll<'x, 'y>(mut self: Pin<&'x mut Self>, cx: &mut Context<'y>) -> Poll<T> {
        match self.0.resume(cx.waker().into()) {
            CoroutineState::Yielded(()) => Poll::Pending,
            CoroutineState::Complete(output) => Poll::Ready(output),
        }
    }
}

pub trait AsymWait: IntoFuture + Sized {
    /// Wait on a future "synchronously" with a specified yielding context.
    fn wait_with(self, cx: &mut AsymContext<'_>) -> Self::Output
    where
        <Self as IntoFuture>::IntoFuture: Send,
    {
        let mut future = core::pin::pin!(self.into_future());
        loop {
            // SAFETY: `cx.waker` remains valid until `cx.y.yield_()`.
            let mut ac = Context::from_waker(unsafe { cx.waker.as_ref() });
            match future.as_mut().poll(&mut ac) {
                Poll::Ready(output) => break output,
                Poll::Pending => cx.waker = cx.y.yield_(()),
            }
        }
    }

    /// Wait on a future "synchronously".
    #[cfg(feature = "std")]
    fn wait(self) -> Self::Output
    where
        <Self as IntoFuture>::IntoFuture: Send,
    {
        match CX.take() {
            Some(cx) => {
                let mut guard = SetCxGuard(None);
                self.wait_with(guard.0.insert(cx))
            }
            None => block_on::block_on(core::pin::pin!(self.into_future())),
        }
    }
}

impl<F: Future + Send + Sized> AsymWait for F {}

/// Turns a block of sync code into a future with its yielding context as an
/// argument.
pub fn sync_with<'a, T, F>(func: F) -> AsymBuilder<'a, T, F>
where
    F: FnOnce(AsymContext<'_>) -> T + Send + 'a,
{
    AsymBuilder {
        func,
        marker: PhantomData,
    }
}

pub struct AsymBuilder<'a, T, F>
where
    F: FnOnce(AsymContext<'_>) -> T + Send + 'a,
{
    func: F,
    marker: PhantomData<&'a ()>,
}

impl<'a, T, F: FnOnce(AsymContext<'_>) -> T + Send> IntoFuture for AsymBuilder<'a, T, F> {
    type Output = T;

    type IntoFuture = Asym<'a, T>;

    fn into_future(self) -> Self::IntoFuture {
        Builder::new()
            .build(self.func)
            .expect("failed to build a stackful future")
    }
}

/// Turns a block of sync code into a future.
#[cfg(feature = "std")]
pub fn sync<'a, T: 'a>(
    func: impl FnOnce() -> T + Send + 'a,
) -> AsymBuilder<'a, T, impl FnOnce(AsymContext<'_>) -> T> {
    sync_with(|cx| {
        // SAFETY: `cx` will be unset when the closure goes out of scope.
        let cx =
            unsafe { core::mem::transmute::<AsymContext<'_>, AsymContext<'static>>(cx) };
        let _old_guard = SetCxGuard(CX.replace(Some(cx)));

        func()
    })
}

#[cfg(feature = "std")]
std::thread_local! {
    static CX: core::cell::Cell<Option<AsymContext<'static>>>
        = const { core::cell::Cell::new(None) };
}

#[cfg(feature = "std")]
struct SetCxGuard(Option<AsymContext<'static>>);

#[cfg(feature = "std")]
impl Drop for SetCxGuard {
    fn drop(&mut self) {
        CX.set(self.0.take());
    }
}

/// Borrowed from `futures_lite::future::block_on` with minor changes.
#[cfg(feature = "std")]
mod block_on {
    use core::{
        cell::RefCell,
        future::Future,
        pin::Pin,
        task::{Context, Poll, Waker},
    };
    use std::{sync::Arc, task::Wake, thread, thread_local};

    pub fn block_on<T>(mut future: Pin<&mut impl Future<Output = T>>) -> T {
        struct WakerFn<F>(F);

        impl<F: Fn() + Send + Sync + 'static> Wake for WakerFn<F> {
            fn wake(self: Arc<Self>) {
                (self.0)();
            }

            fn wake_by_ref(self: &Arc<Self>) {
                (self.0)();
            }
        }

        thread_local! {
            // Cached waker for efficiency.
            static CACHE: RefCell<Waker> = RefCell::new({
                let thread = thread::current();
                Arc::new(WakerFn(move || thread.unpark())).into()
            });
        }

        // Grab the cached waker.
        //
        // This function won't be called nestedly, so we don't need to
        // `try_borrow_mut`.
        CACHE.with_borrow_mut(|waker| {
            let cx = &mut Context::from_waker(waker);
            // Keep polling until the future is ready.
            loop {
                match future.as_mut().poll(cx) {
                    Poll::Ready(output) => return output,
                    Poll::Pending => thread::park(),
                }
            }
        })
    }
}
