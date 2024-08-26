use core::{
    cell::Cell,
    mem,
    ptr::{self, NonNull},
};
use std::{boxed::Box, io::Error as IoError};

use libc::ucontext_t;

use crate::{stack_top, Entry, Map, Resume};

type Transfer = crate::Transfer<ucontext_t>;

std::thread_local! {
    static TRANSFER: Cell<LocalTransfer> = {
        Cell::new(LocalTransfer {
            from: None,
            ucx: new_root(),
            on_top: None,
            data: ptr::null_mut(),
        })
    };
}

#[derive(Debug, Clone, Copy)]
struct LocalTransfer {
    from: Option<NonNull<ucontext_t>>,
    ucx: NonNull<ucontext_t>,
    on_top: Option<Map<ucontext_t>>,
    data: *mut (),
}

/// The wrapper type of a pointer to `ucontext_t` with some additional data.

fn new_root() -> NonNull<ucontext_t> {
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
    NonNull::from(Box::leak(unsafe { ret.assume_init() }))
}

/// # Safety
///
/// See [`Resume::new_on`] for more information.
unsafe fn new_on(
    stack: NonNull<[u8]>,
    entry: Entry<ucontext_t>,
) -> Result<NonNull<ucontext_t>, NewError> {
    #[allow(improper_ctypes_definitions)]
    unsafe extern "C" fn wrapper(entry: Entry<ucontext_t>) {
        let t = TRANSFER.get();
        entry(t.from.unwrap(), t.data);
    }

    let pointer: NonNull<ucontext_t> = stack_top(stack).ok_or(NewError::StackTooSmall)?;
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
        ucx.uc_stack.ss_size = pointer.as_ptr().byte_offset_from(ucx.uc_stack.ss_sp) as _;
        ucx.uc_link = ptr::null_mut();
    }

    // SAFETY: `ucx` is initialized by `libc::getcontext`; `wrapper` has exactly 1
    // parameter.
    unsafe {
        let wrapper = mem::transmute(wrapper as unsafe extern "C" fn(_));
        libc::makecontext(ucx, wrapper, 1, entry)
    };

    Ok(pointer)
}

/// # Safety
///
/// See [`Resume::resume`] for more information.
unsafe fn resume_with(
    target: NonNull<ucontext_t>,
    on_top: Option<Map<ucontext_t>>,
    data: *mut (),
) -> Transfer {
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
        Some(on_top) => on_top(ucx, t.data),
        None => Transfer {
            context: Some(ucx),
            data: t.data,
        },
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

// SAFETY: The `ucontext_t` is created on the given stack. See `self::new_on`
// for more information.
unsafe impl Resume for Ucontext {
    type Context = ucontext_t;

    type NewError = NewError;

    unsafe fn new_on(
        &self,
        stack: NonNull<[u8]>,
        entry: Entry<ucontext_t>,
    ) -> Result<NonNull<ucontext_t>, NewError> {
        new_on(stack, entry)
    }

    unsafe fn resume(&self, cx: NonNull<ucontext_t>, data: *mut ()) -> Transfer {
        resume_with(cx, None, data)
    }

    unsafe fn resume_with(
        &self,
        cx: NonNull<ucontext_t>,
        data: *mut (),
        map: Map<ucontext_t>,
    ) -> crate::Transfer<ucontext_t> {
        resume_with(cx, Some(map), data)
    }
}
