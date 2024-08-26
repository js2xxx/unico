//! The integration of [futures](core::future::Future) based on asymmetric
//! stackful coroutines.

use core::{
    future::{Future, IntoFuture},
    marker::PhantomData,
    ops::CoroutineState,
    pin::Pin,
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
pub struct Asym<'a, T>(Gn<'a, T, (), Waker>);

/// The context of the execution of the current [`Asym`].
///
/// This structure is dual to [`Context`] in futures. Users should pass a
/// mutable reference of this struct to [`AsymWait::wait`].
pub struct AsymContext<'y> {
    y: &'y mut YieldHandle<(), Waker>,
    waker: Waker,
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

impl<'a, F, T, S, P> BuildUnchecked<F, S, P> for Asym<'a, T>
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

impl<'a, T> Future for Asym<'a, T> {
    type Output = T;

    fn poll<'x, 'y>(mut self: Pin<&'x mut Self>, cx: &mut Context<'y>) -> Poll<T> {
        match self.0.resume(cx.waker().clone()) {
            CoroutineState::Yielded(()) => Poll::Pending,
            CoroutineState::Complete(output) => Poll::Ready(output),
        }
    }
}

pub trait AsymWait: IntoFuture + Sized {
    /// Wait on a future "synchronously".
    fn wait(self, cx: &mut AsymContext<'_>) -> Self::Output
    where
        <Self as IntoFuture>::IntoFuture: Send,
    {
        let mut future = core::pin::pin!(self.into_future());
        loop {
            let mut ac = Context::from_waker(&cx.waker);
            match future.as_mut().poll(&mut ac) {
                Poll::Ready(output) => break output,
                Poll::Pending => cx.waker = cx.y.yield_(()),
            }
        }
    }
}

impl<F: Future + Send + Sized> AsymWait for F {}

/// Turns a block of sync code into a future.
pub fn sync<'a, T, F>(func: F) -> AsymBuilder<'a, T, F>
where
    F: FnOnce(AsymContext<'_>) -> T + Send + 'a,
{
    AsymBuilder(func, PhantomData)
}

pub struct AsymBuilder<'a, T, F>(F, PhantomData<&'a ()>)
where
    F: FnOnce(AsymContext<'_>) -> T + Send + 'a;

impl<'a, T, F: FnOnce(AsymContext<'_>) -> T + Send> IntoFuture for AsymBuilder<'a, T, F> {
    type Output = T;

    type IntoFuture = Asym<'a, T>;

    fn into_future(self) -> Self::IntoFuture {
        Builder::new()
            .build(self.0)
            .expect("failed to build a stackful future")
    }
}
