use super::Future;
use core::{
    borrow::BorrowMut,
    pin::Pin,
    task::{Context, Poll},
};
use futures::ready;

pub(super) enum EitherState<T, U> {
    Left(T),
    Right(U),
    Done,
}

pub struct Either<T, U> {
    pub(super) state: EitherState<T, U>,
}

impl<T: Unpin + Future<C>, U: Unpin + Future<C, Ok = T::Ok, Error = T::Error>, C: ?Sized> Future<C>
    for Either<T, U>
{
    type Ok = T::Ok;
    type Error = T::Error;

    fn poll<R: BorrowMut<C>>(
        mut self: Pin<&mut Self>,
        cx: &mut Context,
        mut ctx: R,
    ) -> Poll<Result<Self::Ok, Self::Error>> {
        let this = &mut *self;

        match &mut this.state {
            EitherState::Left(future) => {
                let data = ready!(Pin::new(future).poll(cx, ctx.borrow_mut()));
                this.state = EitherState::Done;
                Poll::Ready(data)
            }
            EitherState::Right(future) => {
                let data = ready!(Pin::new(future).poll(cx, ctx.borrow_mut()));
                this.state = EitherState::Done;
                Poll::Ready(data)
            }
            EitherState::Done => panic!("Either polled after completion"),
        }
    }
}
