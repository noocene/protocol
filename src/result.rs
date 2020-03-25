use crate::{
    future::{Either, MapErr},
    Coalesce, Dispatch, Fork, Future, FutureExt, Join, Read, Unravel, Write,
};
use core::{
    borrow::BorrowMut,
    mem::replace,
    pin::Pin,
    task::{Context, Poll},
};
use core_error::Error;
use futures::ready;
use thiserror::Error;

type UnravelError<C, T, E> = ResultUnravelError<
    <<C as Fork<T>>::Future as Future<C>>::Error,
    <<C as Fork<E>>::Future as Future<C>>::Error,
    <C as Write<Result<<C as Dispatch<T>>::Handle, <C as Dispatch<E>>::Handle>>>::Error,
    <<C as Fork<T>>::Target as Future<C>>::Error,
    <<C as Fork<E>>::Target as Future<C>>::Error,
    <<C as Fork<T>>::Finalize as Future<C>>::Error,
    <<C as Fork<E>>::Finalize as Future<C>>::Error,
>;

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
    OkWrite(<C as Dispatch<T>>::Handle, <C as Fork<T>>::Target),
    ErrWrite(<C as Dispatch<E>>::Handle, <C as Fork<E>>::Target),
    OkFlush(<C as Fork<T>>::Target),
    ErrFlush(<C as Fork<E>>::Target),
    OkTarget(<C as Fork<T>>::Target),
    ErrTarget(<C as Fork<E>>::Target),
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

#[derive(Debug, Error)]
#[bounds(
    where
        T: Error + 'static,
        E: Error + 'static,
        U: Error + 'static
)]
pub enum ResultCoalesceError<T, E, U> {
    #[error("failed to join Ok variant of Result: {0}")]
    DispatchOk(#[source] T),
    #[error("failed to join Err variant of Result: {0}")]
    DispatchErr(#[source] E),
    #[error("failed to read handle for Result: {0}")]
    Transport(#[source] U),
}

#[derive(Debug, Error)]
#[bounds(
    where
        T: Error + 'static,
        E: Error + 'static,
        U: Error + 'static,
        V: Error + 'static,
        W: Error + 'static,
        X: Error + 'static,
        Y: Error + 'static
)]
pub enum ResultUnravelError<T, E, U, V, W, X, Y> {
    #[error("failed to fork Ok variant of Result: {0}")]
    DispatchOk(#[source] T),
    #[error("failed to fork Err variant of Result: {0}")]
    DispatchErr(#[source] E),
    #[error("failed to target Ok variant of Result: {0}")]
    TargetOk(#[source] V),
    #[error("failed to target Err variant of Result: {0}")]
    TargetErr(#[source] W),
    #[error("failed to write handle for Result: {0}")]
    Transport(#[source] U),
    #[error("failed to target Ok variant of Result: {0}")]
    FinalizeOk(#[source] X),
    #[error("failed to target Err variant of Result: {0}")]
    FinalizeErr(#[source] Y),
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
    <C as Fork<T>>::Target: Unpin,
    <C as Fork<E>>::Target: Unpin,
    <C as Fork<T>>::Finalize: Unpin,
    <C as Fork<E>>::Finalize: Unpin,
    <C as Dispatch<T>>::Handle: Unpin,
    <C as Dispatch<E>>::Handle: Unpin,
{
    type Ok = Either<
        MapErr<
            <C as Fork<T>>::Finalize,
            fn(<<C as Fork<T>>::Finalize as Future<C>>::Error) -> UnravelError<C, T, E>,
        >,
        MapErr<
            <C as Fork<E>>::Finalize,
            fn(<<C as Fork<E>>::Finalize as Future<C>>::Error) -> UnravelError<C, T, E>,
        >,
    >;
    type Error = UnravelError<C, T, E>;

    fn poll<R: BorrowMut<C>>(
        mut self: Pin<&mut Self>,
        cx: &mut Context,
        mut ctx: R,
    ) -> Poll<Result<<Self as Future<C>>::Ok, Self::Error>> {
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
                    let (target, handle) = ready!(Pin::new(&mut *future).poll(cx, &mut *ctx))
                        .map_err(ResultUnravelError::DispatchOk)?;
                    replace(this, ResultUnravel::OkWrite(handle, target));
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
                    let (target, handle) = ready!(Pin::new(&mut *future).poll(cx, &mut *ctx))
                        .map_err(ResultUnravelError::DispatchErr)?;
                    replace(this, ResultUnravel::ErrWrite(handle, target));
                }
                ResultUnravel::OkWrite(_, _) => {
                    let mut ctx = Pin::new(&mut *ctx);
                    ready!(ctx.as_mut().poll_ready(cx)).map_err(ResultUnravelError::Transport)?;
                    let data = replace(this, ResultUnravel::Done);
                    if let ResultUnravel::OkWrite(data, target) = data {
                        ctx.write(Ok(data)).map_err(ResultUnravelError::Transport)?;
                        replace(this, ResultUnravel::OkFlush(target));
                    } else {
                        panic!("invalid state in ResultUnravel Write")
                    }
                }
                ResultUnravel::ErrWrite(_, _) => {
                    let mut ctx = Pin::new(&mut *ctx);
                    ready!(ctx.as_mut().poll_ready(cx)).map_err(ResultUnravelError::Transport)?;
                    let data = replace(this, ResultUnravel::Done);
                    if let ResultUnravel::ErrWrite(data, target) = data {
                        ctx.write(Err(data))
                            .map_err(ResultUnravelError::Transport)?;
                        replace(this, ResultUnravel::ErrFlush(target));
                    } else {
                        panic!("invalid state in ResultUnravel Write")
                    }
                }
                ResultUnravel::OkFlush(_) => {
                    ready!(Pin::new(&mut *ctx).poll_flush(cx))
                        .map_err(ResultUnravelError::Transport)?;
                    let data = replace(this, ResultUnravel::Done);
                    if let ResultUnravel::OkFlush(target) = data {
                        replace(this, ResultUnravel::OkTarget(target));
                    } else {
                        panic!("invalid state in ResultUnravel Write")
                    }
                }
                ResultUnravel::ErrFlush(_) => {
                    ready!(Pin::new(&mut *ctx).poll_flush(cx))
                        .map_err(ResultUnravelError::Transport)?;
                    let data = replace(this, ResultUnravel::Done);
                    if let ResultUnravel::ErrFlush(target) = data {
                        replace(this, ResultUnravel::ErrTarget(target));
                    } else {
                        panic!("invalid state in ResultUnravel Write")
                    }
                }
                ResultUnravel::OkTarget(target) => {
                    let finalize = ready!(Pin::new(target).poll(cx, &mut *ctx))
                        .map_err(ResultUnravelError::TargetOk)?;
                    replace(this, ResultUnravel::Done);
                    return Poll::Ready(Ok(FutureExt::<C>::into_left(
                        finalize.map_err(ResultUnravelError::FinalizeOk),
                    )));
                }
                ResultUnravel::ErrTarget(target) => {
                    let finalize = ready!(Pin::new(target).poll(cx, &mut *ctx))
                        .map_err(ResultUnravelError::TargetErr)?;
                    replace(this, ResultUnravel::Done);
                    return Poll::Ready(Ok(FutureExt::<C>::into_right(
                        finalize.map_err(ResultUnravelError::FinalizeErr),
                    )));
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
    type Error = ResultCoalesceError<
        <<C as Join<T>>::Future as Future<C>>::Error,
        <<C as Join<E>>::Future as Future<C>>::Error,
        <C as Read<Result<<C as Dispatch<T>>::Handle, <C as Dispatch<E>>::Handle>>>::Error,
    >;

    fn poll<R: BorrowMut<C>>(
        mut self: Pin<&mut Self>,
        cx: &mut Context,
        mut ctx: R,
    ) -> Poll<Result<Self::Ok, Self::Error>> {
        let ctx = ctx.borrow_mut();

        let this = &mut *self;

        loop {
            match this {
                ResultCoalesce::Read => {
                    let mut p_ctx = Pin::new(&mut *ctx);
                    match ready!(p_ctx.as_mut().read(cx)).map_err(ResultCoalesceError::Transport)? {
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
                        .map_err(ResultCoalesceError::DispatchOk)?)));
                }
                ResultCoalesce::ErrJoin(future) => {
                    return Poll::Ready(Ok(Err(ready!(Pin::new(future).poll(cx, &mut *ctx))
                        .map_err(ResultCoalesceError::DispatchErr)?)));
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
    <C as Fork<T>>::Target: Unpin,
    <C as Fork<E>>::Target: Unpin,
    <C as Fork<T>>::Finalize: Unpin,
    <C as Fork<E>>::Finalize: Unpin,
    <C as Dispatch<T>>::Handle: Unpin,
    <C as Dispatch<E>>::Handle: Unpin,
{
    type Finalize = Either<
        MapErr<
            <C as Fork<T>>::Finalize,
            fn(<<C as Fork<T>>::Finalize as Future<C>>::Error) -> UnravelError<C, T, E>,
        >,
        MapErr<
            <C as Fork<E>>::Finalize,
            fn(<<C as Fork<E>>::Finalize as Future<C>>::Error) -> UnravelError<C, T, E>,
        >,
    >;
    type Target = ResultUnravel<T, E, C>;

    fn unravel(self) -> Self::Target {
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
