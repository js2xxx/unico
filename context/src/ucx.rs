use core::{
    cell::Cell,
    mem,
    ptr::{self, NonNull},
};
use std::{boxed::Box, io::Error as IoError};

use libc::ucontext_t;

use crate::{stack_top, Entry, Map, Resume};

type Transfer = crate::Transfer<Ucx>;

std::thread_local! {
    static TRANSFER: Cell<LocalTransfer> = {
        let root = Ucx::new_root();
        Cell::new(LocalTransfer {
            from: None,
            ucx: root.0,
            on_top: None,
            data: ptr::null_mut(),
        })
    };
}

#[derive(Debug, Clone, Copy)]
struct LocalTransfer {
    from: Option<NonNull<ucontext_t>>,
    ucx: NonNull<ucontext_t>,
    on_top: Option<Map<Ucx>>,
    data: *mut (),
}

/// The wrapper type of a pointer to `ucontext_t` with some additional data.
#[derive(Debug)]
#[repr(transparent)]
pub struct Ucx(NonNull<ucontext_t>);

impl Ucx {
    fn new_root() -> Self {
        let mut ret = Box::new_uninit();
        // SAFETY: `ret` contains a valid block of memory, waiting to be initialized.
        let status = unsafe { libc::getcontext(ret.as_mut_ptr()) };
        assert_eq!(
            status,
            0,
            "Failed to construct top level context: {:?}",
            IoError::last_os_error()
        );
        // SAFETY: `ret` is initialized by `libc::getcontext`.
        Ucx(NonNull::from(Box::leak(unsafe { ret.assume_init() })))
    }

    /// # Safety
    ///
    /// See [`Resume::new_on`] for more information.
    unsafe fn new_on(stack: NonNull<[u8]>, entry: Entry<Ucx>) -> Result<Self, NewError> {
        #[allow(improper_ctypes_definitions)]
        unsafe extern "C" fn wrapper(entry: Entry<Ucx>) {
            let t = TRANSFER.get();
            entry(Ucx(t.from.unwrap()), t.data);
        }

        let pointer: NonNull<ucontext_t> =
            stack_top(stack).ok_or(NewError::StackTooSmall)?;
        let ucx = pointer.as_ptr();

        // SAFETY: `ucx` is proper aligned and points to a valid block of uninitialized
        // memory.
        let status = unsafe { libc::getcontext(ucx) };
        if status != 0 {
            return Err(NewError::GetContext(IoError::last_os_error()));
        }

        // SAFETY: `ucx` is initialized by `libc::getcontext`; `pointer` is greater than
        // `stack`'s base pointer by address and points to the same block of memory.
        unsafe {
            let ucx = &mut *ucx;
            ucx.uc_stack.ss_sp = stack.as_ptr().cast();
            ucx.uc_stack.ss_size =
                pointer.as_ptr().byte_offset_from(ucx.uc_stack.ss_sp) as _;
            ucx.uc_link = ptr::null_mut();
        }

        // SAFETY: `ucx` is initialized by `libc::getcontext`; `wrapper` has exactly 1
        // parameter.
        unsafe {
            let wrapper = mem::transmute(wrapper as unsafe extern "C" fn(_));
            libc::makecontext(ucx, wrapper, 1, entry)
        };

        Ok(Ucx(pointer))
    }

    /// # Safety
    ///
    /// See [`Resume::resume`] for more information.
    unsafe fn resume(context: Ucx, on_top: Option<Map<Ucx>>, data: *mut ()) -> Transfer {
        let target = context.0;
        let src = TRANSFER.get().ucx;
        TRANSFER.set(LocalTransfer {
            from: Some(src),
            ucx: target,
            on_top,
            data,
        });

        // SAFETY: Both pointers have their reference to a valid `ucontext_t`
        // respectively.
        let status = unsafe { libc::swapcontext(src.as_ptr(), target.as_ptr()) };
        assert_eq!(
            status,
            0,
            "failed to swap context: {:?}",
            IoError::last_os_error()
        );

        let t = TRANSFER.get();
        let ucx = t.from.unwrap();
        match t.on_top {
            Some(on_top) => on_top(Ucx(ucx), t.data),
            None => Transfer {
                context: Some(Ucx(ucx)),
                data: t.data,
            },
        }
    }
}

/// The [`Resume`] implementation with the POSIX library's [`makecontext`](https://man7.org/linux/man-pages/man3/makecontext.3.html) functionalities.
#[derive(Debug, Copy, Clone, Default)]
pub struct Ucontext;

#[derive(Debug)]
pub enum NewError {
    StackTooSmall,
    GetContext(IoError),
}

// SAFETY: The `ucontext_t` is created on the given stack. See `Ucx::new_on` for
// more information.
unsafe impl Resume for Ucontext {
    type Context = Ucx;

    type NewError = NewError;

    unsafe fn new_on(
        &self,
        stack: NonNull<[u8]>,
        entry: Entry<Ucx>,
    ) -> Result<Ucx, NewError> {
        Ucx::new_on(stack, entry)
    }

    unsafe fn resume(&self, cx: Ucx, data: *mut ()) -> Transfer {
        Ucx::resume(cx, None, data)
    }

    unsafe fn resume_with(
        &self,
        cx: Ucx,
        data: *mut (),
        map: Map<Ucx>,
    ) -> crate::Transfer<Ucx> {
        Ucx::resume(cx, Some(map), data)
    }
}
