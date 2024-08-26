use core::{
    cell::Cell,
    mem,
    ptr::{self, NonNull},
};
use std::{boxed::Box, io::Error as IoError};

use libc::ucontext_t;

use crate::{stack_top, Context, Entry};

type Transfer = crate::Transfer<Ucx>;

std::thread_local! {
    static TRANSFER: Cell<LocalTransfer> = {
        Cell::new(LocalTransfer {
            ucx: Ucx::new_root().0,
            data: ptr::null_mut(),
        })
    };
}

#[derive(Debug, Clone, Copy)]
struct LocalTransfer {
    ucx: NonNull<ucontext_t>,
    data: *mut (),
}

impl From<LocalTransfer> for Transfer {
    fn from(value: LocalTransfer) -> Self {
        Self {
            context: Ucx(value.ucx),
            data: value.data,
        }
    }
}

impl From<Transfer> for LocalTransfer {
    fn from(value: Transfer) -> Self {
        Self {
            ucx: value.context.0,
            data: value.data,
        }
    }
}

/// The POSIX library's [`makecontext`](https://man7.org/linux/man-pages/man3/makecontext.3.html) functions.
#[derive(Debug)]
#[repr(transparent)]
pub struct Ucx(NonNull<ucontext_t>);

impl Ucx {
    fn new_root() -> Self {
        let mut ret = Box::new_uninit();
        let status = unsafe { libc::getcontext(ret.as_mut_ptr()) };
        assert_eq!(
            status,
            0,
            "Failed to construct top level context: {:?}",
            IoError::last_os_error()
        );
        Ucx(unsafe { NonNull::from(Box::leak(ret.assume_init())) })
    }

    unsafe fn new_on(stack: NonNull<[u8]>, entry: Entry<Ucx>) -> Result<Self, NewError> {
        extern "C" fn wrapper(entry: Entry<Ucx>) {
            entry(TRANSFER.get().into());
        }

        let pointer: NonNull<ucontext_t> =
            stack_top(stack).ok_or(NewError::StackTooSmall)?;
        let ucx = pointer.as_ptr();

        let status = libc::getcontext(ucx);
        if status != 0 {
            return Err(NewError::GetContext(IoError::last_os_error()));
        }

        unsafe {
            let ucx = &mut *ucx;
            ucx.uc_stack.ss_sp = stack.as_ptr().cast();
            ucx.uc_stack.ss_size =
                pointer.as_ptr().byte_offset_from(ucx.uc_stack.ss_sp) as _;
            ucx.uc_link = ptr::null_mut();
        }

        libc::makecontext(ucx, mem::transmute(wrapper as extern "C" fn(_)), 1, entry);

        Ok(Ucx(pointer))
    }

    unsafe fn resume(t: Transfer) -> Transfer {
        let target = t.context.0;
        let old = TRANSFER.replace(t.into());

        let status = libc::swapcontext(old.ucx.as_ptr(), target.as_ptr());
        assert_eq!(
            status,
            0,
            "failed to swap context: {:?}",
            IoError::last_os_error()
        );

        TRANSFER.get().into()
    }
}

#[derive(Debug, Copy, Clone, Default)]
pub struct Ucontext;

#[derive(Debug)]
pub enum NewError {
    StackTooSmall,
    GetContext(IoError),
}

unsafe impl Context for Ucontext {
    type Context = Ucx;

    type NewError = NewError;

    unsafe fn new_on(
        &self,
        stack: NonNull<[u8]>,
        entry: Entry<Ucx>,
    ) -> Result<Ucx, NewError> {
        Ucx::new_on(stack, entry)
    }

    unsafe fn resume(&self, t: Transfer) -> Transfer {
        Ucx::resume(t)
    }
}
