use core::alloc::Layout;

const fn max(a: usize, b: usize) -> usize {
    if a > b {
        a
    } else {
        b
    }
}

pub const fn extend(layout: Layout, next: Layout) -> Option<(Layout, usize)> {
    let new_align = max(layout.align(), next.align());
    let pad = layout.padding_needed_for(next.align());

    let offset = match layout.size().checked_add(pad) {
        Some(offset) => offset,
        None => return None,
    };
    let new_size = match offset.checked_add(next.size()) {
        Some(new_size) => new_size,
        None => return None,
    };

    // The safe constructor is called here to enforce the isize size limit.
    let layout = unsafe { Layout::from_size_align_unchecked(new_size, new_align) };
    Some((layout, offset))
}
