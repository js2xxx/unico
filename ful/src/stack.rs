use core::{alloc::Layout, ptr::NonNull};

pub struct RawStack {
    pointer: NonNull<u8>,
    layout: Layout,
    drop: unsafe fn(NonNull<u8>, Layout),
}

impl RawStack {
    /// Creates a new [`RawStack`].
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
        RawStack {
            pointer,
            layout,
            drop,
        }
    }

    pub fn base(&self) -> NonNull<u8> {
        self.pointer
    }

    pub fn layout(&self) -> Layout {
        self.layout
    }
}

impl Drop for RawStack {
    fn drop(&mut self) {
        unsafe { (self.drop)(self.pointer, self.layout) }
    }
}
