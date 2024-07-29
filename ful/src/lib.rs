#![no_std]
#![deny(future_incompatible)]
#![deny(rust_2018_idioms)]
// #![deny(rust_2024_compatibility)]
#![deny(trivial_casts)]
#![deny(trivial_numeric_casts)]
#![feature(alloc_layout_extra)]
#![feature(allocator_api)]
#![feature(const_alloc_layout)]
#![feature(coroutine_trait)]
#![feature(coroutines)]
#![feature(strict_provenance)]
#![feature(thread_local)]

macro_rules! ct {
    ($e:expr) => {
        match $e {
            Some(value) => value,
            None => return None,
        }
    };
}

pub mod asym;
mod builder;
pub mod sym;

use core::alloc::{AllocError, Layout};

pub use crate::builder::*;

#[cfg(any(feature = "unwind", feature = "std"))]
extern crate alloc;

#[cfg(any(test, feature = "std"))]
extern crate std;

#[derive(Debug)]
pub enum NewError {
    StackTooSmall { expected: Layout, actual: Layout },
    Context(AllocError),
}

#[cfg(all(not(feature = "std"), feature = "unwind"))]
mod unwind {
    use alloc::boxed::Box;
    use core::any::Any;

    pub use unwinding::panic::catch_unwind;

    pub fn resume_unwind(payload: Box<dyn Any + Send>) -> ! {
        unwinding::panic::begin_panic(payload);
        unreachable!("Unwind erroneously returned.")
    }
}
#[cfg(feature = "std")]
mod unwind {
    pub use std::panic::{catch_unwind, resume_unwind};
}
