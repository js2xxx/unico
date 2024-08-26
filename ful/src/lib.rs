#![no_std]
#![feature(alloc_layout_extra)]
#![feature(const_alloc_layout)]
#![feature(strict_provenance)]

macro_rules! ct {
    ($e:expr) => {
        match $e {
            Some(value) => value,
            None => return None,
        }
    };
}

mod builder;
mod layout;
mod raw;
pub mod stack;

#[cfg(feature = "alloc")]
use alloc::boxed::Box;
#[cfg(feature = "alloc")]
use core::any::Any;
use core::{
    alloc::Layout,
    mem::{self, ManuallyDrop},
    ptr,
};

use unico_context::{Resume, Transfer};

use crate::stack::RawStack;
pub use crate::{
    builder::*,
    raw::{AbortHook, PanicHook},
};

#[cfg(feature = "alloc")]
extern crate alloc;

#[cfg(test)]
extern crate std;

#[derive(Debug)]
pub enum NewError<R: Resume> {
    StackTooSmall { expected: Layout, actual: Layout },
    Context(R::NewError),
}

/// A continuation of the current control flow.
///
/// This structure represents a continuation, a.k.a. the handle of a symmetric
/// coroutine.
///
/// Note that if `alloc` feature is not enabled and the coroutine is dropped
/// while not resumed to end, the stack allocation and context resumer will be
/// ***LEAKED***. It's because the dropping process requires unwinding, and thus
/// a `Box<dyn Any + Send>`.
#[derive(Debug)]
pub struct Co<R: Resume> {
    context: ManuallyDrop<R::Context>,
    rs: ManuallyDrop<R>,
}

// SAFETY: The bounds of the actual function will be checked in the builder.
unsafe impl<R: Resume + Send + Sync> Send for Co<R> {}
unsafe impl<R: Resume + Send + Sync> Sync for Co<R> {}

// The auto derivation also requires `Resume::Context` to be `Unpin`, which is
// unnecessary because the context is already pinned on the stack.
impl<R: Resume + Unpin> Unpin for Co<R> {}

impl<R: Resume> Co<R> {
    fn from_inner(context: R::Context, rs: R) -> Self {
        Co {
            context: ManuallyDrop::new(context),
            rs: ManuallyDrop::new(rs),
        }
    }

    fn into_inner(mut this: Self) -> (R::Context, R) {
        unsafe {
            let context = ManuallyDrop::take(&mut this.context);
            let rs = ManuallyDrop::take(&mut this.rs);
            mem::forget(this);
            (context, rs)
        }
    }
}

impl<R: Resume> Co<R> {
    /// Initiate a builder for a coroutine with some defaults.
    pub const fn builder(rs: R) -> Builder<R, (), AbortHook> {
        Builder::with(rs)
    }

    /// Move the current control flow to this continuation.
    ///
    /// This method moves the current control flow to this continuation,
    /// alongside with this object's ownership. Note that the return value may
    /// not be the same [`Co`] as the callee because this method is symmetric.
    pub fn resume(self) -> Option<Self> {
        let (context, rs) = Co::into_inner(self);
        // SAFETY: `context`'s lifetime is bound to its own coroutine, and it is ALWAYS
        // THE UNIQUE REFERENCE to the runtime stack. The proof is divided into 2
        // points:
        //
        // 1. If the coroutines are assymetrically nested, like B in A->B->C, in both A
        //    and C's context the reference(s) of B seem to coexist. However, the
        //    statement is that they never coexist at all:
        //
        //    1) Before the resumption from A to B, the reference of B in C is never
        //       instantiated;
        //    2) Between the resumption from A to B and the one from B to C, the actual
        //       reference is implicitly with B's call stack, unreachable in both A and
        //       C;
        //    3) After the resumption from B to C, the reference of B in C is
        //       instantiated, while the one in A is dropped just as the code below.
        //
        //    The returning procedure performs like the inverse of the procedure above,
        // thus proving the statement.
        //
        // 2. If the coroutines are symmetrically transferred, and someone attempted to
        //    resume B from both A and C in 2 executing units (like 2 CPU cores), they
        //    simply cannot reach this evil goal:
        //
        //    1) The only method to create the pseudo-double-reference-of-B scenario is
        //       to perform the nested creation, i.e. the example in point 1;
        //    2) To execute C in another executing unit, the user must move the
        //       ownership of C to another call stack;
        //    3) However, once C is actually executed in another unit, its main
        //       function's argument will be the reference of that call stack, instead
        //       of B's.
        //
        //    Thus, though the naming of variables will be a bit rough, the statement
        // actually proves to be true.
        let Transfer { context, .. } = unsafe { rs.resume(context, ptr::null_mut()) };

        context.map(|context| Co::from_inner(context, rs))
    }

    /// Similar to [`Co::resume`], but maps the source of this continuation,
    /// a.k.a. the caller of this method, to another one if possible.
    ///
    /// This method resumes the continuation, and then executes `map` on the
    /// call stack of that continuation. The only argument of `map` is the
    /// current control flow calling this method.
    pub fn resume_with<M: FnOnce(Self) -> Option<Self>>(self, map: M) -> Option<Self> {
        let (context, rs) = Co::into_inner(self);

        let mut data = ManuallyDrop::new((rs.clone(), map));
        let ptr = (&mut data as *mut ManuallyDrop<(R, M)>).cast();

        // SAFETY: The proof is the same as the one in `Co::resume`.
        let Transfer { context, .. } =
            unsafe { rs.resume_with(context, ptr, raw::map::<R, M>) };

        context.map(|context| Co::from_inner(context, rs))
    }

    /// Resume the unwinding for this coroutine's partial destruction process.
    /// Use it in the process of error handling for some `catch_unwind`.
    ///
    /// Note that if you don't use it according to the text above, then the
    /// destruction process will be aborted and the caller of that process
    /// will not be returned and thus unreachable forever!
    #[cfg(feature = "alloc")]
    pub fn resume_unwind(&self, payload: Box<dyn Any + Send>) -> Box<dyn Any + Send> {
        raw::resume_unwind(self, payload)
    }
}

impl<R: Resume> Drop for Co<R> {
    fn drop(&mut self) {
        #[allow(unused_variables)]
        // SAFETY： We don't use `self.context` and `self.rs` any longer after taking out
        // data from these fields. The safety proof of `rx.resume_with` is the same as the
        // one in `Co::resume`.
        unsafe {
            let cx = ManuallyDrop::take(&mut self.context);
            let rs = ManuallyDrop::take(&mut self.rs);
            #[cfg(feature = "alloc")]
            rs.resume_with(cx, ptr::null_mut(), raw::unwind::<R>);
        }
    }
}

#[cfg(test)]
mod tests {
    use alloc::string::String;
    use core::ptr::NonNull;
    use std::alloc::{alloc, dealloc};

    use super::*;

    fn raw_stack() -> RawStack {
        #[repr(align(4096))]
        struct Stack([u8; 6144]);

        // UNSAFE: `memory` and `drop` are valid.
        unsafe {
            let layout = Layout::new::<Stack>();
            let memory = alloc(layout);
            std::println!("alloc {memory:p}");
            RawStack::new(NonNull::new_unchecked(memory), layout, |ptr, layout| {
                std::println!("dealloc {ptr:p}");
                dealloc(ptr.as_ptr(), layout)
            })
        }
    }

    #[test]
    fn creation() {
        spawn(raw_stack(), Option::unwrap);
    }

    #[test]
    fn empty() {
        assert!(callcc(raw_stack(), Option::unwrap).is_none());
    }

    #[test]
    fn capture_move() {
        let s = String::from("hello");
        let ret = callcc(raw_stack(), move |co| {
            assert_eq!(s, "hello");
            co.unwrap()
        });
        assert!(ret.is_none());
    }

    #[test]
    fn capture_ref() {
        let mut counter = 0;
        let mut co = unsafe {
            spawn_unchecked(raw_stack(), |mut co| {
                for _ in 0..10 {
                    counter += 1;
                    co = co.unwrap().resume();
                }
                co.unwrap()
            })
        };
        loop {
            co = match co.resume() {
                Some(co) => co,
                None => break,
            }
        }
        assert_eq!(counter, 10);
    }

    #[test]
    fn symmetric() {
        let b = spawn(raw_stack(), |a| {
            let c = spawn(raw_stack(), move |b| {
                let ret = b.unwrap().resume();
                assert!(ret.is_none());
                a.unwrap()
            });
            c.resume().unwrap()
        });
        let ret = b.resume();
        assert!(ret.is_none());
    }

    #[test]
    fn symmetric_direct() {
        let b = spawn(raw_stack(), |a| spawn(raw_stack(), move |_| a.unwrap()));
        let ret = b.resume();
        assert!(ret.is_none());
    }
}
