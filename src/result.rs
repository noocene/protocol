use crate::{ready, Coalesce, Dispatch, Fork, Future, Join, Read, Unravel, Write};
use core::{
    borrow::BorrowMut,
    mem::replace,
    pin::Pin,
    task::{Context, Poll},
};

pub enum ResultUnravel<
    T: Unpin,
    E: Unpin,
    C: ?Sized
        + Write<Result<<C as Dispatch<T>>::Handle, <C as Dispatch<E>>::Handle>>
        + Fork<T>
        + Fork<E>
        + Unpin,
> {
    Ok(T),
    Err(E),
    OkFork(<C as Fork<T>>::Future),
    ErrFork(<C as Fork<E>>::Future),
    OkWrite(<C as Dispatch<T>>::Handle),
    ErrWrite(<C as Dispatch<E>>::Handle),
    Flush,
    Done,
}

pub enum ResultCoalesce<
    T: Unpin,
    E: Unpin,
    C: ?Sized
        + Read<Result<<C as Dispatch<T>>::Handle, <C as Dispatch<E>>::Handle>>
        + Join<T>
        + Join<E>
        + Unpin,
> {
    Read,
    OkJoin(<C as Join<T>>::Future),
    ErrJoin(<C as Join<E>>::Future),
    Done,
}

impl<
        T: Unpin,
        E: Unpin,
        C: ?Sized
            + Write<Result<<C as Dispatch<T>>::Handle, <C as Dispatch<E>>::Handle>>
            + Fork<T>
            + Fork<E>
            + Unpin,
    > From<Result<T, E>> for ResultUnravel<T, E, C>
{
    fn from(data: Result<T, E>) -> Self {
        match data {
            Ok(data) => ResultUnravel::Ok(data),
            Err(data) => ResultUnravel::Err(data),
        }
    }
}

#[derive(Debug)]
pub enum ResultError<T, E, U> {
    DispatchOk(T),
    DispatchErr(E),
    Transport(U),
}

impl<
        T: Unpin,
        E: Unpin,
        C: ?Sized
            + Write<Result<<C as Dispatch<T>>::Handle, <C as Dispatch<E>>::Handle>>
            + Fork<T>
            + Fork<E>
            + Unpin,
    > Future<C> for ResultUnravel<T, E, C>
where
    <C as Fork<T>>::Future: Unpin,
    <C as Fork<E>>::Future: Unpin,
    <C as Dispatch<T>>::Handle: Unpin,
    <C as Dispatch<E>>::Handle: Unpin,
{
    type Ok = ();
    type Error = ResultError<
        <<C as Fork<T>>::Future as Future<C>>::Error,
        <<C as Fork<E>>::Future as Future<C>>::Error,
        <C as Write<Result<<C as Dispatch<T>>::Handle, <C as Dispatch<E>>::Handle>>>::Error,
    >;

    fn poll<R: BorrowMut<C>>(
        mut self: Pin<&mut Self>,
        cx: &mut Context,
        mut ctx: R,
    ) -> Poll<Result<(), Self::Error>>
    where
        Self: Sized,
    {
        let ctx = ctx.borrow_mut();

        let this = &mut *self;

        loop {
            match this {
                ResultUnravel::Ok(_) => {
                    let data = replace(this, ResultUnravel::Done);
                    if let ResultUnravel::Ok(data) = data {
                        replace(this, ResultUnravel::OkFork(ctx.fork(data)));
                    } else {
                        panic!("invalid state in ResultUnravel Some")
                    }
                }
                ResultUnravel::OkFork(future) => {
                    let handle = ready!(Pin::new(&mut *future).poll(cx, &mut *ctx))
                        .map_err(ResultError::DispatchOk)?;
                    replace(this, ResultUnravel::OkWrite(handle));
                }
                ResultUnravel::Err(_) => {
                    let data = replace(this, ResultUnravel::Done);
                    if let ResultUnravel::Err(data) = data {
                        replace(this, ResultUnravel::ErrFork(ctx.fork(data)));
                    } else {
                        panic!("invalid state in ResultUnravel Some")
                    }
                }
                ResultUnravel::ErrFork(future) => {
                    let handle = ready!(Pin::new(&mut *future).poll(cx, &mut *ctx))
                        .map_err(ResultError::DispatchErr)?;
                    replace(this, ResultUnravel::ErrWrite(handle));
                }
                ResultUnravel::OkWrite(_) => {
                    let mut ctx = Pin::new(&mut *ctx);
                    ready!(ctx.as_mut().poll_ready(cx)).map_err(ResultError::Transport)?;
                    let data = replace(this, ResultUnravel::Done);
                    if let ResultUnravel::OkWrite(data) = data {
                        ctx.write(Ok(data)).map_err(ResultError::Transport)?;
                        replace(this, ResultUnravel::Flush);
                    } else {
                        panic!("invalid state in ResultUnravel Write")
                    }
                }
                ResultUnravel::ErrWrite(_) => {
                    let mut ctx = Pin::new(&mut *ctx);
                    ready!(ctx.as_mut().poll_ready(cx)).map_err(ResultError::Transport)?;
                    let data = replace(this, ResultUnravel::Done);
                    if let ResultUnravel::ErrWrite(data) = data {
                        ctx.write(Err(data)).map_err(ResultError::Transport)?;
                        replace(this, ResultUnravel::Flush);
                    } else {
                        panic!("invalid state in ResultUnravel Write")
                    }
                }
                ResultUnravel::Flush => {
                    ready!(Pin::new(&mut *ctx).poll_ready(cx)).map_err(ResultError::Transport)?;
                    replace(this, ResultUnravel::Done);
                    return Poll::Ready(Ok(()));
                }
                ResultUnravel::Done => panic!("ResultUnravel polled after completion"),
            }
        }
    }
}

impl<
        T: Unpin,
        E: Unpin,
        C: ?Sized
            + Read<Result<<C as Dispatch<T>>::Handle, <C as Dispatch<E>>::Handle>>
            + Join<T>
            + Join<E>
            + Unpin,
    > Future<C> for ResultCoalesce<T, E, C>
where
    <C as Join<T>>::Future: Unpin,
    <C as Join<E>>::Future: Unpin,
    <C as Dispatch<T>>::Handle: Unpin,
    <C as Dispatch<E>>::Handle: Unpin,
{
    type Ok = Result<T, E>;
    type Error = ResultError<
        <<C as Join<T>>::Future as Future<C>>::Error,
        <<C as Join<E>>::Future as Future<C>>::Error,
        <C as Read<Result<<C as Dispatch<T>>::Handle, <C as Dispatch<E>>::Handle>>>::Error,
    >;

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
                ResultCoalesce::Read => {
                    let mut p_ctx = Pin::new(&mut *ctx);
                    match ready!(p_ctx.as_mut().read(cx)).map_err(ResultError::Transport)? {
                        Ok(data) => {
                            replace(
                                this,
                                ResultCoalesce::OkJoin(<C as Join<T>>::join(ctx, data)),
                            );
                        }
                        Err(data) => {
                            replace(
                                this,
                                ResultCoalesce::ErrJoin(<C as Join<E>>::join(ctx, data)),
                            );
                        }
                    }
                }
                ResultCoalesce::OkJoin(future) => {
                    return Poll::Ready(Ok(Ok(ready!(Pin::new(future).poll(cx, &mut *ctx))
                        .map_err(ResultError::DispatchOk)?)));
                }
                ResultCoalesce::ErrJoin(future) => {
                    return Poll::Ready(Ok(Err(ready!(Pin::new(future).poll(cx, &mut *ctx))
                        .map_err(ResultError::DispatchErr)?)));
                }
                ResultCoalesce::Done => panic!("ResultUnravel polled after completion"),
            }
        }
    }
}

impl<
        T: Unpin,
        E: Unpin,
        C: ?Sized
            + Write<Result<<C as Dispatch<T>>::Handle, <C as Dispatch<E>>::Handle>>
            + Fork<T>
            + Fork<E>
            + Unpin,
    > Unravel<C> for Result<T, E>
where
    <C as Fork<T>>::Future: Unpin,
    <C as Fork<E>>::Future: Unpin,
    <C as Dispatch<T>>::Handle: Unpin,
    <C as Dispatch<E>>::Handle: Unpin,
{
    type Future = ResultUnravel<T, E, C>;

    fn unravel(self) -> Self::Future {
        self.into()
    }
}

impl<
        T: Unpin,
        E: Unpin,
        C: ?Sized
            + Read<Result<<C as Dispatch<T>>::Handle, <C as Dispatch<E>>::Handle>>
            + Join<T>
            + Join<E>
            + Unpin,
    > Coalesce<C> for Result<T, E>
where
    <C as Join<T>>::Future: Unpin,
    <C as Join<E>>::Future: Unpin,
    <C as Dispatch<T>>::Handle: Unpin,
    <C as Dispatch<E>>::Handle: Unpin,
{
    type Future = ResultCoalesce<T, E, C>;

    fn coalesce() -> Self::Future {
        ResultCoalesce::Read
    }
}
