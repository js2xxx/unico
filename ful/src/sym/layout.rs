use core::alloc::Layout;

const fn max(a: usize, b: usize) -> usize {
    if a > b { a } else { b }
}

pub const fn extend(layout: Layout, next: Layout) -> Option<(Layout, usize)> {
    let new_align = max(layout.align(), next.align());
    let pad = layout.padding_needed_for(next.align());

    let offset = ct!(layout.size().checked_add(pad));
    let new_size = ct!(offset.checked_add(next.size()));

    // The safe constructor is called here to enforce the isize size limit.
    let layout = match Layout::from_size_align(new_size, new_align) {
        Ok(layout) => layout,
        Err(_) => return None,
    };
    Some((layout, offset))
}
