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
            ucx: root.pointer,
            on_top: root.on_top,
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

impl From<LocalTransfer> for Transfer {
    fn from(value: LocalTransfer) -> Self {
        Self {
            context: Some(Ucx {
                pointer: value.from.unwrap(),
                on_top: value.on_top,
            }),
            data: value.data,
        }
    }
}

#[derive(Debug)]
#[repr(C)]
pub struct Ucx {
    pointer: NonNull<ucontext_t>,
    on_top: Option<Map<Ucx>>,
}

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
        Ucx {
            pointer: NonNull::from(Box::leak(unsafe { ret.assume_init() })),
            on_top: None,
        }
    }

    unsafe fn new_on(stack: NonNull<[u8]>, entry: Entry<Ucx>) -> Result<Self, NewError> {
        #[allow(improper_ctypes_definitions)]
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

        Ok(Ucx {
            pointer,
            on_top: None,
        })
    }

    unsafe fn resume(t: Transfer) -> Transfer {
        let Transfer { context, data } = t;
        let context = context.unwrap();

        let target = context.pointer;
        let src = TRANSFER.get().ucx;
        TRANSFER.set(LocalTransfer {
            from: Some(src),
            ucx: context.pointer,
            on_top: context.on_top,
            data,
        });

        let status = libc::swapcontext(src.as_ptr(), target.as_ptr());
        assert_eq!(
            status,
            0,
            "failed to swap context: {:?}",
            IoError::last_os_error()
        );

        let mut t: Transfer = TRANSFER.get().into();
        match t.context.as_mut().and_then(|c| c.on_top.take()) {
            Some(on_top) => on_top(t),
            None => t,
        }
    }
}

/// The POSIX library's [`makecontext`](https://man7.org/linux/man-pages/man3/makecontext.3.html) series functions.
#[derive(Debug, Copy, Clone, Default)]
pub struct Ucontext;

#[derive(Debug)]
pub enum NewError {
    StackTooSmall,
    GetContext(IoError),
}

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

    unsafe fn resume(&self, t: Transfer) -> Transfer {
        Ucx::resume(t)
    }

    unsafe fn resume_with(
        &self,
        mut t: crate::Transfer<Ucx>,
        map: Map<Ucx>,
    ) -> crate::Transfer<Ucx> {
        if let Some(c) = t.context.as_mut() {
            c.on_top = Some(map);
        }
        Ucx::resume(t)
    }
}
