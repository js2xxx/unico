#![no_std]
#![feature(alloc_layout_extra)]
#![feature(const_alloc_layout)]
#![feature(core_intrinsics)]
#![feature(strict_provenance)]

macro_rules! ct {
    ($e:expr) => {
        match $e {
            Some(value) => value,
            None => return None,
        }
    };
}

mod layout;
mod raw;
pub mod stack;

use core::{alloc::Layout, ptr};

use unico_context::{Resume, Transfer};

use crate::{raw::RawCo, stack::RawStack};

#[cfg(test)]
extern crate alloc;

#[derive(Debug)]
pub enum NewError<R: Resume> {
    StackTooSmall { expected: Layout, actual: Layout },
    Context(R::NewError),
}

#[derive(Debug)]
pub struct Co<R: Resume> {
    context: R::Context,
    rs: R,
}

impl<R: Resume> Co<R> {
    pub fn new(
        stack: RawStack,
        rs: R,
        func: impl FnOnce(Option<Self>) -> Self,
    ) -> Result<Self, NewError<R>> {
        RawCo::new_on(stack, &rs, func).map(|context| Co { context, rs })
    }

    pub fn resume(self) -> Option<Self> {
        let Co { context, rs } = self;
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
        context.map(|context| Co { context, rs })
    }
}

#[cfg(test)]
mod tests {
    use core::ptr::NonNull;
    use alloc::alloc::{alloc, dealloc};

    use unico_context::boost::Boost;

    use super::*;

    fn raw_stack() -> RawStack {
        #[repr(align(4096))]
        struct Stack([u8; 4096]);

        // UNSAFE: `memory` and `drop` are valid.
        unsafe {
            let layout = Layout::new::<Stack>();
            let memory = alloc(layout);
            RawStack::new(NonNull::new_unchecked(memory), layout, |ptr, layout| {
                dealloc(ptr.as_ptr(), layout)
            })
        }
    }

    #[test]
    fn creation() {
        Co::new(raw_stack(), Boost, Option::unwrap).unwrap();
    }

    #[test]
    fn empty() {
        let co = Co::new(raw_stack(), Boost, Option::unwrap).unwrap();
        let ret = co.resume();
        assert!(ret.is_none());
    }

    #[test]
    fn capture() {
        let mut a = 0;
        let co = Co::new(raw_stack(), Boost, |co| {
            a = 1;
            co.unwrap()
        })
        .unwrap();
        let ret = co.resume();
        assert!(ret.is_none());
        assert_eq!(a, 1);
    }

    #[test]
    fn asymmetric() {
        let mut counter = 0;
        let mut co = Co::new(raw_stack(), Boost, |mut co| {
            for _ in 0..10 {
                counter += 1;
                co = co.unwrap().resume();
            }
            co.unwrap()
        })
        .unwrap();
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
        let b = Co::new(raw_stack(), Boost, |a| {
            let c = Co::new(raw_stack(), Boost, move |b| {
                let ret = b.unwrap().resume();
                assert!(ret.is_none());
                a.unwrap()
            })
            .unwrap();
            c.resume().unwrap()
        })
        .unwrap();
        let ret = b.resume();
        assert!(ret.is_none());
    }

    #[test]
    fn symmetric_direct() {
        let b = Co::new(raw_stack(), Boost, |a| {
            Co::new(raw_stack(), Boost, move |_| a.unwrap()).unwrap()
        })
        .unwrap();
        let ret = b.resume();
        assert!(ret.is_none());
    }
}
