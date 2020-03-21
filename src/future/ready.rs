use super::Future;
use core::{
    borrow::BorrowMut,
    marker::PhantomData,
    pin::Pin,
    task::{Context, Poll},
};
use void::Void;

pub struct Ready<T, E = Void> {
    data: Option<Result<T, E>>,
    error: PhantomData<E>,
}

impl<T: Unpin, E> Unpin for Ready<T, E> {}

impl<C: ?Sized, T: Unpin, E> Future<C> for Ready<T, E> {
    type Ok = T;
    type Error = E;

    fn poll<R: BorrowMut<C>>(
        mut self: Pin<&mut Self>,
        _: &mut Context,
        _: R,
    ) -> Poll<Result<Self::Ok, Self::Error>> {
        Poll::Ready(self.data.take().expect("Ready polled after completion"))
    }
}

pub fn ready<T, E>(data: Result<T, E>) -> Ready<T, E> {
    Ready {
        data: Some(data),
        error: PhantomData,
    }
}

pub fn ok<T, E>(data: T) -> Ready<T, E> {
    Ready {
        data: Some(Ok(data)),
        error: PhantomData,
    }
}

pub fn err<T, E>(data: E) -> Ready<T, E> {
    Ready {
        data: Some(Err(data)),
        error: PhantomData,
    }
}
