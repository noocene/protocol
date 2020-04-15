use crate::{
    allocated::ProtocolError, Dispatch, Finalize, Fork, Future, Join, Notify, Read, Write,
};
use core::{
    borrow::BorrowMut,
    future,
    mem::replace,
    pin::Pin,
    task::{Context, Poll},
};
use core_error::Error;
use futures::ready;
use thiserror::Error;

#[derive(Debug, Error)]
#[bounds(
    where
        A: Error + 'static,
        T: Error + 'static,
        B: Error + 'static,
        U: Error + 'static,
        V: Error + 'static,
        W: Error + 'static,
        C: Error + 'static,
)]
pub enum MoveCoalesceError<A, C, B, T, U, V, W> {
    #[error("failed to write argument handle for erased FnOnce: {0}")]
    Write(#[source] A),
    #[error("failed to fork argument for erased FnOnce: {0}")]
    Fork(#[source] C),
    #[error("failed to wrap argument notification for erased FnOnce: {0}")]
    Wrap(#[source] B),
    #[error("failed to read handle for erased FnOnce return: {0}")]
    Read(#[source] T),
    #[error("failed to create notification wrapper for erased FnOnce return: {0}")]
    Join(#[source] U),
    #[error("failed to target erased FnOnce argument: {0}")]
    Target(#[source] V),
    #[error("failed to finalize erased FnOnce argument: {0}")]
    Finalize(#[source] W),
}

pub type CoalesceError<T, U, C, W> = MoveCoalesceError<
    <C as Write<W>>::Error,
    <<C as Fork<<C as Notify<T>>::Notification>>::Future as Future<C>>::Error,
    <<C as Notify<T>>::Wrap as Future<C>>::Error,
    <C as Read<<C as Dispatch<U>>::Handle>>::Error,
    <<C as Join<U>>::Future as Future<C>>::Error,
    <<C as Fork<<C as Notify<T>>::Notification>>::Target as Future<C>>::Error,
    <<C as Finalize<<C as Fork<<C as Notify<T>>::Notification>>::Finalize>>::Output as Future<C>>::Error,
>;

pub enum MoveCoalesceState<
    T,
    U,
    C: Notify<T> + Join<U> + Finalize<<C as Fork<<C as Notify<T>>::Notification>>::Finalize>,
> where
    <C as Fork<<C as Notify<T>>::Notification>>::Finalize:
        Future<<C as Finalize<<C as Fork<<C as Notify<T>>::Notification>>::Finalize>>::Target>,
{
    Wrap(<C as Notify<T>>::Wrap),
    Fork(<C as Fork<<C as Notify<T>>::Notification>>::Future),
    Write(
        <C as Fork<<C as Notify<T>>::Notification>>::Target,
        <C as Dispatch<<C as Notify<T>>::Notification>>::Handle,
    ),
    Flush(<C as Fork<<C as Notify<T>>::Notification>>::Target),
    Target(<C as Fork<<C as Notify<T>>::Notification>>::Target),
    Finalize(<C as Finalize<<C as Fork<<C as Notify<T>>::Notification>>::Finalize>>::Output),
    Read,
    Join(<C as Join<U>>::Future),
    Done,
}

pub struct MoveCoalesce<
    T,
    U,
    C: Notify<T>
        + Join<U>
        + Write<W>
        + Read<<C as Dispatch<U>>::Handle>
        + Finalize<<C as Fork<<C as Notify<T>>::Notification>>::Finalize>,
    W,
    F: FnOnce(<C as Dispatch<<C as Notify<T>>::Notification>>::Handle) -> W,
> where
    <C as Fork<<C as Notify<T>>::Notification>>::Finalize:
        Future<<C as Finalize<<C as Fork<<C as Notify<T>>::Notification>>::Finalize>>::Target>,
{
    state: MoveCoalesceState<T, U, C>,
    conv: Option<F>,
    context: C,
}

impl<
    T,
    U,
    C: Notify<T>
        + Join<U>
        + Write<W>
        + Read<<C as Dispatch<U>>::Handle>
        + Finalize<<C as Fork<<C as Notify<T>>::Notification>>::Finalize>,
    W,
    F: FnOnce(<C as Dispatch<<C as Notify<T>>::Notification>>::Handle) -> W,
> MoveCoalesce<T, U, C, W, F>
where
    <C as Fork<<C as Notify<T>>::Notification>>::Finalize:
        Future<<C as Finalize<<C as Fork<<C as Notify<T>>::Notification>>::Finalize>>::Target>,
{
    pub fn new(mut context: C, args: T, conv: F) -> Self {
        MoveCoalesce {
            state: MoveCoalesceState::Wrap(context.wrap(args)),
            context,
            conv: Some(conv),
        }
    }
}

impl<
    T,
    U,
    C: Notify<T>
        + Join<U>
        + Write<W>
        + Read<<C as Dispatch<U>>::Handle>
        + Finalize<<C as Fork<<C as Notify<T>>::Notification>>::Finalize>,
    W,
    F: FnOnce(<C as Dispatch<<C as Notify<T>>::Notification>>::Handle) -> W,
> Unpin for MoveCoalesce<T, U, C, W, F>
where
    <C as Fork<<C as Notify<T>>::Notification>>::Finalize:
        Future<<C as Finalize<<C as Fork<<C as Notify<T>>::Notification>>::Finalize>>::Target>,
{
}

impl<
    T,
    U,
    C: Notify<T>
        + Join<U>
        + Write<W>
        + Read<<C as Dispatch<U>>::Handle>
        + Finalize<<C as Fork<<C as Notify<T>>::Notification>>::Finalize>,
    W,
    F: FnOnce(<C as Dispatch<<C as Notify<T>>::Notification>>::Handle) -> W,
> future::Future for MoveCoalesce<T, U, C, W, F>
where
    <C as Notify<T>>::Wrap: Unpin,
    C: Unpin,
    <C as Fork<<C as Notify<T>>::Notification>>::Future: Unpin,
    <C as Fork<<C as Notify<T>>::Notification>>::Target: Unpin,
    <C as Fork<<C as Notify<T>>::Notification>>::Finalize:
        Future<<C as Finalize<<C as Fork<<C as Notify<T>>::Notification>>::Finalize>>::Target>,
    <C as Finalize<<C as Fork<<C as Notify<T>>::Notification>>::Finalize>>::Output: Unpin,
    <C as Write<W>>::Error: Error + Send + 'static,
    <<C as Fork<<C as Notify<T>>::Notification>>::Future as Future<C>>::Error:
        Error + Send + 'static,
    <<C as Notify<T>>::Wrap as Future<C>>::Error: Error + Send + 'static,
    <C as Read<<C as Dispatch<U>>::Handle>>::Error: Error + Send + 'static,
    <C as Join<U>>::Future: Unpin,
    <<C as Join<U>>::Future as Future<C>>::Error: Error + Send + 'static,
    <<C as Fork<<C as Notify<T>>::Notification>>::Target as Future<C>>::Error:
        Error + Send + 'static,
    <<C as Finalize<<C as Fork<<C as Notify<T>>::Notification>>::Finalize>>::Output as Future<C>>::Error:
        Error + Send + 'static,
{
    type Output = Result<U, ProtocolError>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        use MoveCoalesceState::*;

        let this = &mut *self;
        let ctx = this.context.borrow_mut();

        loop {
            match &mut this.state {
                Wrap(future) => {
                    let wrapped = ready!(Pin::new(future).poll(cx, &mut *ctx))
                        .map_err(|e| ProtocolError::new(CoalesceError::<T, U, C, W>::Wrap(e)))?;
                    this.state = Fork(ctx.fork(wrapped));
                }
                Fork(future) => {
                    let (target, handle) = ready!(Pin::new(future).poll(cx, &mut *ctx))
                        .map_err(|e| ProtocolError::new(CoalesceError::<T, U, C, W>::Fork(e)))?;
                    this.state = Write(target, handle);
                }
                Write(_, _) => {
                    let mut ctx = Pin::new(&mut *ctx);
                    ready!(ctx.as_mut().poll_ready(cx))
                        .map_err(|e| ProtocolError::new(CoalesceError::<T, U, C, W>::Write(e)))?;
                    let data = replace(&mut this.state, Done);
                    if let Write(target, data) = data {
                        ctx.write((this.conv.take().unwrap())(data)).map_err(|e| {
                            ProtocolError::new(CoalesceError::<T, U, C, W>::Write(e))
                        })?;
                        this.state = Flush(target);
                    } else {
                        panic!("invalid state in MoveCoalesce Write")
                    }
                }
                Flush(_) => {
                    ready!(Pin::new(&mut *ctx).poll_ready(cx))
                        .map_err(|e| ProtocolError::new(CoalesceError::<T, U, C, W>::Write(e)))?;
                    let data = replace(&mut this.state, Done);
                    if let Flush(target) = data {
                        this.state = Target(target);
                    } else {
                        panic!("invalid state in MoveUnravel Write")
                    }
                }
                Target(target) => {
                    let finalize = ready!(Pin::new(target).poll(cx, &mut *ctx))
                        .map_err(|e| ProtocolError::new(CoalesceError::<T, U, C, W>::Target(e)))?;
                    this.state = Finalize(ctx.finalize(finalize));
                }
                Finalize(future) => {
                    ready!(Pin::new(future).poll(cx, &mut *ctx)).map_err(|e| {
                        ProtocolError::new(CoalesceError::<T, U, C, W>::Finalize(e))
                    })?;
                    this.state = Read;
                }
                Read => {
                    let handle: <C as Dispatch<U>>::Handle = ready!(Pin::new(&mut *ctx).read(cx))
                        .map_err(|e| ProtocolError::new(CoalesceError::<T, U, C, W>::Read(e)))?;
                    let join: <C as crate::Join<U>>::Future = crate::Join::<U>::join(ctx, handle);
                    this.state = Join(join);
                }
                Join(future) => {
                    let data = ready!(Pin::new(future).poll(cx, &mut *ctx))
                        .map_err(|e| ProtocolError::new(CoalesceError::<T, U, C, W>::Join(e)))?;
                    this.state = Done;
                    return Poll::Ready(Ok(data));
                }
                Done => panic!("erased FnOnce coalesce polled after completion"),
            }
        }
    }
}
