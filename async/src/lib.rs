#![no_std]
#![feature(allocator_api)]
#![feature(coroutine_trait)]

#[cfg(feature = "asym")]
pub mod asym;
#[cfg(feature = "sym")]
pub mod sym;

#[cfg(feature = "sym")]
extern crate alloc;

#[cfg(test)]
extern crate std;
