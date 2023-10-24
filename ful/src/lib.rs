#![no_std]
#![feature(alloc_layout_extra)]
#![feature(const_alloc_layout)]
#![feature(core_intrinsics)]
#![feature(strict_provenance)]

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
        func: impl FnOnce(Self) -> Self,
    ) -> Result<Self, NewError<R>> {
        RawCo::new_on(stack, &rs, func).map(|context| Co { context, rs })
    }

    pub fn resume(self) -> Option<Self> {
        let Co { context, rs } = self;
        let Transfer { context, .. } = unsafe {
            rs.resume(Transfer {
                context: Some(context),
                data: ptr::null_mut(),
            })
        };
        context.map(|context| Co { context, rs })
    }
}

#[cfg(test)]
mod tests {
    use core::{convert::identity, ptr::NonNull};
    use alloc::alloc::{alloc, dealloc};

    use unico_context::boost::Boost;

    use super::*;

    fn raw_stack() -> RawStack {
        #[repr(align(4096))]
        struct Stack([u8; 4096]);

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
        Co::new(raw_stack(), Boost, identity).unwrap();
    }

    #[test]
    fn empty() {
        let co = Co::new(raw_stack(), Boost, identity).unwrap();
        let ret = co.resume();
        assert!(ret.is_none());
    }

    #[test]
    fn capture() {
        let mut a = 0;
        let co = Co::new(raw_stack(), Boost, |co| {
            a = 1;
            co
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
                co = co.resume().unwrap();
            }
            co
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
                let ret = b.resume();
                assert!(ret.is_none());
                a
            })
            .unwrap();
            c.resume().unwrap()
        })
        .unwrap();
        let ret = b.resume();
        assert!(ret.is_none());
    }
}
