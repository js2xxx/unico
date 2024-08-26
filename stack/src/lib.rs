#![no_std]
#![deny(future_incompatible)]
#![deny(rust_2018_idioms)]
#![deny(rust_2024_compatibility)]
#![deny(trivial_casts)]
#![deny(trivial_numeric_casts)]
#![allow(internal_features)]
#![feature(allocator_api)]
#![feature(allow_internal_unstable)]
#![feature(slice_ptr_get)]
#![feature(strict_provenance)]
//! This module tackles with stacks.
//!
//! We have [a stack structure](Stack) that keep track of its own memory, and
//! a trait represents [a stack allocator](StackAllocator).

use core::{
    alloc::{AllocError, Allocator, Layout},
    mem::{self, MaybeUninit},
    ptr::NonNull,
};

// SAFETY: The alignment is a power of 2.
pub const DEFAULT_LAYOUT: Layout =
    unsafe { Layout::from_size_align_unchecked(4096 * 6, 4096) };

/// The raw stack structure.
///
/// The structure owns its memory, and drops the memory if it's dropped. The
/// user should not regard this as some sort of plain old data.
pub struct Stack {
    pointer: NonNull<u8>,
    layout: Layout,
    drop: unsafe fn(NonNull<u8>, Layout),
}

impl Stack {
    /// Creates a new [`Stack`].
    ///
    /// # Safety
    ///
    /// `pointer` must contains a valid block of memory of `layout`, and must be
    /// able to be dropped by `drop`.
    pub unsafe fn new(
        pointer: NonNull<u8>,
        layout: Layout,
        drop: unsafe fn(NonNull<u8>, Layout),
    ) -> Self {
        Stack {
            pointer,
            layout,
            drop,
        }
    }

    /// The base of the allocated stack.
    pub fn base(&self) -> NonNull<u8> {
        self.pointer
    }

    /// The usable layout of the allocated stack.
    pub fn layout(&self) -> Layout {
        self.layout
    }
}

impl Drop for Stack {
    fn drop(&mut self) {
        // SAFETY: See `Stack::new` for more information.
        unsafe { (self.drop)(self.pointer, self.layout) }
    }
}

impl Default for Stack {
    fn default() -> Self {
        Self::from(DEFAULT_LAYOUT)
    }
}

impl From<()> for Stack {
    fn from(_: ()) -> Self {
        Self::default()
    }
}

impl From<Layout> for Stack {
    fn from(layout: Layout) -> Self {
        Self::from((&Global, layout))
    }
}

impl<A: StackAllocator> From<&A> for Stack {
    fn from(alloc: &A) -> Self {
        Self::from((alloc, DEFAULT_LAYOUT))
    }
}

impl<A: StackAllocator> From<(&A, Layout)> for Stack {
    fn from((alloc, layout): (&A, Layout)) -> Self {
        match alloc.allocate(layout) {
            Ok(stack) => stack,
            Err(_) => panic!("failed to allocate with layout {layout:?}"),
        }
    }
}

/// Generic stack allocators.
///
/// # Safety
///
/// See [`Stack::new`] for more information. All the requirements of that
/// function should be satisfied by the implementor, ***EXCEPT*** that the
/// actual layout returned can be slightly different than the requested layout
/// with extra allocated space containing the dropper itself.
pub unsafe trait StackAllocator {
    /// Allocate a block of memory of `layout` as the returned stack alongside
    /// with its deallocator/dropper/destructor.
    fn allocate(&self, layout: Layout) -> Result<Stack, AllocError>;
}

// SAFETY: See the implementation below.
unsafe impl<T: Allocator + Clone> StackAllocator for T {
    fn allocate(&self, layout: Layout) -> Result<Stack, AllocError> {
        #[derive(Clone, Copy)]
        struct Layouts {
            allocated: Layout,
            returned: Layout,
        }

        #[inline]
        fn layouts<F>(layout: Layout, _: &F) -> Result<Layouts, AllocError> {
            let allocated = layout.pad_to_align();

            let returned_size = allocated
                .size()
                .checked_sub(mem::size_of::<F>())
                .ok_or(AllocError)?
                & !(mem::align_of::<F>() - 1);

            Ok(Layouts {
                allocated,
                returned: Layout::from_size_align(returned_size, layout.align()).unwrap(),
            })
        }

        #[inline]
        fn dropper_in<F>(memory: NonNull<u8>, returned: Layout) -> *mut F {
            memory
                .as_ptr()
                .map_addr(|addr| addr + returned.size())
                .cast::<F>()
        }

        #[inline]
        unsafe fn write_dropper<F>(
            memory: NonNull<u8>,
            layouts: &Layouts,
            drop: F,
        ) -> Stack
        where
            F: FnOnce(NonNull<u8>, Layout),
        {
            // SAFETY: The memory is just allocated with a valid size, and the pointer
            // is well-aligned.
            unsafe { dropper_in::<F>(memory, layouts.returned).write(drop) };
            // SAFETY: `memory` is allocated by us and will be dropped by `dropper`.
            unsafe { Stack::new(memory, layouts.returned, execute_dropper::<F>) }
        }

        unsafe fn execute_dropper<F>(memory: NonNull<u8>, returned: Layout)
        where
            F: FnOnce(NonNull<u8>, Layout),
        {
            let ptr = dropper_in::<F>(memory, returned);
            // SAFETY: We only call this dropper once and the memory is allocated by us.
            unsafe { (ptr.read())(memory, returned) }
        }

        // We use this trick to obtain the dropper's type information before its
        // construction, because the layout of `MaybeUninit<T>` is the same as `T` for
        // all `T`.
        let mut dropper = MaybeUninit::uninit();

        let layouts = layouts(layout, &dropper)?;

        let this = self.clone();
        let l = layouts;
        // Through this trick, we can even capture the closure's layout information in
        // the closure itself!
        dropper.write(move |ptr: NonNull<u8>, returned: Layout| {
            debug_assert_eq!(returned, l.returned);
            // SAFETY: the pointer and layout is unmodified after the allocation below.
            unsafe { this.deallocate(ptr, l.allocated) };
        });

        // SAFETY: We just write the variable with a valid closure.
        let dropper = unsafe { dropper.assume_init() };

        let memory = Allocator::allocate(&self, layouts.allocated)?.as_non_null_ptr();

        // The final process must be wrapped in a function to obtain the closure's type.
        //
        // SAFETY: See the implementation above.
        Ok(unsafe { write_dropper(memory, &layouts, dropper) })
    }
}

// SAFETY: This function is implemented by `global_stack_allocator!`.
unsafe extern "Rust" {
    /// # Safety
    ///
    /// See [`Stack::new`] for more information. All the requirements of that
    /// function should be satisfied by the implementor.
    fn __rust_unico_allocate_stack(layout: Layout) -> Result<Stack, AllocError>;
}

/// The global stack allocator interface (not implementation). The user should
/// define a unique instance of that implementation using
/// [`global_stack_allocator`].
#[derive(Debug, Clone, Copy, Default)]
pub struct Global;

unsafe impl StackAllocator for Global {
    fn allocate(&self, layout: Layout) -> Result<Stack, AllocError> {
        unsafe { __rust_unico_allocate_stack(layout) }
    }
}

/// Define a global stack allocator so that [`Global`] can be used in general.
///
/// This macro works just like `#[global_allocator]` attribute, except it only
/// receives the path of the target static variable, while the actual definition
/// can lie elsewhere.
#[allow_internal_unstable(allocator_api)]
#[macro_export]
macro_rules! global_stack_allocator {
    ($name:path) => {
        #[no_mangle]
        #[doc(hidden)]
        unsafe fn __rust_unico_allocate_stack(
            layout: core::alloc::Layout,
        ) -> Result<$crate::Stack, core::alloc::AllocError> {
            $crate::StackAllocator::allocate(&$name, layout)
        }
    };
}
