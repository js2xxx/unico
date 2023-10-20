use core::{
    cell::Cell,
    mem,
    ptr::{self, NonNull},
};
use std::{boxed::Box, io::Error as IoError};

use libc::ucontext_t;

use crate::{stack_top, Entry, Yield};

type Transfer = crate::Transfer<Ucx>;

std::thread_local! {
    static RECORD: Cell<NonNull<Record>> = {
        Cell::new(Record::new_root())
    };
}

#[derive(Debug)]
#[repr(transparent)]
pub struct Ucx(NonNull<Record>);

struct Record {
    ucx: ucontext_t,
    data: *mut (),
}

impl Record {
    fn new_root() -> NonNull<Self> {
        let mut ret = Box::<Self>::new_uninit();
        let status =
            unsafe { libc::getcontext(ptr::addr_of_mut!((*ret.as_mut_ptr()).ucx)) };
        assert_eq!(
            status,
            0,
            "Failed to construct top level context: {:?}",
            IoError::last_os_error()
        );
        unsafe { ptr::addr_of_mut!((*ret.as_mut_ptr()).data).write(ptr::null_mut()) };
        unsafe { NonNull::from(Box::leak(ret.assume_init())) }
    }

    unsafe fn new_on(
        stack: NonNull<[u8]>,
        entry: Entry<Ucx>,
    ) -> Result<NonNull<Self>, NewError> {
        extern "C" fn wrapper(entry: Entry<Ucx>) {
            let current = RECORD.with(|r| r.get());
            entry(Transfer {
                context: Ucx(current),
                data: unsafe { ptr::addr_of_mut!((*current.as_ptr()).data).read() },
            });
        }

        let pointer: NonNull<Self> = stack_top(stack).ok_or(NewError::StackTooSmall)?;

        let ucx = ptr::addr_of_mut!((*pointer.as_ptr()).ucx);

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

        Ok(pointer)
    }

    unsafe fn resume(t: Transfer) -> Transfer {
        let Transfer {
            context: Ucx(target),
            data,
        } = t;
        let old = RECORD.with(|r| r.replace(target));
        ptr::addr_of_mut!((*target.as_ptr()).data).write(data);

        let status = libc::swapcontext(
            ptr::addr_of_mut!((*old.as_ptr()).ucx),
            ptr::addr_of_mut!((*target.as_ptr()).ucx),
        );
        assert_eq!(
            status,
            0,
            "failed to swap context: {:?}",
            IoError::last_os_error()
        );

        let current = RECORD.with(|r| r.get());
        Transfer {
            context: Ucx(current),
            data: ptr::addr_of_mut!((*current.as_ptr()).data).read(),
        }
    }
}

#[derive(Debug, Copy, Clone, Default)]
pub struct Ucontext;

#[derive(Debug)]
pub enum NewError {
    StackTooSmall,
    GetContext(IoError),
}

unsafe impl Yield for Ucontext {
    type Context = Ucx;

    type NewError = NewError;

    unsafe fn new_on(
        &self,
        stack: NonNull<[u8]>,
        entry: Entry<Ucx>,
    ) -> Result<Self::Context, Self::NewError> {
        Record::new_on(stack, entry).map(Ucx)
    }

    unsafe fn resume(&self, t: Transfer) -> Transfer {
        Record::resume(t)
    }
}
