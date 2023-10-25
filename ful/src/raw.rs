mod panicking;

use core::{
    alloc::Layout,
    any::type_name,
    ptr::{self, NonNull},
};

use unico_context::{Resume, Transfer};
#[cfg(feature = "alloc")]
use unwinding::panic;

pub use self::panicking::*;
use crate::{layout::extend, Co, NewError, Stack};

struct Layouts {
    layout: Layout,
    offset_func: usize,
    offset_stack: usize,
    offset_hook: usize,
}

pub(crate) struct RawCo<F, R: Resume, P: PanicHook<R>> {
    rs: *mut R,
    func: *mut F,
    stack: *mut Stack,
    panic_hook: *mut P,
}

impl<F, R: Resume, P: PanicHook<R>> RawCo<F, R, P> {
    const fn eval_layouts() -> Option<Layouts> {
        let rs = Layout::new::<R>();
        let func = Layout::new::<F>();
        let stack = Layout::new::<Stack>();
        let hook = Layout::new::<P>();

        let layout = rs;
        let (layout, offset_func) = ct!(extend(layout, func));
        let (layout, offset_stack) = ct!(extend(layout, stack));
        let (layout, offset_hook) = ct!(extend(layout, hook));
        Some(Layouts {
            layout,
            offset_func,
            offset_stack,
            offset_hook,
        })
    }

    const LAYOUTS: Option<Layouts> = Self::eval_layouts();

    fn layouts() -> Layouts {
        match Self::LAYOUTS {
            Some(layouts) => layouts,
            None => unreachable!(
                "Layout evaluation failed for {} and {}",
                type_name::<R>(),
                type_name::<F>()
            ),
        }
    }

    fn from_ptr(ptr: *mut ()) -> Self {
        let layouts = Self::layouts();
        RawCo {
            rs: ptr.cast(),
            func: ptr.map_addr(|addr| addr + layouts.offset_func).cast(),
            stack: ptr.map_addr(|addr| addr + layouts.offset_stack).cast(),
            panic_hook: ptr.map_addr(|addr| addr + layouts.offset_hook).cast(),
        }
    }
}

impl<F, R, P> RawCo<F, R, P>
where
    F: FnOnce(Option<Co<R>>) -> Co<R>,
    R: Resume,
    P: PanicHook<R>,
{
    /// # Safety
    ///
    /// See [`crate::Builder::spawn_unchecked`] for more information.
    pub(crate) unsafe fn new_on(
        stack: Stack,
        rs: &R,
        panic_hook: P,
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

        // SAFETY: The pointer in `stack` is valid according to its constructor
        // `Stack::new`.
        let context = unsafe {
            rs.new_on(
                NonNull::slice_from_raw_parts(stack.base(), rest_size),
                Self::entry,
            )
        }
        .map_err(NewError::Context)?;

        let raw = Self::from_ptr(pointer);
        // SAFETY: `raw` is created from `pointer`, which is calculated above and
        // resides somewhere unique in `stack`.
        unsafe {
            raw.rs.write(rs.clone());
            raw.func.write(func);
            raw.stack.write(stack);
            raw.panic_hook.write(panic_hook);
        }

        // SAFETY: The proof is the same as the one in `Co::resume`.
        let resume = unsafe { rs.resume(context, pointer) };
        Ok(resume.context.unwrap())
    }
}

impl<F, R, P> RawCo<F, R, P>
where
    F: FnOnce(Option<Co<R>>) -> Co<R>,
    R: Resume,
    P: PanicHook<R>,
{
    /// # Safety
    ///
    /// `ptr` must points to a valid `RawCo` calculated from `RawCo::from_ptr`.
    unsafe extern "C" fn entry(cx: R::Context, ptr: *mut ()) -> ! {
        let task = Self::from_ptr(ptr);

        // SAFETY: The task is valid by contract.
        let rs = unsafe { (*task.rs).clone() };
        #[cfg(feature = "alloc")]
        let hook = unsafe { task.panic_hook.read() };

        let run = || {
            // SAFETY: The task is valid by contract. `func` must be read before the
            // initial resume, in case of early unwinding.
            let func = unsafe { task.func.read() };
            // SAFETY: The proof is the same as the one in `Co::resume`.
            let Transfer { context, .. } = unsafe { rs.resume(cx, ptr) };
            func(context.map(|cx| Co::from_inner(cx, rs)))
        };

        #[cfg(feature = "alloc")]
        let context = {
            // Move the hook in the braces to make sure it drops when the control flow
            // goes out of the scope.
            let hook = hook;
            let result = panic::catch_unwind(run);
            match result {
                Ok(co) => Co::into_inner(co).0,
                Err(payload) => match payload.downcast::<HandleDrop<R::Context>>() {
                    Ok(hd) => hd.0,
                    Err(payload) => Co::into_inner(hook.rewind(payload)).0,
                },
            }
        };
        #[cfg(not(feature = "alloc"))]
        let context = Co::into_inner(run()).0;

        // SAFETY: The proof is the same as the one in `Co::resume`.
        unsafe { (*task.rs).resume_with(context, ptr, Self::exit) };
        unreachable!("Exiting failed. There's at least some dangling `Co` instance!")
    }

    /// # Safety
    ///
    /// `ptr` must points to a valid `RawCo` calculated from `RawCo::from_ptr`.
    #[allow(improper_ctypes_definitions)]
    unsafe extern "C" fn exit(_: R::Context, ptr: *mut ()) -> Transfer<R::Context> {
        let task = Self::from_ptr(ptr);
        // SAFETY: The task is valid by contract.
        unsafe {
            ptr::drop_in_place(task.rs);
            // `stack` must not be dropped in place to avoid access to dropped stack
            // memory.
            drop(task.stack.read())
        }
        Transfer {
            context: None,
            data: ptr::null_mut(),
        }
    }
}

/// # Safety
///
/// `ptr` must offer a valid tuple of `R` and `M`.
#[allow(improper_ctypes_definitions)]
pub(crate) unsafe extern "C" fn map<R: Resume, M: FnOnce(Co<R>) -> Option<Co<R>>>(
    cx: R::Context,
    ptr: *mut (),
) -> Transfer<R::Context> {
    // SAFETY: The only reading is safe by contract.
    let (rs, func) = unsafe { ptr.cast::<(R, M)>().read() };
    let ret = func(Co::from_inner(cx, rs));
    Transfer {
        context: ret.map(|co| Co::into_inner(co).0),
        data: ptr::null_mut(),
    }
}
