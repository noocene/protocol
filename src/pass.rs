use crate::{ready, Coalesce, Dispatch, Fork, Future, Join, Read, Unravel, Write};
use core::{
    borrow::BorrowMut,
    marker::PhantomData,
    mem::replace,
    pin::Pin,
    task::{Context, Poll},
};

#[derive(Debug)]
pub struct Pass<T>(pub T);

pub enum PassUnravel<T: Unpin, C: ?Sized + Write<<C as Dispatch<T>>::Handle> + Fork<T> + Unpin> {
    Data(T),
    Fork(C::Future),
    Write(C::Handle),
    Flush,
    Done,
}

pub enum PassCoalesce<T: Unpin, C: ?Sized + Read<<C as Dispatch<T>>::Handle> + Join<T> + Unpin> {
    Read,
    Join(C::Future),
    Done(PhantomData<T>),
}

#[derive(Debug)]
pub enum PassError<T, U> {
    Write(T),
    Dispatch(U),
}

impl<T: Unpin, C: ?Sized + Write<<C as Dispatch<T>>::Handle> + Fork<T> + Unpin> Future<C>
    for PassUnravel<T, C>
where
    C::Future: Unpin,
    C::Handle: Unpin,
{
    type Ok = ();
    type Error = PassError<C::Error, <C::Future as Future<C>>::Error>;

    fn poll<R: BorrowMut<C>>(
        mut self: Pin<&mut Self>,
        cx: &mut Context,
        mut ctx: R,
    ) -> Poll<Result<Self::Ok, Self::Error>>
    where
        Self: Sized,
    {
        let ctx = ctx.borrow_mut();

        let this = &mut *self;

        loop {
            match this {
                PassUnravel::Data(_) => {
                    let data = replace(this, PassUnravel::Done);
                    if let PassUnravel::Data(data) = data {
                        replace(this, PassUnravel::Fork(ctx.fork(data)));
                    } else {
                        panic!("invalid state in PassUnravel Data")
                    }
                }
                PassUnravel::Fork(future) => {
                    let handle = ready!(Pin::new(&mut *future).poll(cx, &mut *ctx))
                        .map_err(PassError::Dispatch)?;
                    replace(this, PassUnravel::Write(handle));
                }
                PassUnravel::Write(_) => {
                    let mut ctx = Pin::new(&mut *ctx);
                    ready!(ctx.as_mut().poll_ready(cx)).map_err(PassError::Write)?;
                    let data = replace(this, PassUnravel::Done);
                    if let PassUnravel::Write(data) = data {
                        ctx.write(data).map_err(PassError::Write)?;
                        replace(this, PassUnravel::Flush);
                    } else {
                        panic!("invalid state in PassUnravel Write")
                    }
                }
                PassUnravel::Flush => {
                    ready!(Pin::new(&mut *ctx).poll_ready(cx)).map_err(PassError::Write)?;
                    replace(this, PassUnravel::Done);
                    return Poll::Ready(Ok(()));
                }
                PassUnravel::Done => panic!("PassUnravel polled after completion"),
            }
        }
    }
}

impl<T: Unpin, C: ?Sized + Read<<C as Dispatch<T>>::Handle> + Join<T> + Unpin> Future<C>
    for PassCoalesce<T, C>
where
    C::Future: Unpin,
    C::Handle: Unpin,
{
    type Ok = Pass<T>;
    type Error = PassError<C::Error, <C::Future as Future<C>>::Error>;

    fn poll<R: BorrowMut<C>>(
        mut self: Pin<&mut Self>,
        cx: &mut Context,
        mut ctx: R,
    ) -> Poll<Result<Self::Ok, Self::Error>>
    where
        Self: Sized,
    {
        let ctx = ctx.borrow_mut();

        let this = &mut *self;

        loop {
            match this {
                PassCoalesce::Read => {
                    let mut ctx = Pin::new(&mut *ctx);
                    let handle = ready!(ctx.as_mut().read(cx)).map_err(PassError::Write)?;
                    replace(this, PassCoalesce::Join(ctx.join(handle)));
                }
                PassCoalesce::Join(future) => {
                    return Poll::Ready(Ok(Pass(
                        ready!(Pin::new(future).poll(cx, &mut *ctx))
                            .map_err(PassError::Dispatch)?,
                    )));
                }
                PassCoalesce::Done(_) => panic!("PassUnravel polled after completion"),
            }
        }
    }
}

impl<T: Unpin, C: ?Sized + Write<<C as Dispatch<T>>::Handle> + Fork<T> + Unpin> Unravel<C>
    for Pass<T>
where
    C::Future: Unpin,
    C::Handle: Unpin,
{
    type Future = PassUnravel<T, C>;

    fn unravel(self) -> Self::Future {
        PassUnravel::Data(self.0)
    }
}

impl<T: Unpin, C: ?Sized + Read<<C as Dispatch<T>>::Handle> + Join<T> + Unpin> Coalesce<C>
    for Pass<T>
where
    C::Future: Unpin,
    C::Handle: Unpin,
{
    type Future = PassCoalesce<T, C>;

    fn coalesce() -> Self::Future {
        PassCoalesce::Read
    }
}
