use alloc::sync::Arc;
use core::mem::{self, discriminant};

use futures_util::task::{waker_ref, ArcWake, WakerRef};
use spin::Mutex;

#[non_exhaustive]
pub struct ScheduleInfo {
    pub woken_while_running: bool,
}

pub trait Schedule {
    fn schedule(self, info: ScheduleInfo);
}

impl<F: FnOnce()> Schedule for F {
    fn schedule(self, _: ScheduleInfo) {
        self();
    }
}

pub struct WithInfo<F>(pub F);

impl<F: FnOnce(ScheduleInfo)> Schedule for WithInfo<F> {
    fn schedule(self, info: ScheduleInfo) {
        (self.0)(info)
    }
}

#[derive(Default)]
enum State<S> {
    #[default]
    Init,
    WokenWhileRunning,
    Waiting(S),
    Woken,
}

impl<S: Schedule> State<S> {
    fn wake(&mut self) {
        let next = match mem::take(self) {
            State::Waiting(schedule) => {
                schedule.schedule(ScheduleInfo {
                    woken_while_running: false,
                });
                State::Woken
            }
            State::Woken => State::Woken,
            _ => State::WokenWhileRunning,
        };
        *self = next;
    }
}

pub struct SchedWaker<S> {
    inner: Mutex<State<S>>,
}

impl<S: Schedule + Send> SchedWaker<S> {
    pub fn new() -> Arc<Self> {
        Arc::new(SchedWaker {
            inner: Default::default(),
        })
    }

    pub fn set(&self, schedule: S) {
        let mut inner = self.inner.lock();
        let next = match mem::take(&mut *inner) {
            State::WokenWhileRunning => {
                schedule.schedule(ScheduleInfo {
                    woken_while_running: true,
                });
                State::Woken
            }
            State::Init => State::Waiting(schedule),
            s => unreachable!("invalid state {:?}", discriminant(&s)),
        };
        *inner = next;
    }

    pub fn reset(&self) {
        *self.inner.lock() = State::Init;
    }

    pub fn as_waker<'a>(self: &'a Arc<Self>) -> WakerRef<'a> {
        waker_ref(self)
    }
}

impl<S: Schedule + Send> ArcWake for SchedWaker<S> {
    fn wake_by_ref(arc_self: &Arc<Self>) {
        arc_self.inner.lock().wake()
    }

    fn wake(mut self: Arc<Self>) {
        match Arc::get_mut(&mut self) {
            Some(w) => w.inner.get_mut().wake(),
            None => SchedWaker::wake_by_ref(&self),
        }
    }
}
