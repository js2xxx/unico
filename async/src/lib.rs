#![no_std]
#![deny(future_incompatible)]
#![deny(rust_2018_idioms)]
#![deny(rust_2024_compatibility)]
#![deny(trivial_casts)]
#![deny(trivial_numeric_casts)]
#![cfg_attr(test, feature(allocator_api))]
#![feature(coroutine_trait)]
#![feature(thread_local)]

#[cfg(feature = "asym")]
pub mod asym;
#[cfg(feature = "sym")]
pub mod sym;

#[cfg(feature = "sym")]
extern crate alloc;

#[cfg(any(test, feature = "std"))]
extern crate std;
