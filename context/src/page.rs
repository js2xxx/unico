use core::{alloc::Layout, mem};

pub const SHIFT: u32 = 12;
pub const SIZE: usize = 1 << SHIFT;

// pub const LAYOUT: Layout = unsafe { Layout::from_size_align_unchecked(SIZE,
// SIZE) };
pub const STACK_LAYOUT: Layout =
    unsafe { Layout::from_size_align_unchecked(SIZE / 2, mem::align_of::<usize>()) };
