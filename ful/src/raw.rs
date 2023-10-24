use core::{
    alloc::Layout,
    intrinsics::{abort, atomic_cxchg_acqrel_acquire},
    mem::ManuallyDrop,
    ptr::{self, NonNull},
    sync::atomic::AtomicU8,
};

use unico_context::{Context, Transfer};

use crate::{layout::extend, Co, NewError, RawStack};

/// The function hasn't been started.
const INITIALIZED: u8 = 0;
/// The function has yielded during its execution.
const STARTED: u8 = 1;
/// The function has been consumed.
const FINISHED: u8 = 3;

pub(crate) struct Header<C: Context> {
    pub(crate) vtable: &'static VTable<C::Context>,
    state: AtomicU8,
    stack: RawStack,
    cx: C,
}

pub(crate) struct VTable<T> {
    pub(crate) resume: unsafe fn(Transfer<T>) -> Transfer<T>,
    pub(crate) drop: unsafe fn(ptr: *mut ()),
}

struct Layouts {
    layout: Layout,
    offset_func: usize,
}

pub(crate) struct RawCo<F, C: Context> {
    header: *mut Header<C>,
    func: *mut F,
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
    pub(crate) fn new_on(
        stack: RawStack,
        cx: C,
        func: F,
    ) -> Result<Transfer<C::Context>, NewError<C>> {
        let layouts = Self::layouts();
        let stack_layout = stack.layout();
        if stack_layout.size() <= layouts.layout.size()
            || stack_layout.align() < layouts.layout.align()
        {
            return Err(NewError::StackTooSmall {
                expected: layouts.layout,
                actual: stack_layout,
            });
        }

        let (pointer, rest_size) = {
            let layouts = Self::layouts();
            let bottom = stack.base().as_ptr();

            let addr = bottom.addr() + stack_layout.size() - layouts.layout.size();
            let addr = addr & !(layouts.layout.align() - 1);

            let pointer = bottom.with_addr(addr).cast();
            (pointer, addr - bottom.addr())
        };

        let context = unsafe {
            cx.new_on(
                NonNull::slice_from_raw_parts(stack.base(), rest_size),
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
