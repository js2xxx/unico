mod waker;

use core::{
    pin::pin,
    task::{Context, Poll},
};

use futures_util::Future;
use unico_context::Resume;
use unico_ful::sym::Co;

pub use self::waker::{SchedWaker, Schedule, ScheduleInfo};

pub trait Scheduler<R: Resume> {
    fn enqueue(&self, co: Co<R>);

    fn dequeue(&self) -> Co<R>;

    fn switch_metadata(&self, co: &Co<R>);

    fn switch(&self, f: impl FnOnce(Co<R>)) {
        let co = self.dequeue().resume_with(move |co| {
            self.switch_metadata(&co);
            f(co);
            None
        });
        if let Some(co) = co {
            self.enqueue(co)
        }
    }

    fn yield_now(&self) {
        self.switch(|co| self.enqueue(co))
    }
}

impl<S: Scheduler<R>, R: Resume> Scheduler<R> for &S {
    fn enqueue(&self, co: Co<R>) {
        (**self).enqueue(co)
    }

    fn dequeue(&self) -> Co<R> {
        (**self).dequeue()
    }

    fn switch_metadata(&self, co: &Co<R>) {
        (**self).switch_metadata(co)
    }
}

pub trait SymWait: Future + Send + Sized {
    fn wait<S, R>(self, sched: S) -> Self::Output
    where
        S: Scheduler<R> + Sync,
        R: Resume + Send + Sync,
    {
        let waker = SchedWaker::new();
        let mut future = pin!(self);
        loop {
            waker.reset();

            match future
                .as_mut()
                .poll(&mut Context::from_waker(&waker.as_waker()))
            {
                Poll::Ready(output) => break output,
                Poll::Pending => sched.switch(|co| waker.set(|| sched.enqueue(co))),
            }
        }
    }
}
impl<F: Future + Send + Sized> SymWait for F {}
