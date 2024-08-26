//! Symmetric scheduling and its [Futures](core::future::Future) integration.
//!
//! Unlike the asymmetric one, this module does not contains some function to
//! convert a symmetric coroutine into a future because symmetric coroutines
//! don't have return values. Instead, users can have their own choice of a(n)
//! sync/async channel that sends the result to somewhere.

use core::{
    future::Future,
    hint,
    pin::pin,
    task::{Context, Poll},
};

use unico_ful::{
    sym::{Co, PanicHook},
    Builder, NewError,
};
use unico_stack::Stack;

use crate::waker::SchedWaker;

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
pub unsafe trait Switch: 'static {
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
pub trait Scheduler<M: Switch = ()>: Sized + Clone + 'static {
    /// Push a task to a run queue of the scheduler for execution.
    fn enqueue(&self, task: Task<M>);

    /// Pop a task from a run queue of the scheduler for execution.
    fn dequeue(&self) -> Option<Task<M>>;

    /// Yield the current running task to another.
    ///
    /// `f` decides whether the current running task should be moved to a wait
    /// queue for some event, or be requeued immediately (a.k.a. yielded).
    fn yield_to(&self, next: Task<M>, f: impl FnOnce(Task<M>) -> Option<Task<M>>) {
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
    /// queue for some event, or be requeued immediately (a.k.a. yielded).
    ///
    /// Note that this function will not perform the context switch if there's
    /// no other schedulable tasks, which the boolean return value indicates.
    fn schedule(&self, f: impl FnOnce(Task<M>) -> Option<Task<M>>) -> bool {
        if let Some(next) = self.dequeue() {
            self.yield_to(next, f);
            return true;
        }
        false
    }

    /// Yield the current running task to the scheduler.
    ///
    /// This method is a shorthand for
    /// [`schedule(Some)`](Scheduler::schedule).
    fn yield_now(&self) -> bool {
        self.schedule(Some)
    }

    /// Spawn a new [`Task`] controlled by this scheduler.
    fn spawn<F, S, P>(
        self,
        builder: Builder<S, P>,
        metadata: M,
        f: F,
    ) -> Result<Task<M>, NewError>
    where
        F: FnOnce(&Self) + Send + 'static,
        S: Into<Stack>,
        P: PanicHook,
        Self: Send,
    {
        let co = builder.spawn(move |other| {
            assert!(other.is_none(), "A task escaped from enqueueing!");
            f(&self);
            loop {
                if self.schedule(|_| None) {
                    unreachable!("Zombie task detected!")
                }
                hint::spin_loop()
            }
        })?;
        Ok(Task { co, metadata })
    }
}

pub trait SymWait: Future + Send + Sized {
    /// Wait on a future "synchronously".
    fn wait<S, R, M>(self, sched: &S) -> Self::Output
    where
        S: Scheduler<M> + Send + Sync,
        M: Switch + Send,
    {
        let waker = SchedWaker::new();
        let mut future = pin!(self);
        loop {
            match future
                .as_mut()
                .poll(&mut Context::from_waker(&waker.as_waker()))
            {
                Poll::Ready(output) => break output,
                Poll::Pending => {
                    let scheduled = sched.schedule(|co| {
                        let sched = sched.clone();
                        waker.set(move || sched.enqueue(co));
                        None
                    });
                    if scheduled {
                        waker.reset();
                    }
                }
            }
        }
    }
}
impl<F: Future + Send + Sized> SymWait for F {}

#[cfg(test)]
mod tests {
    use alloc::{alloc::Global, collections::VecDeque, sync::Arc};
    use std::println;

    use spin::Mutex;
    use unico_context::{boost::Boost, global_resumer};
    use unico_stack::global_stack_allocator;

    use super::{Scheduler, Task};

    global_resumer!(Boost);
    global_stack_allocator!(Global);

    struct Fifo(Mutex<VecDeque<Task>>);

    impl Scheduler for Arc<Fifo> {
        fn enqueue(&self, task: Task<()>) {
            self.0.lock().push_back(task)
        }

        fn dequeue(&self) -> Option<Task<()>> {
            self.0.lock().pop_front()
        }
    }

    impl Drop for Fifo {
        fn drop(&mut self) {
            println!("Dropped");
        }
    }

    #[test]
    fn basic() {
        let sched = Arc::new(Fifo(Mutex::new(VecDeque::new())));
        let t1 = sched
            .clone()
            .spawn(Default::default(), (), |s| {
                println!("1");
                s.yield_now();
                println!("2");
            })
            .unwrap();
        let t2 = sched
            .clone()
            .spawn(Default::default(), (), |s| {
                println!("3");
                s.yield_now();
                println!("4");
            })
            .unwrap();
        sched.enqueue(t2);
        println!("Start");
        sched.yield_to(t1, Some);
        println!("5");
        sched.yield_now();
        println!("6\nEnd");
    }
}
