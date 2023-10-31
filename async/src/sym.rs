//! Symmetric scheduling and its [Futures](core::future::Future) integration.
//!
//! Unlike the asymmetric one, this module does not contains some function to
//! convert a symmetric coroutine into a future because symmetric coroutines
//! don't have return values. Instead, users can have their own choice of a(n)
//! sync/async channel that sends the result to somewhere.

mod waker;

use core::{
    future::Future,
    pin::pin,
    task::{Context, Poll},
};

use unico_ful::{
    sym::{Co, PanicHook},
    Builder, NewError,
};
use unico_stack::Stack;

pub use self::waker::{SchedWaker, Schedule, ScheduleInfo};

/// A task that can be spawned in a scheduler.
pub struct Task<M: Switch = ()> {
    co: Co,
    metadata: M,
}

impl<M: Switch> Task<M> {
    /// Retrive an immutable reference of the metadata from the task.
    pub fn metadata(&self) -> &M {
        &self.metadata
    }

    /// Retrive the mutable reference of the metadata from the task.
    pub fn metadata_mut(&mut self) -> &mut M {
        &mut self.metadata
    }
}

/// Represents some data structure that can be switched onto and off from a
/// "current" state.
///
/// ## "Current" state
///
/// This words refer to some data structure that is unique on each execution
/// unit, such as a thread in user space or a CPU core in kernel. for example, a
/// mutable TLS in threads, or the registration of page tables in CPU cores, are
/// all "current" states in their own context.
///
/// You cannot use `()` for any actual state storage because it literally
/// represents non-data and thus non-state. Custom unit struct is a better
/// option instead.
///
/// # Safety
///
/// The switch method must perform a valid switch on some "current" state, which
/// any scheduler with task metadata implementing this trait relies on.
pub unsafe trait Switch {
    /// Switch on `self` as the new "current" state, and return the old state.
    ///
    /// See [the documentation on the trait](Switch) for more information about
    /// "current" states and implementation notice.
    fn switch(self) -> Self;
}

// SAFETY: All current states of `()` is non-state.
unsafe impl Switch for () {
    fn switch(self) {}
}

/// A scheduler operating on [`Task`]s.
///
/// This trait only concerns about "run queues", not "wait queues". The user
/// should use [`Waker`](core::task::Waker)s for the item of their own wait
/// queues.
pub trait Scheduler: Sized + Clone + 'static {
    /// The metadata of the scheduled [`Task`]s.
    type Metadata: Switch;

    /// Push a task to a run queue of the scheduler for execution.
    fn enqueue(&self, task: Task<Self::Metadata>);

    /// Pop a task from a run queue of the scheduler for execution.
    fn dequeue(&self) -> Option<Task<Self::Metadata>>;

    /// Yield the current running task to another.
    ///
    /// `f` decides whether the current running task should be moved to a wait
    /// queue for some event, or be re-enqueue immediately (a.k.a. yielded).
    fn yield_to(
        &self,
        next: Task<Self::Metadata>,
        f: impl FnOnce(Task<Self::Metadata>) -> Option<Task<Self::Metadata>>,
    ) {
        let other = next.co.resume_with(|co| {
            let metadata = next.metadata.switch();
            if let Some(next) = f(Task { co, metadata }) {
                self.enqueue(next);
            }
            None
        });
        assert!(other.is_none(), "A task escaped from enqueueing!");
    }

    /// Schedule the current running task for another chance for execution.
    ///
    /// `f` decides whether the current running task should be moved to a wait
    /// queue for some event, or be re-enqueue immediately (a.k.a. yielded).
    fn schedule(
        &self,
        f: impl FnOnce(Task<Self::Metadata>) -> Option<Task<Self::Metadata>>,
    ) {
        if let Some(next) = self.dequeue() {
            self.yield_to(next, f)
        }
    }

    /// Yield the current running task to the scheduler.
    ///
    /// This method is a shorthand for
    /// [`schedule(Some)`](Scheduler::schedule).
    fn yield_now(&self) {
        self.schedule(Some)
    }

    /// Spawn a new [`Task`] controlled by this scheduler.
    fn spawn<F, S, P>(
        self,
        builder: Builder<S, P>,
        metadata: Self::Metadata,
        f: F,
    ) -> Result<Task<Self::Metadata>, NewError>
    where
        F: FnOnce(&Self) + Send + 'static,
        S: Into<Stack>,
        P: PanicHook,
        Self: Send,
    {
        let co = builder.spawn(move |other: Option<Co>| {
            assert!(other.is_none(), "A task escaped from enqueueing!");
            f(&self);
            loop {
                if let Some(task) = self.dequeue() {
                    task.co.resume_with(|co| {
                        let metadata = task.metadata.switch();
                        drop(Task { co, metadata });
                        None
                    });
                    unreachable!("Zombie task detected!")
                }
            }
        })?;
        Ok(Task { co, metadata })
    }
}

pub trait SymWait: Future + Send + Sized {
    /// Wait on a future "synchronously".
    fn wait<S, R>(self, sched: S) -> Self::Output
    where
        S: Scheduler + Send + Sync,
        <S as Scheduler>::Metadata: Send,
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
