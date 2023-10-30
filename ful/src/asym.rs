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

use unico_stack::{Global, Stack};

#[cfg(any(feature = "unwind", feature = "std"))]
use crate::unwind::*;
use crate::{
    sym::{AbortHook, Co, PanicHook},
    Build, BuildUnchecked, Builder, NewError,
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
pub struct Gn<'a, C, Y = (), R = ()> {
    inner: Option<Co>,
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
pub struct YieldHandle<Y = (), R = ()> {
    inner: Option<Co>,
    marker: PhantomYieldHandle<Y, R>,
}
type PhantomYieldHandle<Y, R> =
    PhantomData<dyn FnOnce(Y) -> R + Unpin + Send + UnwindSafe>;

impl Gn<'static, ()> {
    pub fn builder() -> Builder<&'static Global, AbortHook> {
        Builder::new()
    }
}

impl<'a, F, C, Y, R, S, P> Build<F, S, P> for Gn<'a, C, Y, R>
where
    F: FnOnce(&mut YieldHandle<Y, R>, R) -> C + Send + 'a,
    S: Into<Stack>,
    P: PanicHook,
{
    fn build(builder: Builder<S, P>, arg: F) -> Result<Self, Self::Error> {
        // SAFETY: `arg` is `Send` and `'a`.
        unsafe { Self::build_unchecked(builder, arg) }
    }
}

impl<'a, F, C, Y, R, S, P> BuildUnchecked<F, S, P> for Gn<'a, C, Y, R>
where
    F: FnOnce(&mut YieldHandle<Y, R>, R) -> C,
    S: Into<Stack>,
    P: PanicHook,
{
    type Error = NewError;

    /// # Safety
    ///
    /// - `func` must be [`Send`], or the caller must not send the coroutine to
    ///   another thread.
    /// - `func` must be at least `'a`.
    unsafe fn build_unchecked(
        builder: Builder<S, P>,
        func: F,
    ) -> Result<Self, Self::Error> {
        // SAFETY: See the type's safety notice.
        let wrapper = move |co: Co| {
            // SAFETY: See step 1 of the type's safety notice.
            let (res, payload) = unsafe { co.resume_payloaded(ptr::null_mut()) };
            let co = res.unwrap();

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
            let (res, _) = unsafe { co.resume_payloaded(y.as_mut_ptr().cast()) };
            res.unwrap()
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

impl<'a, C, Y, R> Gn<'a, C, Y, R> {
    pub fn resume(&mut self, resumed: R) -> CoroutineState<Y, C> {
        let mut m = MaybeUninit::new(resumed);
        let co = self
            .inner
            .take()
            .expect("coroutine resumed after completion");

        // SAFETY: See step 2 and 4 of the type's safety notice.
        let (res, payload) = unsafe { co.resume_payloaded(m.as_mut_ptr().cast()) };
        let co = res.unwrap();

        // SAFETY: See step 3 of the type's safety notice.
        match unsafe { payload.cast::<Payload<Y>>().read() } {
            Payload::Yielded(yielded) => {
                self.inner = Some(co);
                CoroutineState::Yielded(yielded)
            }
            Payload::Complete(complete) => {
                let complete = unsafe { complete.cast::<C>().read() };
                let res = co.resume();
                debug_assert!(res.is_none());
                CoroutineState::Complete(complete)
            }
            #[cfg(any(feature = "unwind", feature = "std"))]
            Payload::Panicked(payload) => resume_unwind(payload),
        }
    }
}

impl<C, Y, R> Coroutine<R> for Gn<'_, C, Y, R> {
    type Yield = Y;
    type Return = C;

    fn resume(mut self: Pin<&mut Self>, arg: R) -> CoroutineState<Y, C> {
        (*self).resume(arg)
    }
}

impl<Y, R> YieldHandle<Y, R> {
    pub fn yield_(&mut self, yielded: Y) -> R {
        let mut y = MaybeUninit::new(Payload::Yielded(yielded));

        let co = self.inner.take().unwrap();
        // SAFETY: See step 3 of the type's safety notice.
        let (res, payload) = unsafe { co.resume_payloaded(y.as_mut_ptr().cast()) };
        self.inner = res;

        // SAFETY: See step 4 of the type's safety notice.
        unsafe { payload.cast::<R>().read() }
    }
}

#[cfg(test)]
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
    #[should_panic = "What the fuck?"]
    fn panicked() {
        gen::<_, _, (), _>(|_, _| panic!("What the fuck?")).resume(());
    }
}
