#![no_std]
#![feature(coroutine_trait)]

pub mod waker;
#[cfg(feature = "asym")]
pub mod asym;
#[cfg(feature = "sym")]
pub mod sym;

#[cfg(feature = "sym")]
extern crate alloc;
