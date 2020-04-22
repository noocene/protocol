use super::Future;
use core::{
    borrow::BorrowMut,
    pin::Pin,
    task::{Context, Poll},
};
use core_error::Error;
use futures::ready;
use thiserror::Error;

#[derive(Debug, Error)]
#[bounds(where T: Error + 'static, U: Error + 'static)]
pub enum ThenError<T, U> {
    #[error("{0}")]
    Original(#[source] T),
    #[error("{0}")]
    Chained(#[source] U),
}

pub(super) enum ThenState<T, U> {
    Original(T),
    Chained(U),
}

pub struct Then<Fut, F, T> {
    pub(super) state: ThenState<Fut, T>,
    pub(super) conv: Option<F>,
}

impl<
    C: ?Sized,
    T: Unpin + Future<C>,
    Fut: Unpin + Future<C>,
    F: Unpin + FnOnce(Result<Fut::Ok, Fut::Error>) -> T,
> Future<C> for Then<Fut, F, T>
{
    type Ok = T::Ok;
    type Error = T::Error;

    fn poll<R: BorrowMut<C>>(
        mut self: Pin<&mut Self>,
        cx: &mut Context,
        mut ctx: R,
    ) -> Poll<Result<Self::Ok, Self::Error>> {
        let this = &mut *self;
        let ctx = ctx.borrow_mut();

        loop {
            match &mut this.state {
                ThenState::Original(future) => {
                    let item = ready!(Pin::new(future).poll(cx, &mut *ctx));
                    this.state = ThenState::Chained((this.conv.take().unwrap())(item));
                }
                ThenState::Chained(future) => {
                    return Pin::new(future).poll(cx, &mut *ctx);
                }
            }
        }
    }
}

pub struct AndThen<Fut, F, T> {
    pub(super) state: ThenState<Fut, T>,
    pub(super) conv: Option<F>,
}

impl<C: ?Sized, T: Unpin + Future<C>, Fut: Unpin + Future<C>, F: Unpin + FnOnce(Fut::Ok) -> T>
    Future<C> for AndThen<Fut, F, T>
{
    type Ok = T::Ok;
    type Error = ThenError<Fut::Error, T::Error>;

    fn poll<R: BorrowMut<C>>(
        mut self: Pin<&mut Self>,
        cx: &mut Context,
        mut ctx: R,
    ) -> Poll<Result<Self::Ok, Self::Error>> {
        let this = &mut *self;
        let ctx = ctx.borrow_mut();

        loop {
            match &mut this.state {
                ThenState::Original(future) => {
                    let item = ready!(Pin::new(future).poll(cx, &mut *ctx))
                        .map_err(ThenError::Original)?;
                    this.state = ThenState::Chained((this.conv.take().unwrap())(item));
                }
                ThenState::Chained(future) => {
                    return Pin::new(future)
                        .poll(cx, &mut *ctx)
                        .map_err(ThenError::Chained);
                }
            }
        }
    }
}
