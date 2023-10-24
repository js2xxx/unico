#![no_std]
#![feature(alloc_layout_extra)]
#![feature(const_alloc_layout)]
#![feature(core_intrinsics)]
#![feature(strict_provenance)]

mod layout;
mod raw;
pub mod stack;

use core::{
    alloc::Layout,
    mem::{self, ManuallyDrop},
};

use unico_context::{Context, Transfer};

use crate::{
    raw::{Header, RawCo},
    stack::RawStack,
};

#[cfg(test)]
extern crate alloc;

#[derive(Debug)]
pub enum NewError<C: Context> {
    StackTooSmall { expected: Layout, actual: Layout },
    Context(C::NewError),
}

#[derive(Debug)]
pub struct Co<C: Context>(ManuallyDrop<Transfer<C::Context>>);

impl<C: Context> Co<C> {
    pub fn new(
        stack: RawStack,
        cx: C,
        func: impl FnOnce(Self) -> Self,
    ) -> Result<Self, NewError<C>> {
        RawCo::new_on(stack, cx, func).map(|cx| Co(ManuallyDrop::new(cx)))
    }

    fn into_inner(mut self) -> Transfer<C::Context> {
        unsafe {
            let ret = ManuallyDrop::take(&mut self.0);
            mem::forget(self);
            ret
        }
    }

    pub fn resume(self) -> Option<Self> {
        let header = self.0.data.cast::<Header<C>>();
        let t = unsafe { ((*header).vtable.resume)(self.into_inner()) };
        (!t.data.is_null()).then(|| Co(ManuallyDrop::new(t)))
    }
}

impl<C: Context> Drop for Co<C> {
    fn drop(&mut self) {
        let header = self.0.data.cast::<Header<C>>();
        unsafe { ((*header).vtable.drop)(self.0.data) }
    }
}

#[cfg(test)]
mod tests {
    use alloc::alloc::{alloc, dealloc};
    use core::{convert::identity, ptr::NonNull};

    use unico_context::{boost::Boost, ucx::Ucontext};

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
        let b = Co::new(raw_stack(), Ucontext, |a| {
            let c = Co::new(raw_stack(), Ucontext, move |b| {
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
