mod panicking;

#[cfg(any(feature = "unwind", feature = "std"))]
use core::panic::AssertUnwindSafe;
use core::{
    alloc::Layout,
    any::type_name,
    panic::UnwindSafe,
    ptr::{self, NonNull},
};

use unico_context::{self as cx, Transfer};

pub use self::panicking::*;
use super::{Co, NewError, Stack, layout::extend};
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
            None => unreachable!(
                "Layout evaluation failed for {} and {}",
                type_name::<F>(),
                type_name::<P>(),
            ),
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
        // SAFETY: The safety requirements is the same.
        unsafe { Self::new_on_imp(stack, panic_hook, func, Self::entry::<false>) }
            .map(Option::unwrap)
    }

    pub(crate) unsafe fn callcc_on(
        stack: Stack,
        panic_hook: P,
        func: F,
    ) -> Result<Option<Co>, NewError> {
        // SAFETY: The safety requirements is the same.
        unsafe { Self::new_on_imp(stack, panic_hook, func, Self::entry::<true>) }
    }

    /// # Safety
    ///
    /// - See `super::Builder::spawn_unchecked` for more information.
    pub(crate) unsafe fn new_on_imp(
        stack: Stack,
        panic_hook: P,
        func: F,
        entry: cx::Entry<()>,
    ) -> Result<Option<Co>, NewError> {
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
            let ptr = NonNull::slice_from_raw_parts(stack.base(), rest_size);
            cx::new_on(ptr, entry)
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
        // SAFETY: `context` is valid by contract.
        Ok(resume.context.map(|cx| unsafe { Co::from_inner(cx) }))
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
    unsafe extern "C" fn entry<const CALLCC: bool>(cx: NonNull<()>, ptr: *mut ()) -> ! {
        let task = Self::from_ptr(ptr);

        // SAFETY: The task is valid by contract.
        #[cfg(any(feature = "unwind", feature = "std"))]
        let hook = unsafe { task.panic_hook.read() };
        // SAFETY: The task is valid by contract. `func` must be read before the
        // initial resume, in case of early unwinding.
        let func = unsafe { task.func.read() };

        let run = || {
            func(if CALLCC {
                // SAFETY: `cx` is valid by contract.
                Some(unsafe { Co::from_inner(cx) })
            } else {
                // SAFETY: The proof is the same as the one in `Co::resume_payloaded`.
                let Transfer { context, .. } = unsafe { cx::resume(cx, ptr) };
                // SAFETY: `cx` is valid by contract.
                context.map(|cx| unsafe { Co::from_inner(cx) })
            })
        };

        #[cfg(any(feature = "unwind", feature = "std"))]
        let context = {
            // Move the hook in the braces to make sure it drops when the control flow
            // goes out of the scope.
            let rewind = |payload| AssertUnwindSafe(|| hook.rewind(payload));
            Co::into_inner('run: {
                // Run the main function and catches its panic (or unwound `HandleDrop`)
                // if possible.
                let payload = match unwind::catch_unwind(AssertUnwindSafe(run)) {
                    Ok(co) => break 'run co,
                    Err(payload) => match payload.downcast::<HandleDrop>() {
                        Ok(hd) => break 'run hd.next,
                        Err(payload) => payload,
                    },
                };
                // If the panic is caught, rewind it and catches the panic hook's own
                // possible unwound `HandleDrop`. If another unexpected panic is caught,
                // abort the current control flow.
                match unwind::catch_unwind(rewind(payload)) {
                    Ok(co) => co,
                    Err(payload) => match payload.downcast::<HandleDrop>() {
                        Ok(hd) => hd.next,
                        Err(payload) => AbortHook.rewind(payload),
                    },
                }
            })
        };
        #[cfg(not(any(feature = "unwind", feature = "std")))]
        let (context, next) = Co::into_inner(run());

        // SAFETY: The proof is the same as the one in `Co::resume_payloaded`.
        unsafe { cx::resume_with(context, ptr, Self::exit) };
        unreachable!("Exiting failed. There's at least some dangling `Co` instance!")
    }

    /// # Safety
    ///
    /// `ptr` must points to a valid `RawCo` calculated from `RawCo::from_ptr`.
    #[allow(improper_ctypes_definitions)]
    unsafe extern "C-unwind" fn exit(_: NonNull<()>, ptr: *mut ()) -> Transfer<()> {
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
/// `ptr` must offer a valid `M` in `TransferData`.
#[allow(improper_ctypes_definitions)]
pub(super) unsafe extern "C-unwind" fn map<M: FnOnce(Co) -> (Option<Co>, *mut ())>(
    cx: NonNull<()>,
    payload: *mut (),
) -> Transfer<()> {
    // SAFETY: The only reading is safe by contract.
    let func = unsafe { payload.cast::<M>().read() };
    // SAFETY: `cx` is valid by contract.
    let (ret, payload) = func(unsafe { Co::from_inner(cx) });
    Transfer {
        context: ret.map(Co::into_inner),
        data: payload,
    }
}

/// Enters a scope in the root control flow so that it can be dropped via its
/// corresponding [`Co`] object without panic.
///
/// Users may want to switch to other `Co` instances without returning back to
/// the root control flow, i.e. the callee of the `main` function. In such cases
/// without this function, dropping the root object will unwind its whole stack
/// without runtime support, which ends up in aborting the whole thread:
///
/// ```rust,should_panic
/// # #![feature(allocator_api)]
/// # unico_stack::global_stack_allocator!(std::alloc::Global);
/// # unico_context::global_resumer!(unico_context::boost::Boost);
///
/// unico_ful::callcc(|co| {
///     // This statement will abort the whole thread,
///     // which is not what we want.
///     drop(co);
///
///     // The infinite loop will be unreachable.
///     loop { println!("Spinning forever") }
/// });
/// ```
///
/// With this function, on the contrary, such ending won't happen, and all the
/// variables in this function will be safely dropped before the control flow
/// is transferred smoothly back to the caller of `drop`.
///
/// ```rust
/// # #![feature(allocator_api)]
/// # unico_stack::global_stack_allocator!(std::alloc::Global);
/// # unico_context::global_resumer!(unico_context::boost::Boost);
///
/// unsafe { unico_ful::sym::enter_root(|| {
///     unico_ful::callcc(|co| {
///         drop(co);
///         println!("Successful!");
///         std::process::exit(0);
///     });
/// }); }
/// ```
///
/// When unwinding is disabled, this function merely calls `f` and has no other
/// side effects.
///
/// # Safety
///
/// This function itself won't cause any undefined behavior when calling
/// multiple times. However, due to its special role in the whole play
/// (creating a scope and separating `Co` contexts), it can cause unexpected
/// behaviors easily. So it should only be called once in the root control
/// flow.
pub unsafe fn enter_root<R>(f: impl FnOnce() -> R + UnwindSafe) -> R {
    #[cfg(any(feature = "unwind", feature = "std"))]
    return match unwind::catch_unwind(f) {
        Ok(ret) => ret,
        Err(payload) => match payload.downcast::<HandleDrop>() {
            Ok(data) => {
                // SAFETY: The `cx` is valid while the root control flow should be aborted
                // immediately.
                unsafe { cx::resume(Co::into_inner(data.next), ptr::null_mut()) };
                unreachable!("Failed to drop the root `Co`")
            }
            Err(payload) => unwind::resume_unwind(payload),
        },
    };
    #[cfg(not(any(feature = "unwind", feature = "std")))]
    f()
}
