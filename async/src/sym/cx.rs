use alloc::{sync::Arc, task::Wake};
use core::{
    fmt,
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

struct Inner<S: Scheduler> {
    sched: S,
    state: Mutex<State<S::Metadata>>,
}

impl<S: Scheduler> Inner<S> {
    fn wake(&self) {
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

pub struct SchedContext<S: Scheduler>(Arc<Inner<S>>);

impl<S> fmt::Debug for SchedContext<S>
where
    S: Scheduler + fmt::Debug,
    <S as Scheduler>::Metadata: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SchedContext")
            .field("sched", &self.0.sched)
            .field("state", &self.0.state)
            .finish()
    }
}

impl<S: Scheduler> SchedContext<S> {
    pub(crate) fn new(sched: S) -> Self {
        SchedContext(Arc::new(Inner {
            sched,
            state: Default::default(),
        }))
    }

    /// Park the current task for [`wake`](Waker::wake) to be called somewhere
    /// else.
    pub fn wait(&self) {
        let scheduled = self.0.sched.schedule(|task| {
            let mut inner = self.0.state.lock();
            let next = match mem::take(&mut *inner) {
                State::WokenWhileRunning => {
                    self.0.sched.enqueue(task);
                    State::Woken
                }
                State::Init => State::Waiting(task),
                s => unreachable!("invalid state {:?}", discriminant(&s)),
            };
            *inner = next;
            None
        });
        if scheduled {
            *self.0.state.lock() = State::Init;
        }
    }

    /// Wake this task.
    pub fn wake(&self) {
        (*self.0).wake()
    }
}

impl<S: Scheduler> Deref for SchedContext<S> {
    type Target = S;

    fn deref(&self) -> &Self::Target {
        &self.0.sched
    }
}

impl<S: Scheduler + Send + Sync + 'static> SchedContext<S> {
    /// Get a [`Waker`] reference from the waker.
    ///
    /// The standard library implements [`Into<Waker>`] for this structure, so
    /// use this method instead of possible redundant clones.
    pub fn as_waker(&self) -> WakerRef
    where
        <S as Scheduler>::Metadata: Send,
    {
        (&self.0).into()
    }
}

impl<S: Scheduler> Wake for Inner<S> {
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
