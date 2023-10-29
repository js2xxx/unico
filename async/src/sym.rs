mod waker;

use core::{
    future::Future,
    pin::pin,
    task::{Context, Poll},
};

use unico_context::Resume;
use unico_ful::sym::Co;

pub use self::waker::{SchedWaker, Schedule, ScheduleInfo};

pub trait Scheduler<R: Resume> {
    fn enqueue(&self, co: Co<R>);

    fn dequeue(&self) -> Co<R>;

    fn block(&self, f: impl FnOnce(Co<R>)) {
        let co = self.dequeue().resume_with(move |co| {
            f(co);
            None
        });
        if let Some(co) = co {
            self.enqueue(co)
        }
    }

    fn yield_now(&self) {
        self.block(|co| self.enqueue(co))
    }
}

pub trait SymWait: Future + Send + Sized {
    fn wait<S, R>(self, sched: S) -> Self::Output
    where
        S: Scheduler<R> + Send + Sync + Clone + 'static,
        R: Resume + Send + Sync + 'static,
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
                Poll::Pending => sched.block(|co| {
                    let sched = sched.clone();
                    waker.set(move || sched.enqueue(co))
                }),
            }
        }
    }
}
impl<F: Future + Send + Sized> SymWait for F {}
