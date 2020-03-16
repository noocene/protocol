use super::Future;
use core::{
    borrow::BorrowMut,
    pin::Pin,
    task::{Context, Poll},
};
use futures::ready;

pub struct Map<Fut, F> {
    pub(super) fut: Fut,
    pub(super) conv: F,
}

impl<
        C: ?Sized,
        T,
        E,
        Fut: Unpin + Future<C>,
        F: Unpin + FnMut(Result<Fut::Ok, Fut::Error>) -> Result<T, E>,
    > Future<C> for Map<Fut, F>
{
    type Ok = T;
    type Error = E;

    fn poll<R: BorrowMut<C>>(
        mut self: Pin<&mut Self>,
        cx: &mut Context,
        ctx: R,
    ) -> Poll<Result<Self::Ok, Self::Error>> {
        let item = ready!(Pin::new(&mut self.fut).poll(cx, ctx));
        Poll::Ready(((&mut *self).conv)(item))
    }
}

pub struct MapOk<Fut, F> {
    pub(super) fut: Fut,
    pub(super) conv: F,
}

impl<C: ?Sized, T, Fut: Unpin + Future<C>, F: Unpin + FnMut(Fut::Ok) -> T> Future<C>
    for MapOk<Fut, F>
{
    type Ok = T;
    type Error = Fut::Error;

    fn poll<R: BorrowMut<C>>(
        mut self: Pin<&mut Self>,
        cx: &mut Context,
        ctx: R,
    ) -> Poll<Result<Self::Ok, Self::Error>> {
        let item = ready!(Pin::new(&mut self.fut).poll(cx, ctx))?;
        Poll::Ready(Ok(((&mut *self).conv)(item)))
    }
}

pub struct MapErr<Fut, F> {
    pub(super) fut: Fut,
    pub(super) conv: F,
}

impl<C: ?Sized, E, Fut: Unpin + Future<C>, F: Unpin + FnMut(Fut::Error) -> E> Future<C>
    for MapErr<Fut, F>
{
    type Ok = Fut::Ok;
    type Error = E;

    fn poll<R: BorrowMut<C>>(
        mut self: Pin<&mut Self>,
        cx: &mut Context,
        ctx: R,
    ) -> Poll<Result<Self::Ok, Self::Error>> {
        let item = match ready!(Pin::new(&mut self.fut).poll(cx, ctx)) {
            Ok(data) => {
                return Poll::Ready(Ok(data));
            }
            Err(e) => e,
        };
        Poll::Ready(Err(((&mut *self).conv)(item)))
    }
}
