use core::{
    fmt::Debug,
    future::{Future, IntoFuture},
    marker::PhantomData,
    ops::CoroutineState,
    pin::Pin,
    task::{Context, Poll, Waker},
};

use pin_project::pin_project;
use unico_context::{boost::Boost, Resume};
use unico_ful::{
    asym::{Gn, YieldHandle},
    sym::PanicHook,
    Build, BuildUnchecked, Builder, NewError,
};
use unico_stack::Stack;

#[pin_project]
pub struct Asym<'a, T, R: Resume = Boost>(Gn<'a, T, (), Waker, R>);

pub struct AsymContext<'y, R: Resume = Boost> {
    y: &'y mut YieldHandle<(), Waker, R>,
    waker: Waker,
}

impl<'a, F, T, R: Resume, S, P> Build<F, R, S, P> for Asym<'a, T, R>
where
    F: FnOnce(AsymContext<R>) -> T + Send + 'a,
    S: Into<Stack>,
    P: PanicHook<R>,
{
    fn build(builder: Builder<R, S, P>, arg: F) -> Result<Self, Self::Error> {
        // SAFETY: `arg` is `Send` and `'a`.
        unsafe { Self::build_unchecked(builder, arg) }
    }
}

impl<'a, F, T, R: Resume, S, P> BuildUnchecked<F, R, S, P> for Asym<'a, T, R>
where
    F: FnOnce(AsymContext<R>) -> T,
    S: Into<Stack>,
    P: PanicHook<R>,
{
    type Error = NewError<R>;

    /// # Safety
    ///
    /// See [`Gn::build_unchecked`] for more information.
    unsafe fn build_unchecked(
        builder: Builder<R, S, P>,
        arg: F,
    ) -> Result<Self, Self::Error> {
        Ok(Asym(Gn::build_unchecked(builder, |y, waker| -> T {
            arg(AsymContext { y, waker })
        })?))
    }
}

impl<'a, T, R: Resume> Future for Asym<'a, T, R> {
    type Output = T;

    fn poll<'x, 'y>(mut self: Pin<&'x mut Self>, cx: &mut Context<'y>) -> Poll<T> {
        match self.0.resume(cx.waker().clone()) {
            CoroutineState::Yielded(()) => Poll::Pending,
            CoroutineState::Complete(output) => Poll::Ready(output),
        }
    }
}

pub trait Wait: Future + Send + Sized {
    fn wait<R: Resume>(self, cx: &mut AsymContext<R>) -> Self::Output {
        let mut future = core::pin::pin!(self);
        loop {
            let mut ac = Context::from_waker(&cx.waker);
            match future.as_mut().poll(&mut ac) {
                Poll::Ready(output) => break output,
                Poll::Pending => cx.waker = cx.y.yield_(()),
            }
        }
    }
}

impl<F: Future + Send + Sized> Wait for F {}

pub fn sync<'a, T, F>(func: F) -> AsymBuilder<'a, T, Boost, F>
where
    F: FnOnce(AsymContext) -> T + Send + 'a,
{
    AsymBuilder(Boost, func, PhantomData)
}

pub fn sync_by<'a, T, R, F>(rs: R, func: F) -> AsymBuilder<'a, T, R, F>
where
    R: Resume,
    F: FnOnce(AsymContext<R>) -> T + Send + 'a,
{
    AsymBuilder(rs, func, PhantomData)
}

pub struct AsymBuilder<'a, T, R, F>(R, F, PhantomData<&'a ()>)
where
    R: Resume,
    F: FnOnce(AsymContext<R>) -> T + Send + 'a;

impl<'a, T, R: Resume + Debug, F: FnOnce(AsymContext<R>) -> T + Send> IntoFuture
    for AsymBuilder<'a, T, R, F>
{
    type Output = T;

    type IntoFuture = Asym<'a, T, R>;

    fn into_future(self) -> Self::IntoFuture {
        Builder::new()
            .impl_by(self.0)
            .build(self.1)
            .expect("failed to build a stackful future")
    }
}