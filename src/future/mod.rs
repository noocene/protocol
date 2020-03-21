use core::{
    borrow::BorrowMut,
    pin::Pin,
    task::{Context, Poll},
};

mod map;
pub use map::{Map, MapErr, MapOk};
mod either;
pub use either::Either;
use either::EitherState;
mod ready;
pub use ready::{err, ok, ready, Ready};
pub mod finalize;
pub use finalize::Finalize;

pub trait Future<C: ?Sized> {
    type Ok;
    type Error;

    fn poll<R: BorrowMut<C>>(
        self: Pin<&mut Self>,
        cx: &mut Context,
        ctx: R,
    ) -> Poll<Result<Self::Ok, Self::Error>>;
}

pub trait FutureExt<C: ?Sized>: Future<C> {
    fn map<T, E, F: FnMut(Result<Self::Ok, Self::Error>) -> Result<T, E>>(
        self,
        conv: F,
    ) -> Map<Self, F>
    where
        Self: Sized,
    {
        Map { fut: self, conv }
    }

    fn map_ok<T, F: FnMut(Self::Ok) -> T>(self, conv: F) -> MapOk<Self, F>
    where
        Self: Sized,
    {
        MapOk { fut: self, conv }
    }

    fn map_err<E, F: FnMut(Self::Error) -> E>(self, conv: F) -> MapErr<Self, F>
    where
        Self: Sized,
    {
        MapErr { fut: self, conv }
    }

    fn into_left<T>(self) -> Either<Self, T>
    where
        Self: Sized,
    {
        Either {
            state: EitherState::Left(self),
        }
    }

    fn into_right<T>(self) -> Either<T, Self>
    where
        Self: Sized,
    {
        Either {
            state: EitherState::Right(self),
        }
    }
}

impl<C: ?Sized, T: Future<C>> FutureExt<C> for T {}
