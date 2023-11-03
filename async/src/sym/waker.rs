use alloc::{sync::Arc, task::Wake};
use core::{
    marker::PhantomData,
    mem::{self, discriminant, ManuallyDrop},
    ops::Deref,
    ptr,
    task::Waker,
};

use spin::Mutex;

use super::{Scheduler, Switch, Task};

#[derive(Default, Debug)]
enum State<M: Switch> {
    #[default]
    Init,
    WokenWhileRunning,
    Waiting(Task<M>),
    Woken,
}

#[derive(Debug)]
pub struct SchedWaker<S: Scheduler> {
    sched: S,
    state: Mutex<State<S::Metadata>>,
}

impl<S: Scheduler> SchedWaker<S> {
    pub fn new(sched: S) -> Arc<Self> {
        Arc::new(SchedWaker {
            sched,
            state: Default::default(),
        })
    }

    pub fn sched(&self) -> &S {
        &self.sched
    }

    /// Park the current task for [`wake`](Waker::wake) to be called somewhere
    /// else.
    pub fn wait(&self) {
        let scheduled = self.sched.schedule(|task| {
            let mut inner = self.state.lock();
            let next = match mem::take(&mut *inner) {
                State::WokenWhileRunning => {
                    self.sched.enqueue(task);
                    State::Woken
                }
                State::Init => State::Waiting(task),
                s => unreachable!("invalid state {:?}", discriminant(&s)),
            };
            *inner = next;
            None
        });
        if scheduled {
            *self.state.lock() = State::Init;
        }
    }

    /// Wake this task.
    pub fn wake(&self) {
        let mut inner = self.state.lock();
        let next = match mem::take(&mut *inner) {
            State::Waiting(task) => {
                self.sched.enqueue(task);
                State::Woken
            }
            State::Woken => State::Woken,
            _ => State::WokenWhileRunning,
        };
        *inner = next;
    }
}

impl<S: Scheduler + Send + Sync> SchedWaker<S> {
    /// Get a [`Waker`] reference from the waker.
    ///
    /// The standard library implements [`Into<Waker>`] for this structure, so
    /// use this method instead of possible redundant clones.
    pub fn as_waker<'a>(self: &'a Arc<Self>) -> WakerRef<'a>
    where
        <S as Scheduler>::Metadata: Send,
    {
        self.into()
    }
}

impl<S: Scheduler> Wake for SchedWaker<S> {
    fn wake(self: Arc<Self>) {
        (*self).wake()
    }

    fn wake_by_ref(self: &Arc<Self>) {
        (**self).wake()
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
