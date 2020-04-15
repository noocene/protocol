use crate::{
    allocated::ProtocolError, CloneContext, Contextualize, Dispatch, Finalize, Fork, Future, Join,
    Notify, Read, Write,
};
use core::{
    future,
    mem::replace,
    pin::Pin,
    task::{Context, Poll},
};
use core_error::Error;
use futures::ready;
use thiserror::Error;

pub enum BorrowCoalesceState<T, U, C: CloneContext>
where
    C::Context: Notify<T>
        + Join<U>
        + Read<<C::Context as Dispatch<U>>::Handle>
        + Finalize<<C::Context as Fork<<C::Context as Notify<T>>::Notification>>::Finalize>,
    <C::Context as Fork<<C::Context as Notify<T>>::Notification>>::Finalize: Future<
        <C::Context as Finalize<
            <C::Context as Fork<<C::Context as Notify<T>>::Notification>>::Finalize,
        >>::Target,
    >,
{
    Begin,
    Contextualize(C::ForkOutput),
    WriteHandle(C::Handle),
    FlushHandle,
    Wrap(<C::Context as Notify<T>>::Wrap),
    Fork(<C::Context as Fork<<C::Context as Notify<T>>::Notification>>::Future),
    Write(
        <C::Context as Fork<<C::Context as Notify<T>>::Notification>>::Target,
        <C::Context as Dispatch<<C::Context as Notify<T>>::Notification>>::Handle,
    ),
    Flush(<C::Context as Fork<<C::Context as Notify<T>>::Notification>>::Target),
    Target(<C::Context as Fork<<C::Context as Notify<T>>::Notification>>::Target),
    Finalize(
        <C::Context as Finalize<
            <C::Context as Fork<<C::Context as Notify<T>>::Notification>>::Finalize,
        >>::Output,
    ),
    Read,
    Join(<C::Context as Join<U>>::Future),
    Done,
}

pub struct BorrowCoalesce<
    T,
    U,
    C: Clone + CloneContext + Write<W>,
    W,
    F: FnOnce(<C as Contextualize>::Handle) -> W,
> where
    C::Context: Notify<T>
        + Join<U>
        + Write<<C::Context as Dispatch<<C::Context as Notify<T>>::Notification>>::Handle>
        + Read<<C::Context as Dispatch<U>>::Handle>
        + Finalize<<C::Context as Fork<<C::Context as Notify<T>>::Notification>>::Finalize>,
    <C::Context as Fork<<C::Context as Notify<T>>::Notification>>::Finalize: Future<
        <C::Context as Finalize<
            <C::Context as Fork<<C::Context as Notify<T>>::Notification>>::Finalize,
        >>::Target,
    >,
{
    state: BorrowCoalesceState<T, U, C>,
    context: C,
    data: Option<T>,
    conv: Option<F>,
    loc_context: Option<C::Context>,
}

impl<T, U, C: Clone + CloneContext + Write<W>, W, F: FnOnce(<C as Contextualize>::Handle) -> W>
    BorrowCoalesce<T, U, C, W, F>
where
    C::Context: Notify<T>
        + Join<U>
        + Write<<C::Context as Dispatch<<C::Context as Notify<T>>::Notification>>::Handle>
        + Read<<C::Context as Dispatch<U>>::Handle>
        + Finalize<<C::Context as Fork<<C::Context as Notify<T>>::Notification>>::Finalize>,
    <C::Context as Fork<<C::Context as Notify<T>>::Notification>>::Finalize: Future<
        <C::Context as Finalize<
            <C::Context as Fork<<C::Context as Notify<T>>::Notification>>::Finalize,
        >>::Target,
    >,
{
    pub fn new(context: C, args: T, conv: F) -> Self {
        BorrowCoalesce {
            state: BorrowCoalesceState::Begin,
            context,
            data: Some(args),
            conv: Some(conv),
            loc_context: None,
        }
    }
}

impl<T, U, C: Clone + CloneContext + Write<W>, W, F: FnOnce(<C as Contextualize>::Handle) -> W> Unpin
    for BorrowCoalesce<T, U, C, W, F>
where
    C::Context: Notify<T>
        + Join<U>
        + Write<<C::Context as Dispatch<<C::Context as Notify<T>>::Notification>>::Handle>
        + Read<<C::Context as Dispatch<U>>::Handle>
        + Finalize<<C::Context as Fork<<C::Context as Notify<T>>::Notification>>::Finalize>,
    <C::Context as Fork<<C::Context as Notify<T>>::Notification>>::Finalize: Future<
        <C::Context as Finalize<
            <C::Context as Fork<<C::Context as Notify<T>>::Notification>>::Finalize,
        >>::Target,
    >,
{
}

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
    K: Error + 'static,
    L: Error + 'static,
)]
pub enum BorrowCoalesceError<A, C, B, T, U, V, W, K, L> {
    #[error("failed to write argument handle for erased Fn: {0}")]
    Write(#[source] A),
    #[error("failed to fork argument for erased Fn: {0}")]
    Fork(#[source] C),
    #[error("failed to wrap argument notification for erased Fn: {0}")]
    Wrap(#[source] B),
    #[error("failed to read handle for erased Fn return: {0}")]
    Read(#[source] T),
    #[error("failed to join erased Fn return: {0}")]
    Join(#[source] U),
    #[error("failed to target erased Fn argument: {0}")]
    Target(#[source] V),
    #[error("failed to finalize erased Fn argument: {0}")]
    Finalize(#[source] W),
    #[error("failed to contextualize erased Fn call: {0}")]
    Contextualize(#[source] K),
    #[error("failed to write context handle for erased Fn call: {0}")]
    WriteHandle(#[source] L),
}

pub type CoalesceError<T, U, C, W> = BorrowCoalesceError<
    <<C as CloneContext>::Context as Write<
        <<C as CloneContext>::Context as Dispatch<
            <<C as CloneContext>::Context as Notify<T>>::Notification,
        >>::Handle,
    >>::Error,
    <<<C as CloneContext>::Context as Fork<
        <<C as CloneContext>::Context as Notify<T>>::Notification,
    >>::Future as Future<<C as CloneContext>::Context>>::Error,
    <<<C as CloneContext>::Context as Notify<T>>::Wrap as Future<<C as CloneContext>::Context>>::Error,
    <<C as CloneContext>::Context as Read<<<C as CloneContext>::Context as Dispatch<U>>::Handle>>::Error,
    <<<C as CloneContext>::Context as Join<U>>::Future as Future<<C as CloneContext>::Context>>::Error,
    <<<C as CloneContext>::Context as Fork<
        <<C as CloneContext>::Context as Notify<T>>::Notification,
    >>::Target as Future<<C as CloneContext>::Context>>::Error,
    <<<C as CloneContext>::Context as Finalize<
        <<C as CloneContext>::Context as Fork<
            <<C as CloneContext>::Context as Notify<T>>::Notification,
        >>::Finalize,
    >>::Output as Future<<C as CloneContext>::Context>>::Error,
    <<C as CloneContext>::ForkOutput as Future<C>>::Error,
    <C as Write<W>>::Error,
>;

impl<T, U, C: Clone + CloneContext + Write<W>, W, F: FnOnce(<C as Contextualize>::Handle) -> W> future::Future
    for BorrowCoalesce<T, U, C, W, F>
where
    C::Context: Notify<T>
        + Join<U>
        + Write<<C::Context as Dispatch<<C::Context as Notify<T>>::Notification>>::Handle>
        + Read<<C::Context as Dispatch<U>>::Handle>
        + Finalize<<C::Context as Fork<<C::Context as Notify<T>>::Notification>>::Finalize>,
    <C::Context as Fork<<C::Context as Notify<T>>::Notification>>::Finalize: Future<
        <C::Context as Finalize<
            <C::Context as Fork<<C::Context as Notify<T>>::Notification>>::Finalize,
        >>::Target,
    >,
    <<C as CloneContext>::Context as Write<
        <C::Context as Dispatch<<C::Context as Notify<T>>::Notification>>::Handle,
    >>::Error: Send + Error + 'static,
    <<<C as CloneContext>::Context as Fork<
        <<C as CloneContext>::Context as Notify<T>>::Notification,
    >>::Future as Future<<C as CloneContext>::Context>>::Error: Send + Error + 'static,
    <<<C as CloneContext>::Context as Notify<T>>::Wrap as Future<<C as CloneContext>::Context>>::Error:
        Send + Error + 'static,
    <<C as CloneContext>::Context as Read<<<C as CloneContext>::Context as Dispatch<U>>::Handle>>::Error:
        Send + Error + 'static,
    <<<C as CloneContext>::Context as Join<U>>::Future as Future<<C as CloneContext>::Context>>::Error:
        Send + Error + 'static,
    <<<C as CloneContext>::Context as Fork<
        <<C as CloneContext>::Context as Notify<T>>::Notification,
    >>::Target as Future<<C as CloneContext>::Context>>::Error: Send + Error + 'static,
    <<<C as CloneContext>::Context as Finalize<
        <<C as CloneContext>::Context as Fork<
            <<C as CloneContext>::Context as Notify<T>>::Notification,
        >>::Finalize,
    >>::Output as Future<<C as CloneContext>::Context>>::Error: Send + Error + 'static,
    <C as CloneContext>::Context: Unpin,
    <<C as CloneContext>::Context as Finalize<
        <<C as CloneContext>::Context as Fork<
            <<C as CloneContext>::Context as Notify<T>>::Notification,
        >>::Finalize,
    >>::Output: Unpin,
    <<C as CloneContext>::Context as Notify<T>>::Wrap: Unpin,
    <<C as CloneContext>::Context as Fork<
        <<C as CloneContext>::Context as Notify<T>>::Notification,
    >>::Future: Unpin,
    <<C as CloneContext>::Context as Join<U>>::Future: Unpin,
    <<C as CloneContext>::Context as Fork<
        <<C as CloneContext>::Context as Notify<T>>::Notification,
    >>::Target: Unpin,
    C::ForkOutput: Unpin,
    C: Unpin,
    <C::ForkOutput as Future<C>>::Error: Send + Error + 'static,
    <C as Write<W>>::Error: Send + Error + 'static,
{
    type Output = Result<U, ProtocolError>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        use BorrowCoalesceState::*;

        let this = &mut *self;

        loop {
            match &mut this.state {
                Begin => {
                    this.state = Contextualize(this.context.fork_owned());
                }
                Contextualize(future) => {
                    let (context, handle) = ready!(Pin::new(future).poll(cx, &mut this.context))
                        .map_err(|e| {
                            ProtocolError::new(CoalesceError::<T, U, C, W>::Contextualize(e))
                        })?;
                    this.loc_context = Some(context);
                    this.state = WriteHandle(handle);
                }
                WriteHandle(_) => {
                    let state = &mut this.state;
                    let mut ctx = Pin::new(&mut this.context);
                    ready!(ctx.as_mut().poll_ready(cx)).map_err(|e| {
                        ProtocolError::new(CoalesceError::<T, U, C, W>::WriteHandle(e))
                    })?;
                    let data = replace(state, Done);
                    if let WriteHandle(handle) = data {
                        ctx.write((this.conv.take().unwrap())(handle))
                            .map_err(|e| {
                                ProtocolError::new(CoalesceError::<T, U, C, W>::WriteHandle(e))
                            })?;
                        *state = FlushHandle;
                    } else {
                        panic!("invalid state in BorrowCoalesce WriteHandle")
                    }
                }
                FlushHandle => {
                    ready!(Pin::new(&mut this.context).poll_flush(cx)).map_err(|e| {
                        ProtocolError::new(CoalesceError::<T, U, C, W>::WriteHandle(e))
                    })?;
                    this.state = Wrap(
                        this.loc_context
                            .as_mut()
                            .unwrap()
                            .wrap(this.data.take().unwrap()),
                    );
                }
                Wrap(future) => {
                    let ctx = this.loc_context.as_mut().unwrap();
                    let wrapped = ready!(Pin::new(future).poll(cx, &mut *ctx))
                        .map_err(|e| ProtocolError::new(CoalesceError::<T, U, C, W>::Wrap(e)))?;
                    this.state = Fork(ctx.fork(wrapped));
                }
                Fork(future) => {
                    let ctx = this.loc_context.as_mut().unwrap();
                    let (target, handle) = ready!(Pin::new(future).poll(cx, &mut *ctx))
                        .map_err(|e| ProtocolError::new(CoalesceError::<T, U, C, W>::Fork(e)))?;
                    this.state = Write(target, handle);
                }
                Write(_, _) => {
                    let ctx = this.loc_context.as_mut().unwrap();
                    let mut ctx = Pin::new(&mut *ctx);
                    ready!(ctx.as_mut().poll_ready(cx))
                        .map_err(|e| ProtocolError::new(CoalesceError::<T, U, C, W>::Write(e)))?;
                    let data = replace(&mut this.state, Done);
                    if let Write(target, data) = data {
                        ctx.write(data).map_err(|e| {
                            ProtocolError::new(CoalesceError::<T, U, C, W>::Write(e))
                        })?;
                        this.state = Flush(target);
                    } else {
                        panic!("invalid state in BorrowCoalesce Write")
                    }
                }
                Flush(_) => {
                    let ctx = this.loc_context.as_mut().unwrap();
                    ready!(Pin::new(&mut *ctx).poll_ready(cx))
                        .map_err(|e| ProtocolError::new(CoalesceError::<T, U, C, W>::Write(e)))?;
                    let data = replace(&mut this.state, Done);
                    if let Flush(target) = data {
                        this.state = Target(target);
                    } else {
                        panic!("invalid state in BorrowUnravel Write")
                    }
                }
                Target(target) => {
                    let ctx = this.loc_context.as_mut().unwrap();
                    let finalize = ready!(Pin::new(target).poll(cx, &mut *ctx))
                        .map_err(|e| ProtocolError::new(CoalesceError::<T, U, C, W>::Target(e)))?;
                    this.state = Finalize(ctx.finalize(finalize));
                }
                Finalize(future) => {
                    let ctx = this.loc_context.as_mut().unwrap();
                    ready!(Pin::new(future).poll(cx, &mut *ctx)).map_err(|e| {
                        ProtocolError::new(CoalesceError::<T, U, C, W>::Finalize(e))
                    })?;
                    this.state = Read;
                }
                Read => {
                    let ctx = this.loc_context.as_mut().unwrap();
                    let handle: <C::Context as Dispatch<U>>::Handle =
                        ready!(Pin::new(&mut *ctx).read(cx)).map_err(|e| {
                            ProtocolError::new(CoalesceError::<T, U, C, W>::Read(e))
                        })?;
                    let join: <C::Context as crate::Join<U>>::Future =
                        crate::Join::<U>::join(ctx, handle);
                    this.state = Join(join);
                }
                Join(future) => {
                    let ctx = this.loc_context.as_mut().unwrap();
                    let data = ready!(Pin::new(future).poll(cx, &mut *ctx))
                        .map_err(|e| ProtocolError::new(CoalesceError::<T, U, C, W>::Join(e)))?;
                    this.state = Done;
                    return Poll::Ready(Ok(data));
                }
                Done => panic!("erased Fn coalesce polled after completion"),
            }
        }
    }
}
