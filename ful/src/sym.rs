mod layout;
mod raw;

#[cfg(any(feature = "unwind", feature = "std"))]
use alloc::boxed::Box;
#[cfg(any(feature = "unwind", feature = "std"))]
use core::any::Any;
use core::{
    cell::Cell,
    mem::{self, ManuallyDrop},
    ptr::{self, NonNull},
};

use unico_context as cx;
use unico_context::Transfer;
use unico_stack::{Global, Stack};

use self::raw::TransferData;
pub use self::raw::{AbortHook, PanicHook};
use crate::{Build, BuildUnchecked, Builder, NewError};

#[thread_local]
static CUR: Cell<*mut ()> = Cell::new(ptr::null_mut());

/// A continuation of the current control flow.
///
/// This structure represents a continuation, a.k.a. the handle of a symmetric
/// coroutine.
///
/// Note that if neither `unwind` nor `std` feature is enabled and the coroutine
/// is dropped while not resumed to end, all the data on the call stack will be
/// ***LEAKED***. It's because the dropping process requires unwinding, and thus
/// a `Box<dyn Any + Send>`.
#[derive(Debug)]
pub struct Co {
    cx: NonNull<()>,
    raw: *mut Stack,
}

// SAFETY: The bounds of the actual function will be checked in the builder.
unsafe impl Send for Co {}
unsafe impl Sync for Co {}

impl Co {
    fn from_inner(cx: NonNull<()>, raw: *mut Stack) -> Self {
        Co { cx, raw }
    }

    fn into_inner(this: Self) -> (NonNull<()>, *mut Stack) {
        let cx = this.cx;
        let raw = this.raw;
        mem::forget(this);
        (cx, raw)
    }
}

impl Co {
    /// Initiate a builder for a coroutine with some defaults.
    pub const fn builder() -> Builder<&'static Global, AbortHook> {
        Builder::new()
    }
}

impl<F, S, P> Build<F, S, P> for Co
where
    F: FnOnce(Option<Co>) -> Co + Send + 'static,
    S: Into<Stack>,
    P: PanicHook,
{
    fn build(builder: Builder<S, P>, arg: F) -> Result<Self, Self::Error> {
        // SAFETY: The function is `Send` and `'static`.
        unsafe { Self::build_unchecked(builder, arg) }
    }
}

impl<F, S, P> BuildUnchecked<F, S, P> for Co
where
    F: FnOnce(Option<Co>) -> Co,
    S: Into<Stack>,
    P: PanicHook,
{
    type Error = NewError;

    /// # Safety
    ///
    /// - `arg` must be [`Send`], or the caller must not send the coroutine to
    ///   another thread.
    /// - `arg` must be `'static`, or the caller must ensure that the returned
    ///   [`Co`] not escape the lifetime of the function.
    unsafe fn build_unchecked(
        builder: Builder<S, P>,
        arg: F,
    ) -> Result<Self, Self::Error> {
        let Builder { stack, panic_hook } = builder;
        raw::RawCo::new_on(stack.into(), panic_hook, arg)
    }
}

impl Co {
    /// Move the current control flow to this continuation.
    ///
    /// This method moves the current control flow to this continuation,
    /// alongside with this object's ownership. Note that the return value may
    /// not be the same [`Co`] as the callee because this method is symmetric.
    pub fn resume(self) -> Option<Self> {
        // SAFETY: The payload pointers are unspecified and unused.
        unsafe { self.resume_payloaded(ptr::null_mut()).0 }
    }

    /// Similar to [`Co::resume`], but maps the source of this continuation,
    /// a.k.a. the caller of this method, to another one if possible.
    ///
    /// This method resumes the continuation, and then executes `map` on the
    /// call stack of that continuation. The only argument of `map` is the
    /// current control flow calling this method.
    ///
    /// Note that `M` doesn't requires extra trait and lifetime bounds:
    ///
    /// - `M` need not to be [`Send`] because the function will be executed on
    ///   the same execution unit (such as the same thread in user space or the
    ///   same CPU core in kernel) right after the stack switch.
    /// - `M` need not to be `'static` because all the instances of [`Co`] are
    ///   `'static` by default or its lifetime should be managed by the caller;
    ///   since both the source (the current call stack) and the destination
    ///   (`self`) of the current control flow transfer are valid at the time,
    ///   the function will be called (consumed) before the transfer completes,
    ///   and thus unable to escape its own lifetime.
    pub fn resume_with<M: FnOnce(Self) -> Option<Self>>(self, map: M) -> Option<Self> {
        let map = move |co| (map(co), ptr::null_mut());
        // SAFETY: The payload pointers are unspecified and unused.
        unsafe { self.resume_payloaded_with(map).0 }
    }

    /// Similar to [`Co::resume`], but with a pointer payload.
    ///
    /// # Safety
    ///
    /// The validity of returned pointer is not guaranteed whether `payload` is
    /// valid. The caller must maintains this manually, usually by calling this
    /// function in pairs.
    pub unsafe fn resume_payloaded(self, payload: *mut ()) -> (Option<Self>, *mut ()) {
        let (cx, raw) = Co::into_inner(self);
        let mut data = TransferData {
            raw: CUR.replace(raw.cast()).cast(),
            payload,
        };
        // SAFETY: `cx`'s lifetime is bound to its own coroutine, and it is ALWAYS
        // THE UNIQUE REFERENCE to the runtime stack. The proof is divided into 2
        // points:
        //
        // 1. If the coroutines are assymetrically nested, like B in A->B->C, in both A
        //    and C's cx the reference(s) of B seem to coexist. However, the statement
        //    is that they never coexist at all:
        //
        //    1) Before the resumption from A to B, the reference of B in C is never
        //       instantiated;
        //    2) Between the resumption from A to B and the one from B to C, the actual
        //       reference is implicitly with B's call stack, unreachable in both A and
        //       C;
        //    3) After the resumption from B to C, the reference of B in C is
        //       instantiated, while the one in A is dropped just as the code below.
        //
        //    The returning procedure performs like the inverse of the procedure above,
        // thus proving the statement.
        //
        // 2. If the coroutines are symmetrically transferred, and someone attempted to
        //    resume B from both A and C in 2 executing units (like 2 CPU cores), they
        //    simply cannot reach this evil goal:
        //
        //    1) The only method to create the pseudo-double-reference-of-B scenario is
        //       to perform the nested creation, i.e. the example in point 1;
        //    2) To execute C in another executing unit, the user must move the
        //       ownership of C to another call stack;
        //    3) However, once C is actually executed in another unit, its main
        //       function's argument will be the reference of that call stack, instead
        //       of B's.
        //
        //    Thus, though the naming of variables will be a bit rough, the statement
        // actually proves to be true.
        let Transfer { context, data } = unsafe { cx::resume(cx, data.as_mut()) };
        // SAFETY: The pointer is always valid according to all the calling of
        // `cx::resume`, `cx::resume_with` except the initial and final ones.
        let data = unsafe { TransferData::read(data) };

        (context.map(|cx| Co::from_inner(cx, data.raw)), data.payload)
    }

    /// Similar to [`Co::resume_with`], but with a possibly-returned pointer
    /// payload.
    ///
    /// # Safety
    ///
    /// The validity of returned pointer is not guaranteed whether `payload` is
    /// valid. The caller must maintains this manually, usually by calling this
    /// function in pairs.
    pub unsafe fn resume_payloaded_with<M: FnOnce(Self) -> (Option<Self>, *mut ())>(
        self,
        map: M,
    ) -> (Option<Self>, *mut ()) {
        let (cx, raw) = Co::into_inner(self);

        let mut data = ManuallyDrop::new(map);
        let ptr = (&mut data as *mut ManuallyDrop<M>).cast();

        let mut data = TransferData {
            raw: CUR.replace(raw.cast()).cast(),
            payload: ptr,
        };

        // SAFETY: The proof is the same as the one in `Co::resume_payloaded`.
        let Transfer { context, data } =
            unsafe { cx::resume_with(cx, data.as_mut(), raw::map::<M>) };
        // SAFETY: The pointer is always valid according to all the calling of
        // `cx::resume`, `cx::resume_with` except the initial and final ones. See
        // `raw::map` for more information.
        let data = unsafe { TransferData::read(data) };

        (context.map(|cx| Co::from_inner(cx, data.raw)), data.payload)
    }

    /// Resume the unwinding for this coroutine's partial destruction process.
    /// Use it in the process of error handling for every `catch_unwind`.
    ///
    /// Note that if you don't use it according to the text above, then the
    /// destruction process will be aborted and the caller of that process
    /// will not be returned and thus unreachable forever!
    ///
    /// This function is required due to the flaw in the unwinding
    /// implementation of Rust standard library which immediately aborts
    /// when encountering a foreign exception class code instead of
    /// continuing the process.
    #[cfg(any(feature = "unwind", feature = "std"))]
    pub fn resume_unwind(&self, payload: Box<dyn Any + Send>) -> Box<dyn Any + Send> {
        raw::resume_unwind(self, payload)
    }
}

impl Drop for Co {
    fn drop(&mut self) {
        #[allow(unused_variables)]
        // SAFETY： We don't use `self.cx` and `self.raw` any longer after taking out data
        // from these fields. The safety proof of `cx::resume_with` is the same as the
        // one in `Co::resume_payloaded`.
        unsafe {
            let cx = self.cx;
            let raw = self.raw;
            if !raw.is_null() {
                #[cfg(any(feature = "unwind", feature = "std"))]
                {
                    let old = CUR.replace(raw.cast());
                    cx::resume_with(cx, old, raw::unwind);
                }
                #[cfg(not(any(feature = "unwind", feature = "std")))]
                unsafe {
                    ptr::drop_in_place(raw)
                };
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use core::{convert::identity, ptr};
    use std::{alloc::Global, string::String};

    use unico_context::{boost::Boost, global_resumer};
    use unico_stack::global_stack_allocator;

    use crate::{callcc, spawn, spawn_unchecked, sym::CUR};

    global_stack_allocator!(Global);
    global_resumer!(Boost);

    #[test]
    fn creation() {
        spawn(Option::unwrap);
        assert_eq!(CUR.get(), ptr::null_mut());
    }

    #[test]
    fn empty() {
        assert!(callcc(identity).is_none());
        assert_eq!(CUR.get(), ptr::null_mut());
    }

    #[test]
    fn panicked() {
        std::println!("0");
        callcc(|co| {
            let ret = super::Co::builder()
                .hook_panic_with(move |_| {
                    std::println!("3");
                    co
                })
                .spawn(|_| panic!("2"));
            std::println!("1");
            ret.unwrap()
        });
        std::println!("4");
        assert_eq!(CUR.get(), ptr::null_mut());
    }

    #[test]
    fn capture_move() {
        let s = String::from("hello");
        let ret = callcc(move |co| {
            assert_eq!(s, "hello");
            co
        });
        assert!(ret.is_none());
        assert_eq!(CUR.get(), ptr::null_mut());
    }

    #[test]
    fn capture_ref() {
        let mut counter = 0;
        let mut co = unsafe {
            spawn_unchecked(|mut co| {
                for _ in 0..10 {
                    counter += 1;
                    co = co.unwrap().resume();
                }
                co.unwrap()
            })
        };
        loop {
            co = match co.resume() {
                Some(co) => co,
                None => break,
            }
        }
        assert_eq!(counter, 10);
        assert_eq!(CUR.get(), ptr::null_mut());
    }

    #[test]
    fn symmetric() {
        let b = spawn(|a| {
            let c = spawn(move |b| {
                std::println!("3");
                let ret = b.unwrap().resume();
                std::println!("5");
                assert!(ret.is_none());
                a.unwrap()
            });
            std::println!("2");
            let ret = c.resume().unwrap();
            std::println!("4");
            ret
        });
        std::println!("1");
        let ret = b.resume();
        std::println!("6");
        assert!(ret.is_none());
        assert_eq!(CUR.get(), ptr::null_mut());
    }

    #[test]
    fn symmetric_direct() {
        assert!(callcc(|a| spawn(move |_| a)).is_none());
        assert_eq!(CUR.get(), ptr::null_mut());
    }
}
