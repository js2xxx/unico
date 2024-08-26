#![no_std]
#![feature(alloc_layout_extra)]
#![feature(allocator_api)]
#![feature(const_alloc_layout)]
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

pub use self::sym::Co;

#[cfg(feature = "unwind")]
extern crate alloc;

#[cfg(test)]
extern crate std;
