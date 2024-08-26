mod panicking;

#[cfg(any(feature = "unwind", feature = "std"))]
use core::panic::AssertUnwindSafe;
use core::{
    alloc::Layout,
    any::type_name,
    ptr::{self, NonNull},
};

use unico_context::{self as cx, Transfer};

pub use self::panicking::*;
use super::{layout::extend, Co, NewError, Stack};
#[cfg(any(feature = "unwind", feature = "std"))]
use crate::unwind;

struct Layouts {
    layout: Layout,
    offset_stack: usize,
    offset_func: usize,
    offset_hook: usize,
}

pub(crate) struct RawCo<F, P: PanicHook> {
    stack: *mut Stack,
    func: *mut F,
    panic_hook: *mut P,
}

impl<F, P: PanicHook> RawCo<F, P> {
    const fn eval_layouts() -> Option<Layouts> {
        let func = Layout::new::<F>();
        let stack = Layout::new::<Stack>();
        let hook = Layout::new::<P>();

        let layout = Layout::new::<()>();
        let (layout, offset_stack) = ct!(extend(layout, stack));
        let (layout, offset_func) = ct!(extend(layout, func));
        let (layout, offset_hook) = ct!(extend(layout, hook));

        assert!(offset_stack == 0);
        Some(Layouts {
            layout,
            offset_stack,
            offset_func,
            offset_hook,
        })
    }

    const LAYOUTS: Option<Layouts> = Self::eval_layouts();

    fn layouts() -> Layouts {
        match Self::LAYOUTS {
            Some(layouts) => layouts,
            None => unreachable!("Layout evaluation failed for {}", type_name::<F>()),
        }
    }

    fn from_ptr(ptr: *mut ()) -> Self {
        let layouts = Self::layouts();
        RawCo {
            stack: ptr.map_addr(|addr| addr + layouts.offset_stack).cast(),
            func: ptr.map_addr(|addr| addr + layouts.offset_func).cast(),
            panic_hook: ptr.map_addr(|addr| addr + layouts.offset_hook).cast(),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct TransferData {
    pub raw: *mut Stack,
    pub payload: *mut (),
}

impl TransferData {
    pub fn as_mut(&mut self) -> *mut () {
        (self as *mut TransferData).cast()
    }

    /// # Safety
    ///
    /// `data` must point to a valid `TransferData` and guarantee a unique
    /// reference.
    pub unsafe fn from_mut<'a>(data: *mut ()) -> &'a mut Self {
        unsafe { &mut *data.cast::<Self>() }
    }

    /// # Safety
    ///
    /// `data` must point to a valid `TransferData` and guarantee a unique
    /// reference or be null.
    pub unsafe fn read(data: *mut ()) -> Self {
        if data.is_null() {
            TransferData {
                raw: ptr::null_mut(),
                payload: ptr::null_mut(),
            }
        } else {
            *Self::from_mut(data)
        }
    }
}

impl<F, P> RawCo<F, P>
where
    F: FnOnce(Option<Co>) -> Co,

    P: PanicHook,
{
    /// # Safety
    ///
    /// See `super::Builder::spawn_unchecked` for more information.
    pub(crate) unsafe fn new_on(
        stack: Stack,
        panic_hook: P,
        func: F,
    ) -> Result<Co, NewError> {
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
            cx::new_on(
                NonNull::slice_from_raw_parts(stack.base(), rest_size),
                Self::entry,
            )
        }
        .map_err(NewError::Context)?;

        let raw = Self::from_ptr(pointer);
        // SAFETY: `raw` is created from `pointer`, which is calculated above and
        // resides somewhere unique in `stack`.
        unsafe {
            raw.stack.write(stack);
            raw.func.write(func);
            raw.panic_hook.write(panic_hook);
        }

        // SAFETY: The proof is the same as the one in `Co::resume_payloaded`.
        let resume = unsafe { cx::resume(context, pointer) };
        Ok(Co::from_inner(resume.context.unwrap(), raw.stack))
    }
}

impl<F, P> RawCo<F, P>
where
    F: FnOnce(Option<Co>) -> Co,

    P: PanicHook,
{
    /// # Safety
    ///
    /// `ptr` must points to a valid `RawCo` calculated from `RawCo::from_ptr`.
    unsafe extern "C" fn entry(cx: NonNull<()>, ptr: *mut ()) -> ! {
        let task = Self::from_ptr(ptr);

        // SAFETY: The task is valid by contract.
        #[cfg(any(feature = "unwind", feature = "std"))]
        let hook = unsafe { task.panic_hook.read() };
        // SAFETY: The task is valid by contract. `func` must be read before the
        // initial resume, in case of early unwinding.
        let func = unsafe { task.func.read() };

        let run = || {
            // SAFETY: The proof is the same as the one in `Co::resume_payloaded`.
            let Transfer { context, data } = unsafe { cx::resume(cx, ptr) };
            // SAFETY: The proof is the same as the one in `Co::resume_payloaded`.
            let raw = unsafe { TransferData::read(data) }.raw;
            func(context.map(|cx| Co::from_inner(cx, raw)))
        };

        #[cfg(any(feature = "unwind", feature = "std"))]
        let (context, next) = {
            // Move the hook in the braces to make sure it drops when the control flow
            // goes out of the scope.
            let hook = hook;
            match unwind::catch_unwind(AssertUnwindSafe(run)) {
                Ok(co) => Co::into_inner(co),
                Err(payload) => match payload.downcast::<HandleDrop>() {
                    Ok(hd) => (hd.cx, hd.raw),
                    Err(payload) => Co::into_inner(hook.rewind(payload)),
                },
            }
        };
        #[cfg(not(any(feature = "unwind", feature = "std")))]
        let (context, next) = Co::into_inner(run());

        let old = super::CUR.replace(next.cast());
        debug_assert_eq!(old, ptr);
        // SAFETY: The proof is the same as the one in `Co::resume_payloaded`.
        unsafe { cx::resume_with(context, ptr, Self::exit) };
        unreachable!("Exiting failed. There's at least some dangling `Co` instance!")
    }

    /// # Safety
    ///
    /// `ptr` must points to a valid `RawCo` calculated from `RawCo::from_ptr`.
    #[allow(improper_ctypes_definitions)]
    unsafe extern "C" fn exit(_: NonNull<()>, ptr: *mut ()) -> Transfer<()> {
        let task = Self::from_ptr(ptr);
        // SAFETY: The task is valid by contract.
        unsafe {
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
/// `ptr` must offer a valid tuple of `M`.
#[allow(improper_ctypes_definitions)]
pub(super) unsafe extern "C" fn map<M: FnOnce(Co) -> (Option<Co>, *mut ())>(
    cx: NonNull<()>,
    ptr: *mut (),
) -> Transfer<()> {
    // SAFETY: The proof is the same as the one in `Co::resume_payloaded`.
    let data = unsafe { TransferData::from_mut(ptr) };
    // SAFETY: The only reading is safe by contract.
    let func = unsafe { ptr.cast::<M>().read() };
    let (ret, payload) = func(Co::from_inner(cx, data.raw));
    Transfer {
        context: ret.map(|co| {
            let (cx, raw) = Co::into_inner(co);
            data.raw = raw;
            data.payload = payload;
            cx
        }),
        // We return the pointer to the same `TransferData`, thus guaranteeing its
        // validity.
        data: ptr,
    }
}
