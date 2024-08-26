#![no_std]
#![cfg_attr(feature = "ucx", feature(new_uninit))]
#![allow(internal_features)]
#![feature(allocator_api)]
#![feature(allow_internal_unstable)]
#![feature(slice_ptr_get)]
#![feature(strict_provenance)]

cfg_if::cfg_if! {
    if #[cfg(feature = "boost")] {
        pub mod boost;
    }
}
cfg_if::cfg_if! {
    if #[cfg(feature = "ucx")] {
        extern crate std;
        pub mod ucx;
    }
}
mod page;

use core::{
    alloc::{AllocError, Layout},
    fmt::Debug,
    ptr::NonNull,
};

/// The transfer structure between contexts.
#[derive(Debug)]
#[repr(C)]
pub struct Transfer<C> {
    /// The target context to be resumed.
    pub context: Option<NonNull<C>>,
    /// The data passed between contexts.
    ///
    /// If the user passes the transfer structure to [`Resume::resume`], the
    /// data will be transferred to the target context.
    ///
    /// If the user gets the transfer structure from [`Resume::resume`] or in
    /// the entry function, the data is received from its resumer.
    pub data: *mut (),
}

pub type Entry<C> = unsafe extern "C" fn(cx: NonNull<C>, data: *mut ()) -> !;
#[allow(improper_ctypes_definitions)]
pub type Map<C> = unsafe extern "C" fn(cx: NonNull<C>, data: *mut ()) -> Transfer<C>;

/// The generic symmetric context-switching trait.
///
/// # Safety
///
/// `Context` must have a proper lifetime bound to a specific stack returned by
/// [`Resume::new_on`].
pub unsafe trait Resume: Sized + Clone + 'static {
    /// The context reference bound to a specfic stack.
    type Context: Sized + 'static;

    /// The error type returned during creation of some context.
    type NewError: Debug;

    /// Creates a new context on top of some stack.
    ///
    /// # Safety
    ///
    /// `stack` must reference to a valid block of enough, fresh memory so that
    /// the context will be created on it.
    unsafe fn new_on(
        &self,
        stack: NonNull<[u8]>,
        entry: Entry<Self::Context>,
    ) -> Result<NonNull<Self::Context>, Self::NewError>;

    /// Yields the execution to the target context 'cx' with `data` passed to
    /// the destination.
    ///
    /// # Safety
    ///
    /// `cx` must be created from the current [`Resume::new_on`] and be bound to
    /// some valid stack, and `data` must be valid according to `entry`
    /// passed to [`Resume::new_on`].
    unsafe fn resume(
        &self,
        cx: NonNull<Self::Context>,
        data: *mut (),
    ) -> Transfer<Self::Context>;

    /// Yields the execution to the target context 'cx' with `data` passed to
    /// the destination, and executes a function on top of that stack.
    ///
    /// # Safety
    ///
    /// `cx` must be created from the current [`Resume::new_on`] and be bound to
    /// some valid stack, the return value of `map` must contains a possible
    /// valid context created from [`Resume::new_on`] wrapped in an option,
    /// and `data` must be valid according to `entry` passed to
    /// [`Resume::new_on`].
    unsafe fn resume_with(
        &self,
        cx: NonNull<Self::Context>,
        data: *mut (),
        map: Map<Self::Context>,
    ) -> Transfer<Self::Context>;
}

fn layout_union(l1: Layout, l2: Layout) -> Layout {
    let size = l1.size().max(l2.size());
    let align = l1.align().max(l2.align());
    Layout::from_size_align(size, align).unwrap()
}

fn stack_top<T>(stack: NonNull<[u8]>) -> Option<NonNull<T>> {
    let layout = Layout::new::<T>();

    let ptr = stack.as_non_null_ptr();
    let addr = ptr.addr().get();
    if stack.len() < layout_union(layout, page::STACK_LAYOUT).size() {
        return None;
    }
    let ret = (addr + stack.len() - layout.size()) & !(layout.align() - 1);
    if ret < addr {
        return None;
    }
    ret.try_into().map(|addr| ptr.with_addr(addr).cast()).ok()
}

extern "Rust" {
    fn __rust_unico_context_new(
        stack: NonNull<u8>,
        stack_size: usize,
        entry: Entry<()>,
    ) -> Result<NonNull<()>, AllocError>;

    fn __rust_unico_context_resume(cx: NonNull<()>, data: *mut ()) -> Transfer<()>;

    fn __rust_unico_context_resume_with(
        cx: NonNull<()>,
        data: *mut (),
        map: Map<()>,
    ) -> Transfer<()>;
}

/// Creates a new context on top of some stack.
///
/// # Safety
///
/// `stack` must reference to a valid block of enough, fresh memory so that
/// the context will be created on it.
pub unsafe fn new_on(
    stack: NonNull<[u8]>,
    entry: Entry<()>,
) -> Result<NonNull<()>, AllocError> {
    __rust_unico_context_new(stack.as_non_null_ptr(), stack.len(), entry)
}

/// Yields the execution to the target context 'cx' with `data` passed to
/// the destination.
///
/// # Safety
///
/// `cx` must be created from [`new_on`] and be bound to some valid stack, and
/// `data` must be valid according to `entry` passed to [`new_on`].
pub unsafe fn resume(cx: NonNull<()>, data: *mut ()) -> Transfer<()> {
    __rust_unico_context_resume(cx, data)
}

/// Yields the execution to the target context 'cx' with `data` passed to
/// the destination, and executes a function on top of that stack.
///
/// # Safety
///
/// `cx` must be created from [`new_on`] and be bound to some valid stack, the
/// return value of `map` must contains a possible valid context created from
/// [`new_on`] wrapped in an option, and `data` must be valid according to
/// `entry` passed to [`new_on`].
pub unsafe fn resume_with(cx: NonNull<()>, data: *mut (), map: Map<()>) -> Transfer<()> {
    __rust_unico_context_resume_with(cx, data, map)
}

#[macro_export]
#[allow_internal_unstable(allocator_api)]
macro_rules! global_resumer {
    ($t:path) => {
        #[no_mangle]
        #[doc(hidden)]
        unsafe fn __rust_unico_context_new(
            stack: core::ptr::NonNull<u8>,
            stack_size: usize,
            entry: $crate::Entry<()>,
        ) -> Result<core::ptr::NonNull<()>, core::alloc::AllocError> {
            $crate::Resume::new_on(
                &$t,
                core::ptr::NonNull::slice_from_raw_parts(stack, stack_size),
                core::mem::transmute(entry),
            )
            .map(core::ptr::NonNull::cast)
            .map_err(|_| core::alloc::AllocError)
        }

        #[no_mangle]
        #[doc(hidden)]
        unsafe fn __rust_unico_context_resume(
            cx: core::ptr::NonNull<()>,
            data: *mut (),
        ) -> $crate::Transfer<()> {
            let t = $crate::Resume::resume(&$t, core::ptr::NonNull::cast(cx), data);
            core::mem::transmute(t)
        }

        #[no_mangle]
        #[doc(hidden)]
        unsafe fn __rust_unico_context_resume_with(
            cx: core::ptr::NonNull<()>,
            data: *mut (),
            map: $crate::Map<()>,
        ) -> $crate::Transfer<()> {
            core::mem::transmute($crate::Resume::resume_with(
                &$t,
                core::ptr::NonNull::cast(cx),
                data,
                core::mem::transmute(map),
            ))
        }
    };
}
