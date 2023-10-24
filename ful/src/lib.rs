#![no_std]
#![feature(alloc_layout_extra)]
#![feature(const_alloc_layout)]
#![feature(core_intrinsics)]
#![feature(strict_provenance)]

use core::{
    alloc::Layout,
    intrinsics::{abort, atomic_cxchg_acqrel_acquire},
    mem::{self, ManuallyDrop},
    ptr::{self, NonNull},
    sync::atomic::AtomicU8,
};

use unico_context::{Context, Transfer};

#[cfg(test)]
extern crate alloc;

pub struct RawStack {
    pointer: NonNull<u8>,
    layout: Layout,
    drop: unsafe fn(NonNull<u8>, Layout),
}

impl RawStack {
    /// Creates a new [`RawStack`].
    ///
    /// # Safety
    ///
    /// `pointer` must contains a valid block of memory of `layout`, and must be
    /// able to be dropped by `drop`.
    pub unsafe fn new(
        pointer: NonNull<u8>,
        layout: Layout,
        drop: unsafe fn(NonNull<u8>, Layout),
    ) -> Self {
        RawStack {
            pointer,
            layout,
            drop,
        }
    }
}

impl Drop for RawStack {
    fn drop(&mut self) {
        unsafe { (self.drop)(self.pointer, self.layout) }
    }
}

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

/// The function hasn't been started.
const INITIALIZED: u8 = 0;
/// The function has yielded during its execution.
const STARTED: u8 = 1;
/// The function has been consumed.
const FINISHED: u8 = 3;

struct Header<C: Context> {
    state: AtomicU8,
    vtable: &'static VTable<C::Context>,
    stack: RawStack,
    cx: C,
}

struct VTable<T> {
    resume: unsafe fn(Transfer<T>) -> Transfer<T>,
    drop: unsafe fn(ptr: *mut ()),
}

struct Layouts {
    layout: Layout,
    offset_func: usize,
}

struct RawCo<F, C: Context> {
    header: *mut Header<C>,
    func: *mut F,
}

const fn max(a: usize, b: usize) -> usize {
    if a > b {
        a
    } else {
        b
    }
}

const fn extend(layout: Layout, next: Layout) -> Option<(Layout, usize)> {
    let new_align = max(layout.align(), next.align());
    let pad = layout.padding_needed_for(next.align());

    let offset = match layout.size().checked_add(pad) {
        Some(offset) => offset,
        None => return None,
    };
    let new_size = match offset.checked_add(next.size()) {
        Some(new_size) => new_size,
        None => return None,
    };

    // The safe constructor is called here to enforce the isize size limit.
    let layout = unsafe { Layout::from_size_align_unchecked(new_size, new_align) };
    Some((layout, offset))
}

impl<F, C: Context> RawCo<F, C> {
    const fn eval_layouts() -> Option<Layouts> {
        let header = Layout::new::<Header<C>>();
        let func = Layout::new::<F>();

        let (layout, offset_func) = match extend(header, func) {
            Some(union) => union,
            None => return None,
        };
        Some(Layouts {
            layout,
            offset_func,
        })
    }

    const LAYOUTS: Option<Layouts> = Self::eval_layouts();

    fn layouts() -> Layouts {
        match Self::LAYOUTS {
            Some(layouts) => layouts,
            None => abort(),
        }
    }

    fn from_ptr(ptr: *mut ()) -> Self {
        let layouts = Self::layouts();
        RawCo {
            header: ptr.cast(),
            func: ptr.map_addr(|addr| addr + layouts.offset_func).cast(),
        }
    }

    unsafe fn update_state(&self, old: u8, src: u8) -> bool {
        let state = ptr::addr_of_mut!((*self.header).state);
        atomic_cxchg_acqrel_acquire(state.cast(), old, src).1
    }
}

impl<F, C> RawCo<F, C>
where
    F: FnOnce(Co<C>) -> Co<C>,
    C: Context,
{
    fn new_on(
        stack: RawStack,
        cx: C,
        func: F,
    ) -> Result<Transfer<C::Context>, NewError<C>> {
        let layouts = Self::layouts();
        if stack.layout.size() <= layouts.layout.size()
            || stack.layout.align() < layouts.layout.align()
        {
            return Err(NewError::StackTooSmall {
                expected: layouts.layout,
                actual: stack.layout,
            });
        }

        let (pointer, rest_size) = {
            let layouts = Self::layouts();
            let bottom = stack.pointer.as_ptr();

            let addr = bottom.addr() + stack.layout.size() - layouts.layout.size();
            let addr = addr & !(layouts.layout.align() - 1);

            let pointer = bottom.with_addr(addr).cast();
            (pointer, addr - bottom.addr())
        };

        let context = unsafe {
            cx.new_on(
                NonNull::slice_from_raw_parts(stack.pointer, rest_size),
                Self::entry,
            )
        }
        .map_err(NewError::Context)?;

        let raw = Self::from_ptr(pointer);
        unsafe {
            raw.header.write(Header {
                state: AtomicU8::new(INITIALIZED),
                vtable: Self::VTABLE,
                stack,
                cx,
            });
            raw.func.write(func);
        }

        Ok(Transfer {
            context,
            data: pointer,
        })
    }
}

impl<F, C> RawCo<F, C>
where
    F: FnOnce(Co<C>) -> Co<C>,
    C: Context,
{
    extern "C" fn entry(t: Transfer<C::Context>) -> ! {
        let task = Self::from_ptr(t.data);
        unsafe {
            if task.update_state(INITIALIZED, STARTED) {
                let ret = (task.func.read())(Co(ManuallyDrop::new(t)));

                task.update_state(STARTED, FINISHED);

                (*task.header).cx.resume_with(ret.into_inner(), Self::exit);
            }
        }
        abort()
    }

    extern "C" fn exit(t: Transfer<C::Context>) -> Transfer<C::Context> {
        let task = Self::from_ptr(t.data);
        unsafe { ptr::drop_in_place(ptr::addr_of_mut!((*task.header).stack)) }
        Transfer {
            context: t.context,
            data: ptr::null_mut(),
        }
    }
}

impl<F, C: Context> RawCo<F, C> {
    unsafe fn resume(t: Transfer<C::Context>) -> Transfer<C::Context> {
        let task = Self::from_ptr(t.data);
        (*task.header).cx.resume(t)
    }

    unsafe fn drop(ptr: *mut ()) {
        let task = Self::from_ptr(ptr);

        if task.update_state(INITIALIZED, FINISHED) {
            ptr::drop_in_place(task.func);
        }
        ptr::drop_in_place(ptr::addr_of_mut!((*task.header).stack));
    }

    const VTABLE: &'static VTable<C::Context> = &VTable {
        resume: Self::resume,
        drop: Self::drop,
    };
}

#[cfg(test)]
mod tests {
    use alloc::alloc::{alloc, dealloc};
    use core::convert::identity;

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
