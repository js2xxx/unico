use core::{
    alloc::Layout,
    intrinsics::{abort, atomic_cxchg_acqrel_acquire},
    ptr::{self, NonNull},
    sync::atomic::AtomicU8,
};

use unico_context::{Resume, Transfer};

use crate::{layout::extend, Co, NewError, RawStack};

/// The function hasn't been started.
const INITIALIZED: u8 = 0;
/// The function has yielded during its execution.
const STARTED: u8 = 1;
/// The function has been consumed.
const FINISHED: u8 = 3;

pub(crate) struct Header<R: Resume> {
    state: AtomicU8,
    rs: R,
}

struct Layouts {
    layout: Layout,
    offset_func: usize,
    offset_stack: usize,
}

pub(crate) struct RawCo<F, R: Resume> {
    header: *mut Header<R>,
    func: *mut F,
    stack: *mut RawStack,
}

impl<F, R: Resume> RawCo<F, R> {
    const fn eval_layouts() -> Option<Layouts> {
        let header = Layout::new::<Header<R>>();
        let func = Layout::new::<F>();
        let stack = Layout::new::<RawStack>();

        let layout = header;
        let (layout, offset_func) = ct!(extend(layout, func));
        let (layout, offset_stack) = ct!(extend(layout, stack));
        Some(Layouts {
            layout,
            offset_func,
            offset_stack,
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
            stack: ptr.map_addr(|addr| addr + layouts.offset_stack).cast(),
        }
    }

    unsafe fn update_state(&self, old: u8, src: u8) -> bool {
        let state = ptr::addr_of_mut!((*self.header).state);
        atomic_cxchg_acqrel_acquire(state.cast(), old, src).1
    }
}

impl<F, R> RawCo<F, R>
where
    F: FnOnce(Option<Co<R>>) -> Co<R>,
    R: Resume,
{
    pub(crate) fn new_on(
        stack: RawStack,
        rs: &R,
        func: F,
    ) -> Result<R::Context, NewError<R>> {
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
            rs.new_on(
                NonNull::slice_from_raw_parts(stack.base(), rest_size),
                Self::entry,
            )
        }
        .map_err(NewError::Context)?;

        let raw = Self::from_ptr(pointer);
        unsafe {
            raw.header.write(Header {
                state: AtomicU8::new(INITIALIZED),
                rs: rs.clone(),
            });
            raw.func.write(func);
            raw.stack.write(stack);
        }

        let resume = unsafe { rs.resume(context, pointer) };
        Ok(resume.context.unwrap())
    }
}

impl<F, R> RawCo<F, R>
where
    F: FnOnce(Option<Co<R>>) -> Co<R>,
    R: Resume,
{
    unsafe extern "C" fn entry(cx: R::Context, ptr: *mut ()) -> ! {
        let task = Self::from_ptr(ptr);

        let rs = unsafe { (*task.header).rs.clone() };
        let Transfer { context, .. } = unsafe { rs.resume(cx, ptr) };
        unsafe {
            if task.update_state(INITIALIZED, STARTED) {
                let Co { context, .. } =
                    (task.func.read())(context.map(|context| Co { context, rs }));

                task.update_state(STARTED, FINISHED);

                (*task.header).rs.resume_with(context, ptr, Self::exit);
            }
        }
        abort()
    }

    #[allow(improper_ctypes_definitions)]
    unsafe extern "C" fn exit(_: R::Context, ptr: *mut ()) -> Transfer<R::Context> {
        let task = Self::from_ptr(ptr);
        unsafe {
            ptr::drop_in_place(ptr::addr_of_mut!((*task.header).rs));
            drop(task.stack.read())
        }
        Transfer {
            context: None,
            data: ptr::null_mut(),
        }
    }
}

#[allow(improper_ctypes_definitions)]
pub(crate) unsafe extern "C" fn map<R: Resume, M: FnOnce(Co<R>) -> Option<Co<R>>>(
    cx: R::Context,
    ptr: *mut (),
) -> Transfer<R::Context> {
    let (rs, func) = unsafe { ptr.cast::<(R, M)>().read() };
    let ret = func(Co { context: cx, rs });
    Transfer {
        context: ret.map(|Co { context, .. }| context),
        data: ptr::null_mut(),
    }
}
