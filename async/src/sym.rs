mod waker;

use core::{
    future::Future,
    hint,
    pin::pin,
    task::{Context, Poll},
};

use unico_ful::sym::Co;

pub use self::waker::{SchedWaker, Schedule, ScheduleInfo};

pub trait Scheduler: Sized + Clone + 'static {
    fn enqueue(&self, co: Co);

    fn dequeue(&self) -> Option<Co>;

    fn schedule(&self, f: impl FnOnce(Co) -> Option<Co>);
}

pub trait SchedulerExt: Scheduler {
    fn yield_now(&self) {
        self.schedule(Some)
    }

    fn wrap<F: FnOnce(Self)>(self, f: F) -> impl FnOnce(Option<Co>) -> Co {
        move |co| {
            if let Some(co) = co {
                self.enqueue(co)
            }
            f(self.clone());
            loop {
                if let Some(co) = self.dequeue() {
                    break co;
                }
                hint::spin_loop()
            }
        }
    }
}

impl<S: Scheduler> SchedulerExt for S {}

pub trait SymWait: Future + Send + Sized {
    fn wait<S, R>(self, sched: S) -> Self::Output
    where
        S: Scheduler + Send + Sync,
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
                Poll::Pending => sched.schedule(|co| {
                    let sched = sched.clone();
                    waker.set(move || sched.enqueue(co));
                    None
                }),
            }
        }
    }
}
impl<F: Future + Send + Sized> SymWait for F {}
