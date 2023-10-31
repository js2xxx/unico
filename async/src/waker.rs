use alloc::{sync::Arc, task::Wake};
use core::{
    marker::PhantomData,
    mem::{self, discriminant, ManuallyDrop},
    ops::Deref,
    ptr,
    task::Waker,
};

use spin::Mutex;

#[non_exhaustive]
pub struct ScheduleInfo {
    pub woken_while_running: bool,
}

/// The one-shot scheduling function.
///
/// Users implementing this trait can retrieve some [schedule
/// information](ScheduleInfo) from the scheduling process, such as the waker is
/// woken during polling.
pub trait Schedule {
    /// See the documentation of the trait for more information.
    fn schedule(self, info: ScheduleInfo);
}

impl<F: FnOnce()> Schedule for F {
    fn schedule(self, _: ScheduleInfo) {
        self();
    }
}

/// The wrapper for construct an instance of [`Schedule`] from one-shot
/// scheduling functions that relies on [`ScheduleInfo`].
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

/// A [`Waker`] that calls a one-shot scheduling function.
///
/// Note that this is different from [`waker-fn`], which requires the function
/// to implement [`Fn`]. To achieve this, every time the target future is
/// polled, a inner state machine is implemented and a new instance of this
/// function should be assigned to this structure.
///
/// `waker-fn`: https://crates.io/crates/waker-fn
pub struct SchedWaker<S> {
    inner: Mutex<State<S>>,
}

impl<S: Schedule + Send + 'static> SchedWaker<S> {
    pub fn new() -> Arc<Self> {
        Arc::new(SchedWaker {
            inner: Default::default(),
        })
    }

    /// Set the one-shot scheduling function of the waker.
    ///
    /// This function should be called right after
    /// [`polling`](core::future::Future::poll) the future, if the result turns
    /// out to be [pending](core::task::Poll::Pending).
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

    /// Clear the state of the waker.
    ///
    /// This function should be called right before
    /// [`polling`](core::future::Future::poll) the future.
    pub fn reset(&self) {
        *self.inner.lock() = State::Init;
    }

    /// Get a [`Waker`] reference from the waker.
    ///
    /// The standard library implements [`Into<Waker>`] for this structure, so
    /// use this method instead of possible redundant clones.
    pub fn as_waker<'a>(self: &'a Arc<Self>) -> WakerRef<'a> {
        self.into()
    }
}

impl<S: Schedule> Wake for SchedWaker<S> {
    fn wake_by_ref(self: &Arc<Self>) {
        self.inner.lock().wake()
    }

    fn wake(mut self: Arc<Self>) {
        match Arc::get_mut(&mut self) {
            Some(w) => w.inner.get_mut().wake(),
            None => SchedWaker::wake_by_ref(&self),
        }
    }
}

/// A [`Waker`] reference.
pub struct WakerRef<'a> {
    inner: ManuallyDrop<Waker>,
    marker: PhantomData<&'a Waker>,
}

impl<'a, W: Wake + Send + Sync + 'static> From<&'a Arc<W>> for WakerRef<'a> {
    fn from(value: &'a Arc<W>) -> Self {
        unsafe {
            let copy = ptr::read(value);
            WakerRef {
                inner: ManuallyDrop::new(Waker::from(copy)),
                marker: PhantomData,
            }
        }
    }
}

impl Deref for WakerRef<'_> {
    type Target = Waker;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}
