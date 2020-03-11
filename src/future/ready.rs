use super::Future;
use core::{
    borrow::BorrowMut,
    pin::Pin,
    task::{Context, Poll},
};
use void::Void;

pub struct Ready<T> {
    data: Option<T>,
}

impl<C: ?Sized, T: Unpin> Future<C> for Ready<T> {
    type Ok = T;
    type Error = Void;

    fn poll<R: BorrowMut<C>>(
        mut self: Pin<&mut Self>,
        _: &mut Context,
        _: R,
    ) -> Poll<Result<Self::Ok, Self::Error>> {
        Poll::Ready(Ok(self.data.take().expect("Ready polled after completion")))
    }
}

pub fn ready<T>(data: T) -> Ready<T> {
    Ready { data: Some(data) }
}
