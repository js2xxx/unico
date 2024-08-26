use alloc::{sync::Arc, task::Wake};
use core::{
    fmt,
    marker::PhantomData,
    mem::{self, ManuallyDrop},
    ops::Deref,
    ptr,
    task::Waker,
};

use spin::Mutex;

use super::{Scheduler, SchedulerExt, Switch, Task};

#[derive(Default, Debug)]
enum State<M: Switch> {
    #[default]
    Empty,
    Notified,
    Waiting(Task<M>),
}

struct Inner<S: Scheduler> {
    sched: S,
    state: Mutex<State<S::Metadata>>,
}

unsafe impl<S: Scheduler + Send + Sync> Sync for Inner<S> {}

impl<S: Scheduler> Inner<S> {}

pub struct SchedContext<S: Scheduler> {
    inner: Arc<Inner<S>>,
    marker: PhantomData<*mut ()>,
}

impl<S> fmt::Debug for SchedContext<S>
where
    S: Scheduler + fmt::Debug,
    <S as Scheduler>::Metadata: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SchedContext")
            .field("sched", &self.inner.sched)
            .field("state", &self.inner.state)
            .finish()
    }
}

impl<S: Scheduler> SchedContext<S> {
    /// We don't make this function public to prevent users from calling `wait`
    /// in other tasks.
    pub(crate) fn new(sched: S) -> Self {
        SchedContext {
            inner: Arc::new(Inner {
                sched,
                state: Default::default(),
            }),
            marker: PhantomData,
        }
    }

    /// Park the current task for [`wake`](Waker::wake) to be called somewhere
    /// else.
    pub fn wait(&self) {
        loop {
            let mut state = self.inner.state.lock();
            match mem::take(&mut *state) {
                State::Notified => break,
                State::Empty => {
                    let f = move |task| *state = State::Waiting(task);
                    self.inner.sched.schedule(f)
                }
                State::Waiting(_) => unreachable!("cannot call `wait` on other tasks"),
            }
        }
    }
}

impl<S: Scheduler> Deref for SchedContext<S> {
    type Target = S;

    fn deref(&self) -> &Self::Target {
        &self.inner.sched
    }
}

impl<S: Scheduler + Send + Sync + 'static> SchedContext<S> {
    /// Get a [`Waker`] reference from the waker.
    ///
    /// The standard library implements [`Into<Waker>`] for this structure, so
    /// use this method instead of possible redundant clones.
    pub fn as_waker(&self) -> WakerRef<'_>
    where
        <S as Scheduler>::Metadata: Send,
    {
        (&self.inner).into()
    }
}

impl<S: Scheduler> Wake for Inner<S> {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref()
    }

    fn wake_by_ref(self: &Arc<Self>) {
        let state = mem::replace(&mut *self.state.lock(), State::Notified);
        if let State::Waiting(task) = state {
            self.sched.enqueue(task);
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
