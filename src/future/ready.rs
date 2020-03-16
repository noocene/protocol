use super::Future;
use core::{
    borrow::BorrowMut,
    marker::PhantomData,
    pin::Pin,
    task::{Context, Poll},
};
use void::Void;

pub struct Ready<T, E = Void> {
    data: Option<T>,
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
        Poll::Ready(Ok(self.data.take().expect("Ready polled after completion")))
    }
}

pub fn ready<T, E>(data: T) -> Ready<T, E> {
    Ready {
        data: Some(data),
        error: PhantomData,
    }
}
