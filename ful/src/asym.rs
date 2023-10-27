use core::{
    marker::PhantomData,
    mem::MaybeUninit,
    ops::{Coroutine, CoroutineState},
    pin::Pin,
    ptr,
};

use unico_context::{boost::Boost, Resume};
use unico_stack::Global;

use crate::{
    sym::{AbortHook, Co, NewError},
    Builder,
};

enum Yield<Y, C> {
    Yielded(Y),
    Complete(C),
}

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
pub struct Gn<'a, Y, C, R = (), Z: Resume = Boost> {
    inner: Option<Co<Z>>,
    marker: PhantomGn<'a, Y, C, R>,
}
type PhantomGn<'a, Y, C, R> =
    PhantomData<dyn FnOnce(R) -> Yield<Y, C> + Unpin + Send + 'a>;

pub struct YieldHandle<'a, Y, C, R = (), Z: Resume = Boost> {
    inner: Option<Co<Z>>,
    marker: PhantomYieldHandle<'a, Y, C, R>,
}
type PhantomYieldHandle<'a, Y, C, R> =
    PhantomData<dyn FnOnce(Yield<Y, C>) -> R + Unpin + Send + 'a>;

impl<'a, Y, C, R, Z: Resume> Gn<'a, Y, C, R, Z> {
    /// # Safety
    ///
    /// The caller must observe the type's safety notice.
    pub(crate) unsafe fn wrapper(
        func: impl FnOnce(&mut YieldHandle<'a, Y, C, R, Z>, R) -> C + Send + 'a,
    ) -> impl FnOnce(Co<Z>) -> Co<Z> + 'a {
        move |co: Co<Z>| {
            // SAFETY: See step 1 of the type's safety notice.
            let res = unsafe { co.resume_with_payload(ptr::null_mut()) };
            let (co, payload) = res.unwrap();

            // SAFETY: See step 2 of the type's safety notice.
            let initial = unsafe { payload.cast::<R>().read() };
            let mut handle = YieldHandle {
                inner: Some(co),
                marker: PhantomData,
            };

            let complete = func(&mut handle, initial);

            let mut c = MaybeUninit::new(Yield::<Y, C>::Complete(complete));
            let co = handle.inner.take().unwrap();

            // SAFETY: See step 4 of the type's safety notice.
            let res = unsafe { co.resume_with_payload(c.as_mut_ptr().cast()) };
            res.unwrap().0
        }
    }

    /// # Safety
    ///
    /// The caller must observe the type's safety notice.
    pub(crate) unsafe fn from_inner(co: Option<Co<Z>>) -> Self {
        Gn {
            inner: co,
            marker: PhantomData,
        }
    }
}

impl Gn<'static, (), (), (), Boost> {
    pub fn builder() -> Builder<Boost, &'static Global, AbortHook> {
        Builder::new()
    }
}

impl<'a, Y, C, R, Z: Resume> Gn<'a, Y, C, R, Z> {
    pub fn new(
        rs: Z,
        func: impl FnOnce(&mut YieldHandle<'a, Y, C, R, Z>, R) -> C + Send + 'a,
    ) -> Result<Self, NewError<Z>> {
        let wrapper = move |co: Option<Co<Z>>| {
            let co = co.unwrap();

            // SAFETY: See step 1 of the type's safety notice.
            let res = unsafe { co.resume_with_payload(ptr::null_mut()) };
            let (co, payload) = res.unwrap();

            // SAFETY: See step 2 of the type's safety notice.
            let initial = unsafe { payload.cast::<R>().read() };
            let mut handle = YieldHandle {
                inner: Some(co),
                marker: PhantomData,
            };

            let complete = func(&mut handle, initial);

            let mut c = MaybeUninit::new(Yield::<Y, C>::Complete(complete));
            let co = handle.inner.take().unwrap();

            // SAFETY: See step 4 of the type's safety notice.
            let res = unsafe { co.resume_with_payload(c.as_mut_ptr().cast()) };
            res.unwrap().0
        };

        // SAFETY: We here constrain the function to be the same lifetime as the
        // generator itself, and the yield handle cannot escape the function as well.
        // Besides, `func` is `Send`.
        let co = unsafe { Co::builder().impl_by(rs).spawn_unchecked(wrapper) }?;
        Ok(Gn {
            inner: co.resume(),
            marker: PhantomData,
        })
    }

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
        match unsafe { payload.cast::<Yield<Y, C>>().read() } {
            Yield::Yielded(yielded) => {
                self.inner = Some(co);
                CoroutineState::Yielded(yielded)
            }
            Yield::Complete(complete) => {
                let res = co.resume();
                debug_assert!(res.is_none());
                CoroutineState::Complete(complete)
            }
        }
    }
}

impl<Y, C, R, Z: Resume + Unpin> Coroutine<R> for Gn<'_, Y, C, R, Z> {
    type Yield = Y;
    type Return = C;

    fn resume(mut self: Pin<&mut Self>, arg: R) -> CoroutineState<Y, C> {
        (*self).resume(arg)
    }
}

impl<'a, Y, C, R, Z: Resume> YieldHandle<'a, Y, C, R, Z> {
    pub fn yield_(&mut self, yielded: Y) -> R {
        let mut y = MaybeUninit::new(Yield::<Y, C>::Yielded(yielded));

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
}
