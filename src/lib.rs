//! **UNI**fied **CO**routines.

#![no_std]

pub use unico_context as context;
pub use unico_ful::{Build, BuildUnchecked, Builder, NewError};
pub use unico_stack as stack;
#[cfg(feature = "sym")]
pub mod sym {
    pub use unico_async::sym::*;
    pub use unico_ful::sym::*;
}
#[cfg(feature = "sym")]
pub use unico_ful::{
    callcc, callcc_unchecked, spawn, spawn_on, spawn_unchecked, spawn_unchecked_on,
};
#[cfg(feature = "asym")]
pub mod asym {
    pub use unico_async::asym::*;
    pub use unico_ful::asym::*;
}
#[cfg(feature = "asym")]
pub use unico_ful::{gen, gen_on};
