#[cfg(any(feature = "unwind", feature = "std"))]
use alloc::boxed::Box;
#[cfg(any(feature = "unwind", feature = "std"))]
use core::{any::Any, panic::AssertUnwindSafe};
use core::{
    marker::PhantomData,
    mem::MaybeUninit,
    ops::{Coroutine, CoroutineState},
    panic::UnwindSafe,
    pin::Pin,
    ptr,
};

use unico_context::{boost::Boost, Resume};
use unico_stack::{Global, Stack};

#[cfg(any(feature = "unwind", feature = "std"))]
use crate::unwind::*;
use crate::{
    sym::{AbortHook, Co, PanicHook},
    Build, BuildUnchecked, Builder, NewError, Sealed,
};

enum Payload<Y> {
    Yielded(Y),
    // We must eliminate `C`'s type signature so as to allow pointers or references of
    // `YieldHandle` to be stored in thread locals.
    Complete(*const ()),
    #[cfg(any(feature = "unwind", feature = "std"))]
    Panicked(Box<dyn Any + Send>),
}

/// A generator, a.k.a. an symmetric coroutine.
// Safety notice for the internal resuming order (N for `ptr::null_mut`):
//
//          caller                    generator
// -----------------------------------------------------------
//       0. callcc ----------------> start execution
//            * ````````````````````````` |
//            * ````````````````````````` |
//     N <- callcc <----------- 1. resume_with_payload(N)
//            | ````````````````````````` *
//            | ````````````````````````` *
//       2. resume(R) -----------> resume_with_payload() -> R
//            * ``````````````````` execute into func(R)
//            * ````````````````````````` |
//            * ````````````````````````` |
//     Y <- resume <--------------- 3. yield_(Y)
//            | ````````````````````````` *
//            | ````````````````````````` *
//       4. resume(R) ---------------> yield_() -> R
//            * ````````````````````````` |
//           ... ```````` ... `````````` ...
//            * ````````````````````````` |
//            * ``````````````````` 4. func() -> C
//          resume <--------------- resume_with_payload(C)
//     store C in local ````````````````` *
//            | ````````````````````````` *
//            | ````````````````````````` *
//       5. resume ---------------> resume_with_payload() -> N
//            * ````````````````````````` |
//            * ````````````````````````` |
//     C <- resume <--------------- end execution
pub struct Gn<'a, C, Y = (), R = (), Z: Resume = Boost> {
    inner: Option<Co<Z>>,
    marker: PhantomGn<'a, C, Y, R>,
}
type PhantomGn<'a, C, Y, R> =
    PhantomData<dyn FnOnce(R) -> (Y, C) + Unpin + Send + UnwindSafe + 'a>;

/// The yield handle that the [coroutine](Gn) holds, used for yielding back to
/// its parent call stack.
///
/// This type does not require all the generics that `Gn` requires.
///
/// 1. `'a` is not required because the coroutine can only obtain it's local
///    reference in the main procedure, thus prevents it from escaping from it.
/// 2. `C` is not required because the main procedure returns a value of that
///    type, which has nothing to do with its yield handle.
pub struct YieldHandle<Y = (), R = (), Z: Resume = Boost> {
    inner: Option<Co<Z>>,
    marker: PhantomYieldHandle<Y, R>,
}
type PhantomYieldHandle<Y, R> =
    PhantomData<dyn FnOnce(Y) -> R + Unpin + Send + UnwindSafe>;

impl Gn<'static, ()> {
    pub fn builder() -> Builder<Boost, &'static Global, AbortHook> {
        Builder::new()
    }
}

impl<'a, C, Y, R, Z: Resume> Sealed for Gn<'a, C, Y, R, Z> {}

impl<'a, F, C, Y, R, Z, S, P> Build<F, Z, S, P> for Gn<'a, C, Y, R, Z>
where
    F: FnOnce(&mut YieldHandle<Y, R, Z>, R) -> C + Send + 'a,
    Z: Resume,
    S: Into<Stack>,
    P: PanicHook<Z>,
{
    fn build(builder: Builder<Z, S, P>, arg: F) -> Result<Self, Self::Error> {
        // SAFETY: `arg` is `Send` and `'a`.
        unsafe { Self::build_unchecked(builder, arg) }
    }
}

impl<'a, F, C, Y, R, Z, S, P> BuildUnchecked<F, Z, S, P> for Gn<'a, C, Y, R, Z>
where
    F: FnOnce(&mut YieldHandle<Y, R, Z>, R) -> C,
    Z: Resume,
    S: Into<Stack>,
    P: PanicHook<Z>,
{
    type Error = NewError<Z>;

    /// # Safety
    ///
    /// - `func` must be [`Send`], or the caller must not send the coroutine to
    ///   another thread.
    /// - `func` must be at least `'a`.
    unsafe fn build_unchecked(
        builder: Builder<Z, S, P>,
        func: F,
    ) -> Result<Self, Self::Error> {
        // SAFETY: See the type's safety notice.
        let wrapper = move |co: Co<Z>| {
            // SAFETY: See step 1 of the type's safety notice.
            let res = unsafe { co.resume_with_payload(ptr::null_mut()) };
            let (co, payload) = res.unwrap();

            // SAFETY: See step 2 of the type's safety notice.
            let initial = unsafe { payload.cast::<R>().read() };
            let mut handle = YieldHandle {
                inner: Some(co),
                marker: PhantomData,
            };
            let c;
            #[cfg(any(feature = "unwind", feature = "std"))]
            let y = match catch_unwind(AssertUnwindSafe(|| func(&mut handle, initial))) {
                Ok(complete) => {
                    c = complete;
                    Payload::<Y>::Complete((&c as *const C).cast())
                }
                Err(payload) => {
                    let payload = handle.inner.as_ref().unwrap().resume_unwind(payload);
                    Payload::Panicked(payload)
                }
            };
            #[cfg(not(any(feature = "unwind", feature = "std")))]
            let y = {
                c = func(&mut handle, initial);
                Payload::<Y>::Complete((&c as *const C).cast())
            };

            let mut y = MaybeUninit::new(y);
            let co = handle.inner.take().unwrap();

            // SAFETY: See step 4 of the type's safety notice.
            let res = unsafe { co.resume_with_payload(y.as_mut_ptr().cast()) };
            res.unwrap().0
        };

        // SAFETY: We here constrain the function to be the same lifetime as the
        // generator itself, and the yield handle cannot escape the function as well.
        // Besides, `func` is `Send`. Also see step 0 of the type's safety notice.
        Ok(Gn {
            inner: unsafe { builder.callcc_unchecked(wrapper) }?,
            marker: PhantomData,
        })
    }
}

impl<'a, C, Y, R, Z: Resume> Gn<'a, C, Y, R, Z> {
    pub fn resume(&mut self, resumed: R) -> CoroutineState<Y, C> {
        let mut m = MaybeUninit::new(resumed);
        let co = self
            .inner
            .take()
            .expect("coroutine resumed after completion");

        // SAFETY: See step 2 and 4 of the type's safety notice.
        let res = unsafe { co.resume_with_payload(m.as_mut_ptr().cast()) };
        let (co, payload) = res.unwrap();

        // SAFETY: See step 3 of the type's safety notice.
        match unsafe { payload.cast::<Payload<Y>>().read() } {
            Payload::Yielded(yielded) => {
                self.inner = Some(co);
                CoroutineState::Yielded(yielded)
            }
            Payload::Complete(complete) => {
                let res = co.resume();
                debug_assert!(res.is_none());
                CoroutineState::Complete(unsafe { complete.cast::<C>().read() })
            }
            #[cfg(any(feature = "unwind", feature = "std"))]
            Payload::Panicked(payload) => resume_unwind(payload),
        }
    }
}

impl<C, Y, R, Z: Resume + Unpin> Coroutine<R> for Gn<'_, C, Y, R, Z> {
    type Yield = Y;
    type Return = C;

    fn resume(mut self: Pin<&mut Self>, arg: R) -> CoroutineState<Y, C> {
        (*self).resume(arg)
    }
}

impl<Y, R, Z: Resume> YieldHandle<Y, R, Z> {
    pub fn yield_(&mut self, yielded: Y) -> R {
        let mut y = MaybeUninit::new(Payload::Yielded(yielded));

        let co = self.inner.take().unwrap();
        // SAFETY: See step 3 of the type's safety notice.
        let res = unsafe { co.resume_with_payload(y.as_mut_ptr().cast()) };
        let (co, payload) = res.unwrap();
        self.inner = Some(co);

        // SAFETY: See step 4 of the type's safety notice.
        unsafe { payload.cast::<R>().read() }
    }
}

#[cfg(all(test, feature = "global-stack-allocator"))]
mod tests {
    use core::ops::CoroutineState;

    use crate::gen;

    #[test]
    fn basic() {
        let mut gn = gen(|y, mut r| {
            for i in 0..100 {
                assert_eq!(r, i);
                r = y.yield_(i);
            }
            assert_eq!(r, 1024);
            1024
        });
        for i in 0..100 {
            assert!(matches!(gn.resume(i), CoroutineState::Yielded(x) if x == i));
        }
        assert!(matches!(gn.resume(1024), CoroutineState::Complete(1024)));
    }

    #[cfg(feature = "std")]
    #[test]
    fn panicked() {
        use core::{alloc::Layout, panic::AssertUnwindSafe};

        use crate::gen_on;

        let mut gn = gen_on::<_, _, _, (), _>(
            Layout::from_size_align(4096 * 6, 4096).unwrap(),
            |_, _| panic!("What the fuck?"),
        );
        let res = crate::unwind::catch_unwind(AssertUnwindSafe(|| gn.resume(())));
        assert!(res.is_err());
    }
}
