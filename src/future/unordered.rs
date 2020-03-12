use super::Future;
use arrayvec::ArrayVec;
use core::{
    borrow::BorrowMut,
    mem::replace,
    pin::Pin,
    task::{Context, Poll},
};
use core_error::Error;
use thiserror::Error;

pub trait Futures {
    type Data;
}

pub enum EventualUnordered<T: Futures> {
    None,
    Incomplete(T),
    Complete(Unordered<T>),
}

#[derive(Debug, Error)]
#[error("attempted to read data from EventualUnordered after completion")]
pub struct Complete(());

impl<T: Futures> EventualUnordered<T> {
    pub fn new(input: T) -> Self {
        EventualUnordered::Incomplete(input)
    }

    pub fn complete(&mut self) -> bool
    where
        Unordered<T>: From<T>,
    {
        match replace(self, EventualUnordered::None) {
            EventualUnordered::None => panic!("invalid state"),
            EventualUnordered::Incomplete(incomplete) => {
                *self = EventualUnordered::Complete(incomplete.into());
                true
            }
            EventualUnordered::Complete(data) => {
                *self = EventualUnordered::Complete(data);
                false
            }
        }
    }

    pub fn data(&mut self) -> Result<&mut T, Complete> {
        if let EventualUnordered::Incomplete(data) = self {
            Ok(data)
        } else {
            Err(Complete(()))
        }
    }
}

#[derive(Debug, Error)]
#[bounds(where T: Error + 'static)]
pub enum EventualUnorderedError<T> {
    #[error(transparent)]
    Underlying(T),
    #[error("attempted to poll EventualUnordered before completion")]
    Incomplete,
}

impl<T> EventualUnorderedError<T> {
    pub fn unwrap_complete(self) -> T {
        if let EventualUnorderedError::Underlying(item) = self {
            item
        } else {
            panic!("unwrapped incomplete EventualUnorderedError")
        }
    }
}

impl<T: Futures + Unpin, C: ?Sized> Future<C> for EventualUnordered<T>
where
    Unordered<T>: Future<C>,
    T::Data: Unpin,
{
    type Ok = <Unordered<T> as Future<C>>::Ok;
    type Error = EventualUnorderedError<<Unordered<T> as Future<C>>::Error>;

    fn poll<R: BorrowMut<C>>(
        mut self: Pin<&mut Self>,
        cx: &mut Context,
        mut ctx: R,
    ) -> Poll<Result<Self::Ok, Self::Error>> {
        if let EventualUnordered::Complete(future) = &mut *self {
            Pin::new(future)
                .poll(cx, ctx.borrow_mut())
                .map_err(EventualUnorderedError::Underlying)
        } else {
            Poll::Ready(Err(EventualUnorderedError::Incomplete))
        }
    }
}

pub struct Unordered<T: Futures> {
    futures: T::Data,
}

impl<T> Futures for [T; 0] {
    type Data = ();
}

impl<C: ?Sized, T: Future<C>> Future<C> for Unordered<[T; 0]> {
    type Ok = ();
    type Error = T::Error;

    fn poll<R: BorrowMut<C>>(
        self: Pin<&mut Self>,
        _: &mut Context,
        _: R,
    ) -> Poll<Result<Self::Ok, Self::Error>> {
        Poll::Ready(Ok(()))
    }
}

impl<T> From<ArrayVec<[T; 0]>> for Unordered<ArrayVec<[T; 0]>> {
    fn from(_: ArrayVec<[T; 0]>) -> Self {
        Unordered { futures: () }
    }
}

impl<T> Futures for ArrayVec<[T; 0]> {
    type Data = ();
}

impl<C: ?Sized, T: Future<C>> Future<C> for Unordered<ArrayVec<[T; 0]>> {
    type Ok = ();
    type Error = T::Error;

    fn poll<R: BorrowMut<C>>(
        self: Pin<&mut Self>,
        _: &mut Context,
        _: R,
    ) -> Poll<Result<Self::Ok, Self::Error>> {
        Poll::Ready(Ok(()))
    }
}

impl<T> Futures for [T; 1] {
    type Data = T;
}

impl<C: ?Sized, T: Future<C> + Unpin> Future<C> for Unordered<[T; 1]> {
    type Ok = T::Ok;
    type Error = T::Error;

    fn poll<R: BorrowMut<C>>(
        mut self: Pin<&mut Self>,
        cx: &mut Context,
        ctx: R,
    ) -> Poll<Result<Self::Ok, Self::Error>> {
        Pin::new(&mut self.futures).poll(cx, ctx)
    }
}

impl<T> From<[T; 1]> for Unordered<[T; 1]> {
    fn from(futures: [T; 1]) -> Self {
        Unordered {
            futures: ArrayVec::from(futures).into_iter().next().unwrap(),
        }
    }
}

impl<T> Futures for ArrayVec<[T; 1]> {
    type Data = T;
}

impl<C: ?Sized, T: Future<C> + Unpin> Future<C> for Unordered<ArrayVec<[T; 1]>> {
    type Ok = T::Ok;
    type Error = T::Error;

    fn poll<R: BorrowMut<C>>(
        mut self: Pin<&mut Self>,
        cx: &mut Context,
        ctx: R,
    ) -> Poll<Result<Self::Ok, Self::Error>> {
        Pin::new(&mut self.futures).poll(cx, ctx)
    }
}

impl<T> From<ArrayVec<[T; 1]>> for Unordered<ArrayVec<[T; 1]>> {
    fn from(futures: ArrayVec<[T; 1]>) -> Self {
        Unordered {
            futures: ArrayVec::from(futures).into_iter().next().unwrap(),
        }
    }
}

macro_rules! array_impl {
    ($($len:literal)*) => {
        $(
            impl<T> Futures for [T; $len] {
                type Data = [Option<T>; $len];
            }

            impl<T> Futures for ArrayVec<[T; $len]> {
                type Data = [Option<T>; $len];
            }

            impl<C: ?Sized, T: Future<C, Ok = ()> + Unpin> Future<C> for Unordered<[T; $len]> {
                type Ok = ();
                type Error = T::Error;

                fn poll<R: BorrowMut<C>>(
                    mut self: Pin<&mut Self>,
                    cx: &mut Context,
                    mut ctx: R,
                ) -> Poll<Result<Self::Ok, Self::Error>> {
                    let mut some_pending = false;

                    for i in 0..$len {
                        if let Some(mut future) = self.futures[i].take() {
                            if let Poll::Pending = Pin::new(&mut future).poll(cx, ctx.borrow_mut())? {
                                self.futures[i] = Some(future);
                                some_pending = true;
                            }
                        }
                    }

                    if some_pending {
                        Poll::Pending
                    } else {
                        Poll::Ready(Ok(()))
                    }
                }
            }

            impl<C: ?Sized, T: Future<C, Ok = ()> + Unpin> Future<C> for Unordered<ArrayVec<[T; $len]>> {
                type Ok = ();
                type Error = T::Error;

                fn poll<R: BorrowMut<C>>(
                    mut self: Pin<&mut Self>,
                    cx: &mut Context,
                    mut ctx: R,
                ) -> Poll<Result<Self::Ok, Self::Error>> {
                    let mut some_pending = false;

                    for i in 0..$len {
                        if let Some(mut future) = self.futures[i].take() {
                            if let Poll::Pending = Pin::new(&mut future).poll(cx, ctx.borrow_mut())? {
                                self.futures[i] = Some(future);
                                some_pending = true;
                            }
                        }
                    }

                    if some_pending {
                        Poll::Pending
                    } else {
                        Poll::Ready(Ok(()))
                    }
                }
            }

            impl<T> From<[T; $len]> for Unordered<[T; $len]> {
                fn from(futures: [T; $len]) -> Self {
                    let futures = ArrayVec::from(futures)
                        .into_iter()
                        .map(|item| Some(item))
                        .collect::<ArrayVec<_>>()
                        .into_inner()
                        .unwrap_or_else(|_| panic!("invalid array state"));

                    Unordered { futures }
                }
            }

            impl<T> From<ArrayVec<[T; $len]>> for Unordered<ArrayVec<[T; $len]>> {
                fn from(futures: ArrayVec<[T; $len]>) -> Self {
                    let futures = futures
                        .into_iter()
                        .map(|item| Some(item))
                        .collect::<ArrayVec<_>>()
                        .into_inner()
                        .unwrap_or_else(|_| panic!("invalid array state"));
                    Unordered { futures }
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

impl<T> Futures for (T,) {
    type Data = T;
}

impl<C: ?Sized, T: Future<C> + Unpin> Future<C> for Unordered<(T,)> {
    type Ok = T::Ok;
    type Error = T::Error;

    fn poll<R: BorrowMut<C>>(
        mut self: Pin<&mut Self>,
        cx: &mut Context,
        ctx: R,
    ) -> Poll<Result<Self::Ok, Self::Error>> {
        Pin::new(&mut self.futures).poll(cx, ctx)
    }
}

impl<T> From<(T,)> for Unordered<(T,)> {
    fn from(futures: (T,)) -> Self {
        Unordered { futures: futures.0 }
    }
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
                    #[error("error in Unordered for tuple variant")]
                    $name(#[source] $name),
                )+
            }

            impl<$($name,)+> Futures for ($(Option<$name>,)+) {
                type Data = ($(Option<$name>,)+);
            }

            impl<C: ?Sized, $($name: Future<C, Ok = ()> + Unpin,)+> Future<C>
                for Unordered<($(Option<$name>,)+)>
            {
                type Ok = ();
                type Error = $error<$($name::Error,)+>;

                fn poll<R: BorrowMut<C>>(
                    mut self: Pin<&mut Self>,
                    cx: &mut Context,
                    mut ctx: R,
                ) -> Poll<Result<Self::Ok, Self::Error>> {
                    let mut some_pending = false;

                    $(
                        if let Some(mut future) = self.futures.$n.take() {
                            if let Poll::Pending = Pin::new(&mut future)
                                .poll(cx, ctx.borrow_mut())
                                .map_err($error::$name)?
                            {
                                self.futures.$n = Some(future);
                                some_pending = true;
                            }
                        }
                    )+

                    if some_pending {
                        Poll::Pending
                    } else {
                        Poll::Ready(Ok(()))
                    }
                }
            }

            impl<$($name,)+> From<($($name,)+)> for Unordered<($(Option<$name>,)+)> {
                fn from(futures: ($($name,)+)) -> Self {
                    Unordered {
                        futures: ($(Some(futures.$n)),+)
                    }
                }
            }

            impl<$($name,)+> From<($(Option<$name>,)+)> for Unordered<($(Option<$name>,)+)> {
                fn from(futures: ($(Option<$name>,)+)) -> Self {
                    Unordered {
                        futures,
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
    use alloc::vec::Vec;

    impl<T> Futures for Vec<T> {
        type Data = Vec<T>;
    }

    impl<C: ?Sized, T: Future<C, Ok = ()> + Unpin> Future<C> for Unordered<Vec<T>> {
        type Ok = ();
        type Error = T::Error;

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

            if self.futures.is_empty() {
                Poll::Ready(Ok(()))
            } else {
                Poll::Pending
            }
        }
    }

    impl<T> From<Vec<T>> for Unordered<Vec<T>> {
        fn from(futures: Vec<T>) -> Self {
            Unordered { futures }
        }
    }
}
