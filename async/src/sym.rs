//! Symmetric scheduling and its [Futures](core::future::Future) integration.
//!
//! Unlike the asymmetric one, this module does not contains some function to
//! convert a symmetric coroutine into a future because symmetric coroutines
//! don't have return values. Instead, users can have their own choice of a(n)
//! sync/async channel that sends the result to somewhere.

mod cx;

use core::{
    future::{Future, IntoFuture},
    hint,
    ops::Deref,
    pin::pin,
    task::{Context, Poll},
};

use bevy_utils_proc_macros::all_tuples;
use unico_ful::{
    Builder, NewError,
    sym::{Co, PanicHook},
};
use unico_stack::Stack;

pub use self::cx::{SchedContext, WakerRef};

/// A task that can be spawned in a scheduler.
#[derive(Debug)]
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

    /// Yield the current running task to this task.
    ///
    /// `f` decides the destination of the current running task, such as a wait
    /// queue or a scheduler's run queue.
    pub fn resume(self, f: impl FnOnce(Self)) {
        let other = self.co.resume_with(|co| {
            let metadata = self.metadata.switch();
            f(Task { co, metadata });
            None
        });
        assert!(other.is_none(), "A task escaped from resuming!");
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

macro_rules! impl_tuples {
    ($($param:ident),*) => {
        unsafe impl<$($param: Switch),*> Switch for ($($param,)*) {
            #[allow(clippy::unused_unit)]
            #[allow(non_snake_case)]
            fn switch(self) -> Self {
                let ($($param,)*) = self;
                ($($param.switch(),)*)
            }
        }
    };
}
all_tuples!(impl_tuples, 0, 12, A);

/// A scheduler operating on [`Task`]s.
///
/// This trait only concerns about "run queues", not "wait queues". The user
/// should use [`Waker`](core::task::Waker)s for the item of their own wait
/// queues.
pub trait Scheduler: Sized {
    type Metadata: Switch;

    /// Push a task to a run queue of the scheduler for execution.
    fn enqueue(&self, task: Task<Self::Metadata>);

    /// Pop a task from a run queue of the scheduler for execution.
    fn dequeue(&self) -> Option<Task<Self::Metadata>>;
}

impl<T: Deref<Target = S>, S: Scheduler> Scheduler for T {
    type Metadata = S::Metadata;

    fn enqueue(&self, task: Task<Self::Metadata>) {
        (**self).enqueue(task)
    }

    fn dequeue(&self) -> Option<Task<Self::Metadata>> {
        (**self).dequeue()
    }
}

/// The provided methods for [schedulers](Scheduler).
pub trait SchedulerExt: Scheduler {
    /// Schedule the current running task for another chance for execution.
    ///
    /// `f` decides the destination of the current running task, such as a wait
    /// queue or a scheduler's run queue.
    ///
    /// This function is similar to
    /// [`try_schedule`](SchedulerExt::try_schedule), but spins when there's
    /// no other schedulable tasks.
    fn schedule(&self, f: impl FnOnce(Task<Self::Metadata>)) {
        loop {
            if let Some(next) = self.dequeue() {
                next.resume(f);
                break;
            }
            hint::spin_loop()
        }
    }

    /// Try to schedule the current running task for another chance for
    /// execution.
    ///
    /// `f` decides the destination of the current running task, such as a wait
    /// queue or a scheduler's run queue.
    ///
    /// Note that this function will not perform the context switch if there's
    /// no other schedulable tasks, which the boolean return value indicates.
    fn try_schedule(&self, f: impl FnOnce(Task<Self::Metadata>)) -> bool {
        if let Some(next) = self.dequeue() {
            next.resume(f);
            return true;
        }
        false
    }

    /// Yield the current running task to the scheduler.
    ///
    /// This method is a shorthand for
    /// [`try_schedule(|t| self.enqueue(t))`](SchedulerExt::try_schedule).
    fn yield_now(&self) -> bool {
        self.try_schedule(|task| self.enqueue(task))
    }

    /// Spawn a new [`Task`] controlled by this scheduler.
    fn spawn<F, S, P>(
        self,
        builder: Builder<S, P>,
        metadata: Self::Metadata,
        f: F,
    ) -> Result<Task<Self::Metadata>, NewError>
    where
        F: FnOnce(&mut SchedContext<Self>) + Send + 'static,
        S: Into<Stack>,
        P: PanicHook,
        Self: Send + 'static,
    {
        let co = builder.spawn(move |other| {
            assert!(other.is_none(), "A task escaped from enqueueing!");
            let mut cx = SchedContext::new(self);
            f(&mut cx);
            loop {
                if let Some(next) = cx.dequeue() {
                    let _ = next.metadata.switch();
                    break next.co;
                }
                hint::spin_loop()
            }
        })?;
        Ok(Task { co, metadata })
    }
}
impl<T: Scheduler> SchedulerExt for T {}

pub trait SymWait: IntoFuture + Sized {
    /// Wait on a future "synchronously".
    fn wait<S, R, M>(self, cx: &mut SchedContext<S>) -> Self::Output
    where
        S: Scheduler<Metadata = M> + Send + Sync + 'static,
        M: Switch + Send,
        <Self as IntoFuture>::IntoFuture: Send,
    {
        let mut future = pin!(self.into_future());
        loop {
            match future
                .as_mut()
                .poll(&mut Context::from_waker(&cx.as_waker()))
            {
                Poll::Ready(output) => break output,
                Poll::Pending => cx.wait(),
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
    use unico_ful::Builder;
    use unico_stack::global_stack_allocator;

    use super::{Scheduler, SchedulerExt, Task};

    global_resumer!(Boost);
    global_stack_allocator!(Global);

    struct Fifo(Mutex<VecDeque<Task>>);

    impl Scheduler for Fifo {
        type Metadata = ();

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
        let t1 = sched.clone().spawn(Default::default(), (), |s| {
            println!("1");
            s.yield_now();
            println!("2");
        });
        let t2 = sched.clone().spawn(Default::default(), (), |s| {
            println!("3");
            s.yield_now();
            println!("4");
        });
        sched.enqueue(t2.unwrap());
        println!("Start");
        t1.unwrap().resume(|task| sched.enqueue(task));
        println!("5");
        assert!(sched.yield_now());
        println!("6\nEnd");
        assert!(!sched.yield_now());
    }

    #[test]
    fn panicked() {
        let sched = Arc::new(Fifo(Mutex::new(VecDeque::new())));
        let s2 = sched.clone();
        let s3 = sched.clone();
        let builder = Builder::new().hook_panic_with(move |_| {
            s2.schedule(drop);
            unreachable!()
        });
        let p = s3.spawn(builder, (), |_| panic!("What the fuck?"));
        p.unwrap().resume(|task| sched.enqueue(task));
        assert!(!sched.yield_now());
    }
}
