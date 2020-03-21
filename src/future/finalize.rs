use super::Future;
use crate::{
    future::{ok, Ready},
    Fork,
};
use arrayvec::{Array, ArrayVec, IntoIter};
use core::{
    borrow::BorrowMut,
    iter::Rev,
    marker::PhantomData,
    mem::replace,
    pin::Pin,
    task::{Context, Poll},
};
use core_error::Error;
use futures::ready;
use thiserror::Error;

pub trait Futures<C: ?Sized> {
    type Data;
}

pub enum EventualFinalize<C: ?Sized, T, U: Futures<C>> {
    None,
    Incomplete(T),
    Complete(Finalize<C, U>),
}

#[derive(Debug, Error)]
#[error("attempted to read data from EventualFinalize after completion")]
pub struct Complete(());

impl<C: ?Sized, T, U: Futures<C>> EventualFinalize<C, T, U> {
    pub fn new(input: T) -> Self {
        EventualFinalize::Incomplete(input)
    }

    pub fn complete(&mut self) -> bool
    where
        Finalize<C, U>: From<T>,
    {
        match replace(self, EventualFinalize::None) {
            EventualFinalize::None => panic!("invalid state"),
            EventualFinalize::Incomplete(incomplete) => {
                *self = EventualFinalize::Complete(incomplete.into());
                true
            }
            EventualFinalize::Complete(data) => {
                *self = EventualFinalize::Complete(data);
                false
            }
        }
    }

    pub fn data(&mut self) -> Result<&mut T, Complete> {
        if let EventualFinalize::Incomplete(data) = self {
            Ok(data)
        } else {
            Err(Complete(()))
        }
    }
}

#[derive(Debug, Error)]
#[bounds(where T: Error + 'static)]
pub enum EventualFinalizeError<T> {
    #[error(transparent)]
    Underlying(T),
    #[error("attempted to poll EventualFinalize before completion")]
    Incomplete,
}

impl<T> EventualFinalizeError<T> {
    pub fn unwrap_complete(self) -> T {
        if let EventualFinalizeError::Underlying(item) = self {
            item
        } else {
            panic!("unwrapped incomplete EventualFinalizeError")
        }
    }
}

impl<T, U: Futures<C> + Unpin, C: ?Sized> Unpin for EventualFinalize<C, T, U> {}

impl<T, U: Futures<C> + Unpin, C: ?Sized> Future<C> for EventualFinalize<C, T, U>
where
    Finalize<C, U>: Future<C>,
    U::Data: Unpin,
{
    type Ok = <Finalize<C, U> as Future<C>>::Ok;
    type Error = EventualFinalizeError<<Finalize<C, U> as Future<C>>::Error>;

    fn poll<R: BorrowMut<C>>(
        mut self: Pin<&mut Self>,
        cx: &mut Context,
        mut ctx: R,
    ) -> Poll<Result<Self::Ok, Self::Error>> {
        if let EventualFinalize::Complete(future) = &mut *self {
            Pin::new(future)
                .poll(cx, ctx.borrow_mut())
                .map_err(EventualFinalizeError::Underlying)
        } else {
            Poll::Ready(Err(EventualFinalizeError::Incomplete))
        }
    }
}

pub struct Finalize<C: ?Sized, T: Futures<C>> {
    futures: T::Data,
}

impl<C: ?Sized, T> Futures<C> for [T; 0] {
    type Data = ();
}

impl<C: ?Sized, T: Future<C>> Future<C> for Finalize<C, [T; 0]> {
    type Ok = Ready<()>;
    type Error = T::Error;

    fn poll<R: BorrowMut<C>>(
        self: Pin<&mut Self>,
        _: &mut Context,
        _: R,
    ) -> Poll<Result<Self::Ok, Self::Error>> {
        Poll::Ready(Ok(ok(())))
    }
}

impl<C: ?Sized, T> From<ArrayVec<[T; 0]>> for Finalize<C, [T; 0]> {
    fn from(_: ArrayVec<[T; 0]>) -> Self {
        Finalize { futures: () }
    }
}

impl<C: ?Sized + Fork<T>, T> Futures<C> for [T; 1] {
    type Data = C::Target;
}

impl<C: ?Sized + Fork<T>, T> Future<C> for Finalize<C, [T; 1]>
where
    C::Target: Unpin,
{
    type Ok = C::Finalize;
    type Error = <C::Target as Future<C>>::Error;

    fn poll<R: BorrowMut<C>>(
        mut self: Pin<&mut Self>,
        cx: &mut Context,
        ctx: R,
    ) -> Poll<Result<Self::Ok, Self::Error>> {
        Pin::new(&mut self.futures).poll(cx, ctx)
    }
}

impl<C: ?Sized + Fork<T>, T> From<[C::Target; 1]> for Finalize<C, [T; 1]> {
    fn from(futures: [C::Target; 1]) -> Self {
        Finalize {
            futures: ArrayVec::from(futures).into_iter().next().unwrap(),
        }
    }
}

impl<C: ?Sized + Fork<T>, T> From<ArrayVec<[C::Target; 1]>> for Finalize<C, [T; 1]> {
    fn from(futures: ArrayVec<[C::Target; 1]>) -> Self {
        Finalize {
            futures: futures.into_iter().next().unwrap(),
        }
    }
}

pub struct ArrayFinalize<C: ?Sized + Fork<T>, T, U: Array<Item = C::Finalize>> {
    futures: ArrayVec<U>,
    data: PhantomData<(T, C)>,
}

impl<C: ?Sized + Fork<T>, T, U: Array<Item = C::Finalize>> Unpin for ArrayFinalize<C, T, U> {}

impl<C: ?Sized + Fork<T>, T, U: Array<Item = C::Finalize>> ArrayFinalize<C, T, U> {
    fn push(&mut self, item: C::Finalize) {
        self.futures.push(item)
    }

    fn new() -> Self {
        ArrayFinalize {
            data: PhantomData,
            futures: ArrayVec::new(),
        }
    }
}

impl<C: ?Sized + Fork<T>, T, U: Array<Item = C::Finalize>> Future<C> for ArrayFinalize<C, T, U>
where
    C::Finalize: Unpin,
{
    type Ok = ();
    type Error = <C::Finalize as Future<C>>::Error;

    fn poll<R: BorrowMut<C>>(
        mut self: Pin<&mut Self>,
        cx: &mut Context,
        mut ctx: R,
    ) -> Poll<Result<Self::Ok, Self::Error>> {
        let mut i = 0;
        while i != self.futures.len() {
            if let Poll::Ready(()) = Pin::new(&mut self.futures[i]).poll(cx, ctx.borrow_mut())? {
                self.futures.remove(i);
            } else {
                i += 1;
            }
        }
        if self.futures.len() == 0 {
            Poll::Ready(Ok(()))
        } else {
            Poll::Pending
        }
    }
}

macro_rules! array_impl {
    ($($len:literal)*) => {
        $(
            impl<C: ?Sized + Fork<T>, T> Futures<C> for [T; $len] {
                type Data = (Rev<IntoIter<[C::Target; $len]>>, Option<C::Target>, Option<ArrayFinalize<C, T, [C::Finalize; $len]>>);
            }

            impl<C: ?Sized + Fork<T>, T> Future<C> for Finalize<C, [T; $len]>
            where
                C::Target: Unpin,
                C::Finalize: Unpin
            {
                type Ok = ArrayFinalize<C, T, [C::Finalize; $len]>;
                type Error = <C::Target as Future<C>>::Error;

                fn poll<R: BorrowMut<C>>(
                    mut self: Pin<&mut Self>,
                    cx: &mut Context,
                    mut ctx: R,
                ) -> Poll<Result<Self::Ok, Self::Error>> {
                    loop {
                        if let Some(futures) = self.futures.2.as_mut() {
                            let _ = Pin::new(futures).poll(cx, ctx.borrow_mut())?;
                        } else {
                            panic!("array Finalize polled after completion")
                        }

                        if let Some(future) = self.futures.1.as_mut() {
                            let finalize = ready!(Pin::new(future).poll(cx, ctx.borrow_mut()))?;
                            self.futures.2.as_mut().unwrap().push(finalize);
                            self.futures.1 = self.futures.0.next();
                            if self.futures.1.is_none() {
                                return Poll::Ready(Ok(self.futures.2.take().unwrap()));
                            }
                        } else {
                            self.futures.1 = self.futures.0.next();
                            if self.futures.1.is_none() {
                                return Poll::Ready(Ok(self.futures.2.take().unwrap()));
                            }
                        }
                    }
                }
            }

            impl<C: ?Sized + Fork<T>, T> From<[C::Target; $len]> for Finalize<C, [T; $len]> {
                fn from(futures: [C::Target; $len]) -> Self {
                    Finalize { futures: (ArrayVec::from(futures).into_iter().rev(), None, Some(ArrayFinalize::new())) }
                }
            }

            impl<C: ?Sized + Fork<T>, T> From<ArrayVec<[C::Target; $len]>> for Finalize<C, [T; $len]> {
                fn from(futures: ArrayVec<[C::Target; $len]>) -> Self {
                    Finalize { futures: (futures.into_iter().rev(), None, Some(ArrayFinalize::new())) }
                }
            }
        )*
    };
}

array_impl! {
    0002
    0003
    0004
    0005
    0006
    0007
    0008
    0009
    0010
    0011
    0012
    0013
    0014
    0015
    0016
    0017
    0018
    0019
    0020
    0021
    0022
    0023
    0024
    0025
    0026
    0027
    0028
    0029
    0030
    0031
    0032
    0064
    0128
    0256
    0512
    1024
    2048
    4096
    8192
}

impl<C: ?Sized + Fork<T>, T> Futures<C> for (T,) {
    type Data = C::Target;
}

impl<C: ?Sized + Fork<T>, T> Future<C> for Finalize<C, (T,)>
where
    C::Target: Unpin,
{
    type Ok = C::Finalize;
    type Error = <C::Target as Future<C>>::Error;

    fn poll<R: BorrowMut<C>>(
        mut self: Pin<&mut Self>,
        cx: &mut Context,
        ctx: R,
    ) -> Poll<Result<Self::Ok, Self::Error>> {
        Pin::new(&mut self.futures).poll(cx, ctx)
    }
}

impl<C: ?Sized + Fork<T>, T> From<(C::Target,)> for Finalize<C, (T,)> {
    fn from(futures: (C::Target,)) -> Self {
        Finalize { futures: futures.0 }
    }
}

pub struct TupleTargetInner<T> {
    data: T,
    index: u8,
}

pub struct TupleFinalizeInner<T, U> {
    marker: PhantomData<U>,
    data: T,
}

macro_rules! tuple_impls {
    ($($error:ident => ($($n:tt $name:ident)+))+) => {
        $(
            #[doc(hidden)]
            #[derive(Debug, Error)]
            #[bounds(
                where
                    $($name: Error + 'static,)+
            )]
            pub enum $error<$($name,)+> {
                $(
                    #[error("error in target or finalize for tuple variant")]
                    $name(#[source] $name),
                )+
            }

            impl<C: ?Sized $(+ Fork<$name>)+, $($name,)+> Futures<C> for ($($name,)+) {
                type Data = (TupleTargetInner<($(Option<<C as Fork<$name>>::Target>,)+)>, ($(Option<<C as Fork<$name>>::Finalize>,)+));
            }

            impl<C: ?Sized $(+ Fork<$name>)+, $($name: Unpin,)+> Future<C>
                for TupleFinalizeInner<($(Option<<C as Fork<$name>>::Finalize>,)+), ($($name,)+)>
            where
                $(<C as Fork<$name>>::Target: Unpin,)+
                $(<C as Fork<$name>>::Finalize: Unpin,)+
            {
                type Ok = ();
                type Error = $error<$(<<C as Fork<$name>>::Target as Future<C>>::Error,)+>;

                fn poll<R: BorrowMut<C>>(
                    mut self: Pin<&mut Self>,
                    cx: &mut Context,
                    mut ctx: R,
                ) -> Poll<Result<Self::Ok, Self::Error>> {
                    let mut some_pending = false;

                    $({
                        if let Some(mut future) = self.data.$n.take() {
                            match Pin::new(&mut future).poll(cx, ctx.borrow_mut()) {
                                Poll::Pending => {
                                    some_pending = true;
                                    self.data.$n = Some(future);
                                }
                                Poll::Ready(Err(e)) => { return Poll::Ready(Err($error::$name(e))); }
                                Poll::Ready(Ok(())) => {}
                            }
                        }
                    })*

                    if some_pending {
                        Poll::Pending
                    } else {
                        Poll::Ready(Ok(()))
                    }
                }
            }

            impl<C: ?Sized $(+ Fork<$name>)+, $($name: Unpin,)+> Future<C>
                for Finalize<C, ($($name,)+)>
            where
                $(<C as Fork<$name>>::Target: Unpin,)+
                $(<C as Fork<$name>>::Finalize: Unpin,)+
            {
                type Ok = TupleFinalizeInner<($(Option<<C as Fork<$name>>::Finalize>,)+), ($($name,)+)>;
                type Error = $error<$(<<C as Fork<$name>>::Target as Future<C>>::Error,)+>;

                fn poll<R: BorrowMut<C>>(
                    mut self: Pin<&mut Self>,
                    cx: &mut Context,
                    mut ctx: R,
                ) -> Poll<Result<Self::Ok, Self::Error>> {
                    $({
                        if let Some(mut future) = (self.futures.1).$n.take() {
                            match Pin::new(&mut future).poll(cx, ctx.borrow_mut()) {
                                Poll::Pending => {
                                    (self.futures.1).$n = Some(future);
                                }
                                Poll::Ready(Err(e)) => { return Poll::Ready(Err($error::$name(e))); }
                                Poll::Ready(Ok(())) => {}
                            }
                        }
                    })*

                    loop {
                        match self.futures.0.index {
                            $($n => {
                                if let Some(future) = self.futures.0.data.$n.as_mut() {
                                    let finalize = ready!(Pin::new(future).poll(cx, ctx.borrow_mut())).map_err($error::$name)?;
                                    self.futures.0.index += 1;
                                    (self.futures.1).$n = Some(finalize);
                                } else {
                                    self.futures.0.index += 1;
                                }
                            })*
                            _ => { return Poll::Ready(Ok(TupleFinalizeInner {
                                marker: PhantomData,
                                data: ($((self.futures.1).$n.take(),)+)
                            })); }
                        }
                    }
                }
            }

            impl<C: ?Sized $(+ Fork<$name>)+, $($name: Unpin,)+> From<($(Option<<C as Fork<$name>>::Target>,)+)>
                for Finalize<C, ($($name,)+)>
            where
                $(<C as Fork<$name>>::Target: Unpin,)+
                $(<C as Fork<$name>>::Finalize: Unpin,)+
            {
                fn from(futures: ($(Option<<C as Fork<$name>>::Target>,)+)) -> Self {
                    Finalize {
                        futures: (TupleTargetInner { data: ($(futures.$n),+), index: 0 }, ($(None::<<C as Fork<$name>>::Finalize>,)+))
                    }
                }
            }
        )+
    }
}

tuple_impls! {
    Tuple2Error  => (0 T0 1 T1)
    Tuple3Error  => (0 T0 1 T1 2 T2)
    Tuple4Error  => (0 T0 1 T1 2 T2 3 T3)
    Tuple5Error  => (0 T0 1 T1 2 T2 3 T3 4 T4)
    Tuple6Error  => (0 T0 1 T1 2 T2 3 T3 4 T4 5 T5)
    Tuple7Error  => (0 T0 1 T1 2 T2 3 T3 4 T4 5 T5 6 T6)
    Tuple8Error  => (0 T0 1 T1 2 T2 3 T3 4 T4 5 T5 6 T6 7 T7)
    Tuple9Error  => (0 T0 1 T1 2 T2 3 T3 4 T4 5 T5 6 T6 7 T7 8 T8)
    Tuple10Error => (0 T0 1 T1 2 T2 3 T3 4 T4 5 T5 6 T6 7 T7 8 T8 9 T9)
    Tuple11Error => (0 T0 1 T1 2 T2 3 T3 4 T4 5 T5 6 T6 7 T7 8 T8 9 T9 10 T10)
    Tuple12Error => (0 T0 1 T1 2 T2 3 T3 4 T4 5 T5 6 T6 7 T7 8 T8 9 T9 10 T10 11 T11)
    Tuple13Error => (0 T0 1 T1 2 T2 3 T3 4 T4 5 T5 6 T6 7 T7 8 T8 9 T9 10 T10 11 T11 12 T12)
    Tuple14Error => (0 T0 1 T1 2 T2 3 T3 4 T4 5 T5 6 T6 7 T7 8 T8 9 T9 10 T10 11 T11 12 T12 13 T13)
    Tuple15Error => (0 T0 1 T1 2 T2 3 T3 4 T4 5 T5 6 T6 7 T7 8 T8 9 T9 10 T10 11 T11 12 T12 13 T13 14 T14)
    Tuple16Error => (0 T0 1 T1 2 T2 3 T3 4 T4 5 T5 6 T6 7 T7 8 T8 9 T9 10 T10 11 T11 12 T12 13 T13 14 T14 15 T15)
}

#[cfg(feature = "alloc")]
#[doc(inline)]
pub use allocated::*;

#[cfg(feature = "alloc")]
mod allocated {
    use super::*;
    use alloc::vec::{IntoIter, Vec};

    pub struct VecFinalize<C: ?Sized + Fork<T>, T> {
        futures: Vec<C::Finalize>,
    }

    impl<C: ?Sized + Fork<T>, T> Unpin for VecFinalize<C, T> {}

    impl<C: ?Sized + Fork<T>, T> VecFinalize<C, T> {
        fn push(&mut self, item: C::Finalize) {
            self.futures.push(item)
        }

        fn with_capacity(cap: usize) -> Self {
            VecFinalize {
                futures: Vec::with_capacity(cap),
            }
        }
    }

    impl<C: ?Sized + Fork<T>, T> Future<C> for VecFinalize<C, T>
    where
        C::Finalize: Unpin,
    {
        type Ok = ();
        type Error = <C::Finalize as Future<C>>::Error;

        fn poll<R: BorrowMut<C>>(
            mut self: Pin<&mut Self>,
            cx: &mut Context,
            mut ctx: R,
        ) -> Poll<Result<Self::Ok, Self::Error>> {
            let mut i = 0;
            while i != self.futures.len() {
                if let Poll::Ready(()) =
                    Pin::new(&mut self.futures[i]).poll(cx, ctx.borrow_mut())?
                {
                    self.futures.remove(i);
                } else {
                    i += 1;
                }
            }
            if self.futures.len() == 0 {
                Poll::Ready(Ok(()))
            } else {
                Poll::Pending
            }
        }
    }

    impl<C: ?Sized + Fork<T>, T> Futures<C> for Vec<T> {
        type Data = (
            Rev<IntoIter<C::Target>>,
            Option<C::Target>,
            Option<VecFinalize<C, T>>,
        );
    }

    impl<C: ?Sized + Fork<T>, T> Future<C> for Finalize<C, Vec<T>>
    where
        C::Target: Unpin,
        C::Finalize: Unpin,
    {
        type Ok = VecFinalize<C, T>;
        type Error = <C::Target as Future<C>>::Error;

        fn poll<R: BorrowMut<C>>(
            mut self: Pin<&mut Self>,
            cx: &mut Context,
            mut ctx: R,
        ) -> Poll<Result<Self::Ok, Self::Error>> {
            loop {
                if let Some(futures) = self.futures.2.as_mut() {
                    let _ = Pin::new(futures).poll(cx, ctx.borrow_mut())?;
                } else {
                    panic!("array Finalize polled after completion")
                }

                if let Some(future) = self.futures.1.as_mut() {
                    let finalize = ready!(Pin::new(future).poll(cx, ctx.borrow_mut()))?;
                    self.futures.2.as_mut().unwrap().push(finalize);
                    self.futures.1 = self.futures.0.next();
                    if self.futures.1.is_none() {
                        return Poll::Ready(Ok(self.futures.2.take().unwrap()));
                    }
                } else {
                    self.futures.1 = self.futures.0.next();
                    if self.futures.1.is_none() {
                        return Poll::Ready(Ok(self.futures.2.take().unwrap()));
                    }
                }
            }
        }
    }

    impl<C: ?Sized + Fork<T>, T> From<Vec<C::Target>> for Finalize<C, Vec<T>> {
        fn from(futures: Vec<C::Target>) -> Self {
            let len = futures.len();

            Finalize {
                futures: (
                    futures.into_iter().rev(),
                    None,
                    Some(VecFinalize::with_capacity(len)),
                ),
            }
        }
    }
}
