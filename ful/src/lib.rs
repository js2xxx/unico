#![no_std]
#![feature(alloc_layout_extra)]
#![feature(allocator_api)]
#![feature(const_alloc_layout)]
#![feature(coroutine_trait)]
#![feature(coroutines)]
#![feature(strict_provenance)]

macro_rules! ct {
    ($e:expr) => {
        match $e {
            Some(value) => value,
            None => return None,
        }
    };
}

pub mod asym;
pub mod sym;

#[cfg(feature = "unwind")]
extern crate alloc;

#[cfg(test)]
extern crate std;
