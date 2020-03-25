use crate::{
    allocated::{Flatten, FromError, ProtocolError},
    CloneContext, Coalesce, ContextReference, Contextualize, Dispatch, Finalize, FinalizeImmediate,
    Fork, Future, Join, Notify, Read, ReferenceContext, Unravel, Write,
};
use alloc::{boxed::Box, vec, vec::Vec};
use core::{
    borrow::BorrowMut,
    future,
    marker::PhantomData,
    mem::replace,
    pin::Pin,
    task::{Context, Poll},
};
use core_error::Error;
use futures::{
    future::{ready, Either},
    ready,
    stream::once,
    FutureExt, Stream, StreamExt,
};
use thiserror::Error;

#[derive(Debug, Error)]
#[bounds(
    where
        A: Error + 'static,
        T: Error + 'static,
        B: Error + 'static,
        I: Error + 'static,
        U: Error + 'static,
        V: Error + 'static,
        W: Error + 'static,
        Z: Error + 'static,
        K: Error + 'static,
        C: Error + 'static,
)]
pub enum StreamUnravelError<A, B, C, Z, K, T, I, U, V, W> {
    #[error("failed to read argument handle for erased stream: {0}")]
    Read(#[source] A),
    #[error("failed to join argument for erased stream: {0}")]
    Join(#[source] C),
    #[error("failed to unwrap argument notification for erased stream: {0}")]
    Unwrap(#[source] B),
    #[error("failed to contextualize erased stream: {0}")]
    Contextualize(#[source] Z),
    #[error("failed to write context handle for erased stream: {0}")]
    Write(#[source] K),
    #[error("failed to write handle for stream Stream: {0}")]
    Transport(#[source] T),
    #[error("failed to create notification wrapper for erased stream return: {0}")]
    Notify(#[source] I),
    #[error("failed to fork erased stream return: {0}")]
    Dispatch(#[source] U),
    #[error("failed to target erased stream return: {0}")]
    Target(#[source] V),
    #[error("failed to finalize erased stream return: {0}")]
    Finalize(#[source] W),
}

pub enum ErasedStreamUnravelState<C: ?Sized + ReferenceContext, U>
where
    <C::Context as ContextReference<C>>::Target: Notify<()>
        + Notify<U>
        + Read<
            Option<
                <<C::Context as ContextReference<C>>::Target as Dispatch<
                    <<C::Context as ContextReference<C>>::Target as Notify<()>>::Notification,
                >>::Handle,
            >,
        > + Write<
            Option<
                <<C::Context as ContextReference<C>>::Target as Dispatch<
                    <<C::Context as ContextReference<C>>::Target as Notify<U>>::Notification,
                >>::Handle,
            >,
        >,
{
    Read,
    Join(
        <<C::Context as ContextReference<C>>::Target as Join<
            <<C::Context as ContextReference<C>>::Target as Notify<()>>::Notification,
        >>::Future,
    ),
    Unwrap(<<C::Context as ContextReference<C>>::Target as Notify<()>>::Unwrap),
    Next,
    Wrap(<<C::Context as ContextReference<C>>::Target as Notify<U>>::Wrap),
    Fork(
        <<C::Context as ContextReference<C>>::Target as Fork<
            <<C::Context as ContextReference<C>>::Target as Notify<U>>::Notification,
        >>::Future,
    ),
    WriteNone,
    Write(
        <<C::Context as ContextReference<C>>::Target as Fork<
            <<C::Context as ContextReference<C>>::Target as Notify<U>>::Notification,
        >>::Target,
        <<C::Context as ContextReference<C>>::Target as Dispatch<
            <<C::Context as ContextReference<C>>::Target as Notify<U>>::Notification,
        >>::Handle,
    ),
    Flush(
        <<C::Context as ContextReference<C>>::Target as Fork<
            <<C::Context as ContextReference<C>>::Target as Notify<U>>::Notification,
        >>::Target,
    ),
    FlushNone,
    Target(
        <<C::Context as ContextReference<C>>::Target as Fork<
            <<C::Context as ContextReference<C>>::Target as Notify<U>>::Notification,
        >>::Target,
    ),
    Done,
}

pub struct ErasedStreamUnravel<C: ?Sized + ReferenceContext, U: Stream>
where
    <C::Context as ContextReference<C>>::Target: Notify<()>
        + Notify<U::Item>
        + Read<
            Option<
                <<C::Context as ContextReference<C>>::Target as Dispatch<
                    <<C::Context as ContextReference<C>>::Target as Notify<()>>::Notification,
                >>::Handle,
            >,
        > + Write<
            Option<
                <<C::Context as ContextReference<C>>::Target as Dispatch<
                    <<C::Context as ContextReference<C>>::Target as Notify<U::Item>>::Notification,
                >>::Handle,
            >,
        >,
{
    state: ErasedStreamUnravelState<C, U::Item>,
    stream: U,
    pending: Vec<
        <<C::Context as ContextReference<C>>::Target as Fork<
            <<C::Context as ContextReference<C>>::Target as Notify<U::Item>>::Notification,
        >>::Finalize,
    >,
    context: C::Context,
}

impl<C: ?Sized + ReferenceContext, U: Stream> Unpin for ErasedStreamUnravel<C, U> where
    <C::Context as ContextReference<C>>::Target: Notify<()>
        + Notify<U::Item>
        + Read<
            Option<
                <<C::Context as ContextReference<C>>::Target as Dispatch<
                    <<C::Context as ContextReference<C>>::Target as Notify<()>>::Notification,
                >>::Handle,
            >,
        > + Write<
            Option<
                <<C::Context as ContextReference<C>>::Target as Dispatch<
                    <<C::Context as ContextReference<C>>::Target as Notify<U::Item>>::Notification,
                >>::Handle,
            >,
        >
{
}

impl<C: ?Sized + Write<<C as Contextualize>::Handle> + ReferenceContext, U: Stream> Future<C>
    for ErasedStreamUnravel<C, U>
where
    <C::Context as ContextReference<C>>::Target: Notify<()>
        + Notify<U::Item>
        + Read<
            Option<
                <<C::Context as ContextReference<C>>::Target as Dispatch<
                    <<C::Context as ContextReference<C>>::Target as Notify<()>>::Notification,
                >>::Handle,
            >,
        > + Write<
            Option<
                <<C::Context as ContextReference<C>>::Target as Dispatch<
                    <<C::Context as ContextReference<C>>::Target as Notify<U::Item>>::Notification,
                >>::Handle,
            >,
        >,
    U: Unpin,
    <<C::Context as ContextReference<C>>::Target as Notify<U::Item>>::Wrap: Unpin,
    <<C::Context as ContextReference<C>>::Target as Fork<
        <<C::Context as ContextReference<C>>::Target as Notify<U::Item>>::Notification,
    >>::Target: Unpin,
    <<C::Context as ContextReference<C>>::Target as Fork<
        <<C::Context as ContextReference<C>>::Target as Notify<U::Item>>::Notification,
    >>::Future: Unpin,
    <<C::Context as ContextReference<C>>::Target as Fork<
        <<C::Context as ContextReference<C>>::Target as Notify<U::Item>>::Notification,
    >>::Finalize: Unpin,
    <C::Context as ContextReference<C>>::Target: Unpin,
    <<C::Context as ContextReference<C>>::Target as Notify<()>>::Unwrap: Unpin,
    <<C::Context as ContextReference<C>>::Target as Join<
        <<C::Context as ContextReference<C>>::Target as Notify<()>>::Notification,
    >>::Future: Unpin,
{
    type Ok = ();
    type Error = StreamUnravelError<
        <<C::Context as ContextReference<C>>::Target as Read<
            Option<
                <<C::Context as ContextReference<C>>::Target as Dispatch<
                    <<C::Context as ContextReference<C>>::Target as Notify<()>>::Notification,
                >>::Handle,
            >,
        >>::Error,
        <<<C::Context as ContextReference<C>>::Target as Notify<()>>::Unwrap as Future<
            <C::Context as ContextReference<C>>::Target,
        >>::Error,
        <<<C::Context as ContextReference<C>>::Target as Join<
            <<C::Context as ContextReference<C>>::Target as Notify<()>>::Notification,
        >>::Future as Future<<C::Context as ContextReference<C>>::Target>>::Error,
        <C::ForkOutput as Future<C>>::Error,
        C::Error,
        <<C::Context as ContextReference<C>>::Target as Write<
            Option<
                <<C::Context as ContextReference<C>>::Target as Dispatch<
                    <<C::Context as ContextReference<C>>::Target as Notify<U::Item>>::Notification,
                >>::Handle,
            >,
        >>::Error,
        <<<C::Context as ContextReference<C>>::Target as Notify<U::Item>>::Wrap as Future<
            <C::Context as ContextReference<C>>::Target,
        >>::Error,
        <<<C::Context as ContextReference<C>>::Target as Fork<
            <<C::Context as ContextReference<C>>::Target as Notify<U::Item>>::Notification,
        >>::Future as Future<<C::Context as ContextReference<C>>::Target>>::Error,
        <<<C::Context as ContextReference<C>>::Target as Fork<
            <<C::Context as ContextReference<C>>::Target as Notify<U::Item>>::Notification,
        >>::Target as Future<<C::Context as ContextReference<C>>::Target>>::Error,
        <<<C::Context as ContextReference<C>>::Target as Fork<
            <<C::Context as ContextReference<C>>::Target as Notify<U::Item>>::Notification,
        >>::Finalize as Future<<C::Context as ContextReference<C>>::Target>>::Error,
    >;

    fn poll<R: BorrowMut<C>>(
        mut self: Pin<&mut Self>,
        cx: &mut Context,
        ctx: R,
    ) -> Poll<Result<Self::Ok, Self::Error>> {
        let this = &mut *self;

        let ctx = this.context.with(ctx);

        let mut i = 0;
        while i != this.pending.len() {
            if let Poll::Ready(()) = Pin::new(&mut this.pending[i])
                .poll(cx, &mut *ctx)
                .map_err(StreamUnravelError::Finalize)?
            {
                this.pending.remove(i);
            } else {
                i += 1;
            }
        }

        loop {
            match &mut this.state {
                ErasedStreamUnravelState::Read => {
                    let mut ct = Pin::new(&mut *ctx);
                    let handle = ready!(ct.as_mut().read(cx)).map_err(StreamUnravelError::Read)?;
                    if let Some(handle) = handle {
                        this.state = ErasedStreamUnravelState::Join(Join::<
                            <<C::Context as ContextReference<C>>::Target as Notify<()>>::Notification,
                        >::join(
                            &mut *ctx, handle
                        ));
                    } else {
                        this.state = ErasedStreamUnravelState::Done;
                        return Poll::Ready(Ok(()));
                    }
                }
                ErasedStreamUnravelState::Join(future) => {
                    let notification = ready!(Pin::new(future).poll(cx, &mut *ctx))
                        .map_err(StreamUnravelError::Join)?;
                    this.state = ErasedStreamUnravelState::Unwrap(
                        <<C::Context as ContextReference<C>>::Target as Notify<()>>::unwrap(
                            &mut *ctx,
                            notification,
                        ),
                    );
                }
                ErasedStreamUnravelState::Unwrap(future) => {
                    ready!(Pin::new(future).poll(cx, &mut *ctx))
                        .map_err(StreamUnravelError::Unwrap)?;
                    this.state = ErasedStreamUnravelState::Next;
                }
                ErasedStreamUnravelState::Next => {
                    let item = ready!(Pin::new(&mut this.stream).poll_next(cx));
                    if let Some(item) = item {
                        this.state = ErasedStreamUnravelState::Wrap(ctx.wrap(item));
                    } else {
                        this.state = ErasedStreamUnravelState::WriteNone;
                    }
                }
                ErasedStreamUnravelState::Wrap(future) => {
                    let item = ready!(Pin::new(future).poll(cx, &mut *ctx))
                        .map_err(StreamUnravelError::Notify)?;
                    this.state = ErasedStreamUnravelState::Fork(ctx.fork(item));
                }
                ErasedStreamUnravelState::Fork(future) => {
                    let (target, handle) = ready!(Pin::new(&mut *future).poll(cx, &mut *ctx))
                        .map_err(StreamUnravelError::Dispatch)?;
                    this.state = ErasedStreamUnravelState::Write(target, handle);
                }
                ErasedStreamUnravelState::Write(_, _) => {
                    let mut ctx = Pin::new(&mut *ctx);
                    ready!(ctx.as_mut().poll_ready(cx)).map_err(StreamUnravelError::Transport)?;
                    let data = replace(&mut this.state, ErasedStreamUnravelState::Done);
                    if let ErasedStreamUnravelState::Write(target, data) = data {
                        ctx.write(Some(data))
                            .map_err(StreamUnravelError::Transport)?;
                        this.state = ErasedStreamUnravelState::Flush(target);
                    } else {
                        panic!("invalid state in ErasedStreamUnravel Write")
                    }
                }
                ErasedStreamUnravelState::WriteNone => {
                    let mut ctx = Pin::new(&mut *ctx);
                    ready!(ctx.as_mut().poll_ready(cx)).map_err(StreamUnravelError::Transport)?;
                    ctx.write(None).map_err(StreamUnravelError::Transport)?;
                    this.state = ErasedStreamUnravelState::FlushNone;
                }
                ErasedStreamUnravelState::Flush(_) => {
                    ready!(Pin::new(&mut *ctx).poll_flush(cx))
                        .map_err(StreamUnravelError::Transport)?;
                    let data = replace(&mut this.state, ErasedStreamUnravelState::Done);
                    if let ErasedStreamUnravelState::Flush(target) = data {
                        this.state = ErasedStreamUnravelState::Target(target);
                    } else {
                        panic!("invalid state in ErasedStreamUnravel Write")
                    }
                }
                ErasedStreamUnravelState::FlushNone => {
                    ready!(Pin::new(&mut *ctx).poll_flush(cx))
                        .map_err(StreamUnravelError::Transport)?;
                    this.state = ErasedStreamUnravelState::Done;
                }
                ErasedStreamUnravelState::Target(target) => {
                    let mut finalize = ready!(Pin::new(target).poll(cx, &mut *ctx))
                        .map_err(StreamUnravelError::Target)?;
                    if let Poll::Pending = Pin::new(&mut finalize)
                        .poll(cx, &mut *ctx)
                        .map_err(StreamUnravelError::Finalize)?
                    {
                        this.pending.push(finalize);
                    }
                    this.state = ErasedStreamUnravelState::Read;
                }
                ErasedStreamUnravelState::Done => {
                    if this.pending.len() == 0 {
                        return Poll::Ready(Ok(()));
                    } else {
                        return Poll::Pending;
                    }
                }
            }
        }
    }
}

enum StreamUnravelState<
    C: ?Sized + Write<<C as Contextualize>::Handle> + ReferenceContext,
    U: Stream,
> where
    <C::Context as ContextReference<C>>::Target: Notify<()>
        + Notify<U::Item>
        + Read<
            Option<
                <<C::Context as ContextReference<C>>::Target as Dispatch<
                    <<C::Context as ContextReference<C>>::Target as Notify<()>>::Notification,
                >>::Handle,
            >,
        > + Write<
            Option<
                <<C::Context as ContextReference<C>>::Target as Dispatch<
                    <<C::Context as ContextReference<C>>::Target as Notify<U::Item>>::Notification,
                >>::Handle,
            >,
        >,
    U: Unpin,
    <<C::Context as ContextReference<C>>::Target as Notify<U::Item>>::Wrap: Unpin,
    <<C::Context as ContextReference<C>>::Target as Fork<
        <<C::Context as ContextReference<C>>::Target as Notify<U::Item>>::Notification,
    >>::Target: Unpin,
    <<C::Context as ContextReference<C>>::Target as Fork<
        <<C::Context as ContextReference<C>>::Target as Notify<U::Item>>::Notification,
    >>::Future: Unpin,
    <<C::Context as ContextReference<C>>::Target as Fork<
        <<C::Context as ContextReference<C>>::Target as Notify<U::Item>>::Notification,
    >>::Finalize: Unpin,
    <C::Context as ContextReference<C>>::Target: Unpin,
    <<C::Context as ContextReference<C>>::Target as Notify<()>>::Unwrap: Unpin,
    <<C::Context as ContextReference<C>>::Target as Join<
        <<C::Context as ContextReference<C>>::Target as Notify<()>>::Notification,
    >>::Future: Unpin,
{
    None(PhantomData<U>),
    Context(C::ForkOutput),
    Write(C::Context, C::Handle),
    Flush(C::Context),
    Done,
}

pub struct StreamUnravel<
    C: ?Sized + Write<<C as Contextualize>::Handle> + ReferenceContext,
    U: Stream,
> where
    <C::Context as ContextReference<C>>::Target: Notify<()>
        + Notify<U::Item>
        + Read<
            Option<
                <<C::Context as ContextReference<C>>::Target as Dispatch<
                    <<C::Context as ContextReference<C>>::Target as Notify<()>>::Notification,
                >>::Handle,
            >,
        > + Write<
            Option<
                <<C::Context as ContextReference<C>>::Target as Dispatch<
                    <<C::Context as ContextReference<C>>::Target as Notify<U::Item>>::Notification,
                >>::Handle,
            >,
        >,
    U: Unpin,
    <<C::Context as ContextReference<C>>::Target as Notify<U::Item>>::Wrap: Unpin,
    <<C::Context as ContextReference<C>>::Target as Fork<
        <<C::Context as ContextReference<C>>::Target as Notify<U::Item>>::Notification,
    >>::Target: Unpin,
    <<C::Context as ContextReference<C>>::Target as Fork<
        <<C::Context as ContextReference<C>>::Target as Notify<U::Item>>::Notification,
    >>::Future: Unpin,
    <<C::Context as ContextReference<C>>::Target as Fork<
        <<C::Context as ContextReference<C>>::Target as Notify<U::Item>>::Notification,
    >>::Finalize: Unpin,
    <C::Context as ContextReference<C>>::Target: Unpin,
    <<C::Context as ContextReference<C>>::Target as Notify<()>>::Unwrap: Unpin,
    <<C::Context as ContextReference<C>>::Target as Join<
        <<C::Context as ContextReference<C>>::Target as Notify<()>>::Notification,
    >>::Future: Unpin,
{
    stream: Option<U>,
    context: StreamUnravelState<C, U>,
}

impl<C: ?Sized + Write<<C as Contextualize>::Handle> + ReferenceContext, U: Stream> Unpin
    for StreamUnravel<C, U>
where
    <C::Context as ContextReference<C>>::Target: Notify<()>
        + Notify<U::Item>
        + Read<
            Option<
                <<C::Context as ContextReference<C>>::Target as Dispatch<
                    <<C::Context as ContextReference<C>>::Target as Notify<()>>::Notification,
                >>::Handle,
            >,
        > + Write<
            Option<
                <<C::Context as ContextReference<C>>::Target as Dispatch<
                    <<C::Context as ContextReference<C>>::Target as Notify<U::Item>>::Notification,
                >>::Handle,
            >,
        >,
    U: Unpin,
    <<C::Context as ContextReference<C>>::Target as Notify<U::Item>>::Wrap: Unpin,
    <<C::Context as ContextReference<C>>::Target as Fork<
        <<C::Context as ContextReference<C>>::Target as Notify<U::Item>>::Notification,
    >>::Target: Unpin,
    <<C::Context as ContextReference<C>>::Target as Fork<
        <<C::Context as ContextReference<C>>::Target as Notify<U::Item>>::Notification,
    >>::Future: Unpin,
    <<C::Context as ContextReference<C>>::Target as Fork<
        <<C::Context as ContextReference<C>>::Target as Notify<U::Item>>::Notification,
    >>::Finalize: Unpin,
    <C::Context as ContextReference<C>>::Target: Unpin,
    <<C::Context as ContextReference<C>>::Target as Notify<()>>::Unwrap: Unpin,
    <<C::Context as ContextReference<C>>::Target as Join<
        <<C::Context as ContextReference<C>>::Target as Notify<()>>::Notification,
    >>::Future: Unpin,
{
}

impl<C: ?Sized + Write<<C as Contextualize>::Handle> + ReferenceContext, U: Stream> Future<C>
    for StreamUnravel<C, U>
where
    <C::Context as ContextReference<C>>::Target: Notify<()>
        + Notify<U::Item>
        + Read<
            Option<
                <<C::Context as ContextReference<C>>::Target as Dispatch<
                    <<C::Context as ContextReference<C>>::Target as Notify<()>>::Notification,
                >>::Handle,
            >,
        > + Write<
            Option<
                <<C::Context as ContextReference<C>>::Target as Dispatch<
                    <<C::Context as ContextReference<C>>::Target as Notify<U::Item>>::Notification,
                >>::Handle,
            >,
        >,
    U: Unpin,
    C: Unpin,
    C::ForkOutput: Unpin,
    <<C::Context as ContextReference<C>>::Target as Notify<U::Item>>::Wrap: Unpin,
    <<C::Context as ContextReference<C>>::Target as Fork<
        <<C::Context as ContextReference<C>>::Target as Notify<U::Item>>::Notification,
    >>::Target: Unpin,
    <<C::Context as ContextReference<C>>::Target as Fork<
        <<C::Context as ContextReference<C>>::Target as Notify<U::Item>>::Notification,
    >>::Future: Unpin,
    <<C::Context as ContextReference<C>>::Target as Fork<
        <<C::Context as ContextReference<C>>::Target as Notify<U::Item>>::Notification,
    >>::Finalize: Unpin,
    <C::Context as ContextReference<C>>::Target: Unpin,
    <<C::Context as ContextReference<C>>::Target as Notify<()>>::Unwrap: Unpin,
    <<C::Context as ContextReference<C>>::Target as Join<
        <<C::Context as ContextReference<C>>::Target as Notify<()>>::Notification,
    >>::Future: Unpin,
{
    type Ok = ErasedStreamUnravel<C, U>;
    type Error = StreamUnravelError<
        <<C::Context as ContextReference<C>>::Target as Read<
            Option<
                <<C::Context as ContextReference<C>>::Target as Dispatch<
                    <<C::Context as ContextReference<C>>::Target as Notify<()>>::Notification,
                >>::Handle,
            >,
        >>::Error,
        <<<C::Context as ContextReference<C>>::Target as Notify<()>>::Unwrap as Future<
            <C::Context as ContextReference<C>>::Target,
        >>::Error,
        <<<C::Context as ContextReference<C>>::Target as Join<
            <<C::Context as ContextReference<C>>::Target as Notify<()>>::Notification,
        >>::Future as Future<<C::Context as ContextReference<C>>::Target>>::Error,
        <C::ForkOutput as Future<C>>::Error,
        C::Error,
        <<C::Context as ContextReference<C>>::Target as Write<
            Option<
                <<C::Context as ContextReference<C>>::Target as Dispatch<
                    <<C::Context as ContextReference<C>>::Target as Notify<U::Item>>::Notification,
                >>::Handle,
            >,
        >>::Error,
        <<<C::Context as ContextReference<C>>::Target as Notify<U::Item>>::Wrap as Future<
            <C::Context as ContextReference<C>>::Target,
        >>::Error,
        <<<C::Context as ContextReference<C>>::Target as Fork<
            <<C::Context as ContextReference<C>>::Target as Notify<U::Item>>::Notification,
        >>::Future as Future<<C::Context as ContextReference<C>>::Target>>::Error,
        <<<C::Context as ContextReference<C>>::Target as Fork<
            <<C::Context as ContextReference<C>>::Target as Notify<U::Item>>::Notification,
        >>::Target as Future<<C::Context as ContextReference<C>>::Target>>::Error,
        <<<C::Context as ContextReference<C>>::Target as Fork<
            <<C::Context as ContextReference<C>>::Target as Notify<U::Item>>::Notification,
        >>::Finalize as Future<<C::Context as ContextReference<C>>::Target>>::Error,
    >;

    fn poll<R: BorrowMut<C>>(
        mut self: Pin<&mut Self>,
        cx: &mut Context,
        mut ctx: R,
    ) -> Poll<Result<Self::Ok, Self::Error>> {
        let this = &mut *self;

        loop {
            match &mut this.context {
                StreamUnravelState::None(_) => {
                    this.context = StreamUnravelState::Context(ctx.borrow_mut().fork_ref())
                }
                StreamUnravelState::Context(future) => {
                    let (context, handle) = ready!(Pin::new(future).poll(cx, ctx.borrow_mut()))
                        .map_err(StreamUnravelError::Contextualize)?;
                    this.context = StreamUnravelState::Write(context, handle);
                }
                StreamUnravelState::Write(_, _) => {
                    let mut ctx = Pin::new(ctx.borrow_mut());
                    ready!(ctx.as_mut().poll_ready(cx)).map_err(StreamUnravelError::Write)?;
                    let data = replace(&mut this.context, StreamUnravelState::None(PhantomData));
                    if let StreamUnravelState::Write(context, handle) = data {
                        ctx.write(handle).map_err(StreamUnravelError::Write)?;
                        this.context = StreamUnravelState::Flush(context);
                    } else {
                        panic!("invalid state")
                    }
                }
                StreamUnravelState::Flush(_) => {
                    let mut ctx = Pin::new(ctx.borrow_mut());
                    ready!(ctx.as_mut().poll_flush(cx)).map_err(StreamUnravelError::Write)?;
                    let data = replace(&mut this.context, StreamUnravelState::None(PhantomData));
                    if let StreamUnravelState::Flush(context) = data {
                        this.context = StreamUnravelState::Done;
                        return Poll::Ready(Ok(ErasedStreamUnravel {
                            context,
                            pending: vec![],
                            stream: this.stream.take().unwrap(),
                            state: ErasedStreamUnravelState::Read,
                        }));
                    } else {
                        panic!("invalid state")
                    }
                }
                StreamUnravelState::Done => panic!("StreamUnravel polled after completion"),
            }
        }
    }
}

pub enum ErasedStreamCoalesceState<
    U,
    C: Notify<()>
        + Notify<U>
        + Write<Option<<C as Dispatch<<C as Notify<()>>::Notification>>::Handle>>
        + Read<Option<<C as Dispatch<<C as Notify<U>>::Notification>>::Handle>>
        + Finalize<<C as Fork<<C as Notify<()>>::Notification>>::Finalize>,
> where
    <C as Fork<<C as Notify<()>>::Notification>>::Finalize:
        Future<<C as Finalize<<C as Fork<<C as Notify<()>>::Notification>>::Finalize>>::Target>,
{
    Begin,
    Wrap(<C as Notify<()>>::Wrap),
    Fork(<C as Fork<<C as Notify<()>>::Notification>>::Future),
    Write(
        <C as Fork<<C as Notify<()>>::Notification>>::Target,
        <C as Dispatch<<C as Notify<()>>::Notification>>::Handle,
    ),
    Flush(<C as Fork<<C as Notify<()>>::Notification>>::Target),
    Target(<C as Fork<<C as Notify<()>>::Notification>>::Target),
    Finalize(<C as Finalize<<C as Fork<<C as Notify<()>>::Notification>>::Finalize>>::Output),
    Read,
    Join(<C as Join<<C as Notify<U>>::Notification>>::Future),
    Unwrap(<C as Notify<U>>::Unwrap),
    Done,
}

pub struct ErasedStreamCoalesce<
    U,
    C: Notify<()>
        + FinalizeImmediate<ErasedStreamComplete>
        + Notify<U>
        + Write<Option<<C as Dispatch<<C as Notify<()>>::Notification>>::Handle>>
        + Read<Option<<C as Dispatch<<C as Notify<U>>::Notification>>::Handle>>
        + Finalize<<C as Fork<<C as Notify<()>>::Notification>>::Finalize>,
> where
    <C as Fork<<C as Notify<()>>::Notification>>::Finalize:
        Future<<C as Finalize<<C as Fork<<C as Notify<()>>::Notification>>::Finalize>>::Target>,
    <C as FinalizeImmediate<ErasedStreamComplete>>::Target: Unpin
        + Notify<()>
        + Write<
            Option<
                <<C as FinalizeImmediate<ErasedStreamComplete>>::Target as Dispatch<
                    <<C as FinalizeImmediate<ErasedStreamComplete>>::Target as Notify<()>>::Notification,
                >>::Handle,
            >,
        >,
{
    state: ErasedStreamCoalesceState<U, C>,
    context: C,
}

pub enum ErasedStreamComplete {
    Write,
    Flush,
    Done,
}

impl<
    C: Unpin + Notify<()> + Write<Option<<C as Dispatch<<C as Notify<()>>::Notification>>::Handle>>,
> Future<C> for ErasedStreamComplete
{
    type Ok = ();
    type Error = C::Error;

    fn poll<R: BorrowMut<C>>(
        mut self: Pin<&mut Self>,
        cx: &mut Context,
        mut ctx: R,
    ) -> Poll<Result<Self::Ok, Self::Error>> {
        let this = &mut *self;

        loop {
            match this {
                ErasedStreamComplete::Write => {
                    let mut ctx = Pin::new(ctx.borrow_mut());
                    ready!(ctx.as_mut().poll_ready(cx))?;
                    ctx.write(None)?;
                    *this = ErasedStreamComplete::Flush;
                }
                ErasedStreamComplete::Flush => {
                    ready!(Pin::new(ctx.borrow_mut()).poll_flush(cx))?;
                    *this = ErasedStreamComplete::Done;
                    return Poll::Ready(Ok(()));
                }
                ErasedStreamComplete::Done => {
                    panic!("erased Stream terminator polled after completion")
                }
            }
        }
    }
}

impl<
    U,
    C: FinalizeImmediate<ErasedStreamComplete>
        + Notify<()>
        + Notify<U>
        + Write<Option<<C as Dispatch<<C as Notify<()>>::Notification>>::Handle>>
        + Read<Option<<C as Dispatch<<C as Notify<U>>::Notification>>::Handle>>
        + Finalize<<C as Fork<<C as Notify<()>>::Notification>>::Finalize>,
> Drop for ErasedStreamCoalesce<U, C>
where
    <C as FinalizeImmediate<ErasedStreamComplete>>::Target: Unpin
        + Notify<()>
        + Write<
            Option<
                <<C as FinalizeImmediate<ErasedStreamComplete>>::Target as Dispatch<
                    <<C as FinalizeImmediate<ErasedStreamComplete>>::Target as Notify<()>>::Notification,
                >>::Handle,
            >,
        >,
    <C as Fork<<C as Notify<()>>::Notification>>::Finalize:
        Future<<C as Finalize<<C as Fork<<C as Notify<()>>::Notification>>::Finalize>>::Target>,
{
    fn drop(&mut self) {
        let _ = FinalizeImmediate::finalize(&mut self.context, ErasedStreamComplete::Write);
    }
}

impl<
    U,
    C: Notify<()>
        + Notify<U>
        + Write<Option<<C as Dispatch<<C as Notify<()>>::Notification>>::Handle>>
        + Read<Option<<C as Dispatch<<C as Notify<U>>::Notification>>::Handle>>
        + Finalize<<C as Fork<<C as Notify<()>>::Notification>>::Finalize>
        + FinalizeImmediate<ErasedStreamComplete>,
> Unpin for ErasedStreamCoalesce<U, C>
where
    <C as Fork<<C as Notify<()>>::Notification>>::Finalize:
        Future<<C as Finalize<<C as Fork<<C as Notify<()>>::Notification>>::Finalize>>::Target>,
    <C as FinalizeImmediate<ErasedStreamComplete>>::Target: Unpin
        + Notify<()>
        + Write<
            Option<
                <<C as FinalizeImmediate<ErasedStreamComplete>>::Target as Dispatch<
                    <<C as FinalizeImmediate<ErasedStreamComplete>>::Target as Notify<()>>::Notification,
                >>::Handle,
            >,
        >,
{
}

#[derive(Debug, Error)]
#[bounds(
    where
        A: Error + 'static,
        T: Error + 'static,
        B: Error + 'static,
        I: Error + 'static,
        U: Error + 'static,
        V: Error + 'static,
        W: Error + 'static,
        C: Error + 'static,
)]
pub enum ErasedStreamCoalesceError<A, C, B, T, I, U, V, W> {
    #[error("failed to write argument handle for erased Stream: {0}")]
    Write(#[source] A),
    #[error("failed to fork argument for erased Stream: {0}")]
    Fork(#[source] C),
    #[error("failed to wrap argument notification for erased Stream: {0}")]
    Wrap(#[source] B),
    #[error("failed to read handle for erased Stream return: {0}")]
    Read(#[source] T),
    #[error("failed to create notification wrapper for erased Stream return: {0}")]
    Unwrap(#[source] I),
    #[error("failed to join erased Stream return: {0}")]
    Join(#[source] U),
    #[error("failed to target erased Stream argument: {0}")]
    Target(#[source] V),
    #[error("failed to finalize erased Stream argument: {0}")]
    Finalize(#[source] W),
}

type CoalesceError<U, C> = ErasedStreamCoalesceError<
    <C as Write<Option<<C as Dispatch<<C as Notify<()>>::Notification>>::Handle>>>::Error,
    <<C as Fork<<C as Notify<()>>::Notification>>::Future as Future<C>>::Error,
    <<C as Notify<()>>::Wrap as Future<C>>::Error,
    <C as Read<Option<<C as Dispatch<<C as Notify<U>>::Notification>>::Handle>>>::Error,
    <<C as Notify<U>>::Unwrap as Future<C>>::Error,
    <<C as Join<<C as Notify<U>>::Notification>>::Future as Future<C>>::Error,
    <<C as Fork<<C as Notify<()>>::Notification>>::Target as Future<C>>::Error,
    <<C as Finalize<<C as Fork<<C as Notify<()>>::Notification>>::Finalize>>::Output as Future<
        C,
    >>::Error,
>;

impl<
    U,
    C: Notify<()>
        + Notify<U>
        + Write<Option<<C as Dispatch<<C as Notify<()>>::Notification>>::Handle>>
        + Read<Option<<C as Dispatch<<C as Notify<U>>::Notification>>::Handle>>
        + Finalize<<C as Fork<<C as Notify<()>>::Notification>>::Finalize>
        + FinalizeImmediate<ErasedStreamComplete>,
> Stream for ErasedStreamCoalesce<U, C>
where
    <C as Notify<()>>::Wrap: Unpin,
    <C as Notify<U>>::Unwrap: Unpin,
    <C as FinalizeImmediate<ErasedStreamComplete>>::Target: Unpin
        + Notify<()>
        + Write<
            Option<
                <<C as FinalizeImmediate<ErasedStreamComplete>>::Target as Dispatch<
                    <<C as FinalizeImmediate<ErasedStreamComplete>>::Target as Notify<()>>::Notification,
                >>::Handle,
            >,
        >,
    C: Unpin,
    <C as Fork<<C as Notify<()>>::Notification>>::Future: Unpin,
    <C as Fork<<C as Notify<()>>::Notification>>::Target: Unpin,
    <C as Fork<<C as Notify<()>>::Notification>>::Finalize:
        Future<<C as Finalize<<C as Fork<<C as Notify<()>>::Notification>>::Finalize>>::Target>,
    <C as Finalize<<C as Fork<<C as Notify<()>>::Notification>>::Finalize>>::Output: Unpin,
    <C as Write<Option<<C as Dispatch<<C as Notify<()>>::Notification>>::Handle>>>::Error:
        Error + Send + 'static,
    <<C as Fork<<C as Notify<()>>::Notification>>::Future as Future<C>>::Error:
        Error + Send + 'static,
    <<C as Notify<()>>::Wrap as Future<C>>::Error: Error + Send + 'static,
    <C as Read<Option<<C as Dispatch<<C as Notify<U>>::Notification>>::Handle>>>::Error:
        Error + Send + 'static,
    <<C as Notify<U>>::Unwrap as Future<C>>::Error: Error + Send + 'static,
    <C as Join<<C as Notify<U>>::Notification>>::Future: Unpin,
    <<C as Join<<C as Notify<U>>::Notification>>::Future as Future<C>>::Error:
        Error + Send + 'static,
    <<C as Fork<<C as Notify<()>>::Notification>>::Target as Future<C>>::Error:
        Error + Send + 'static,
    <<C as Finalize<<C as Fork<<C as Notify<()>>::Notification>>::Finalize>>::Output as Future<
        C,
    >>::Error: Error + Send + 'static,
{
    type Item = Result<U, ProtocolError>;

    fn poll_next(
        mut self: Pin<&mut Self>,
        cx: &mut Context,
    ) -> Poll<Option<Result<U, ProtocolError>>> {
        let this = &mut *self;
        let ctx = this.context.borrow_mut();

        loop {
            match &mut this.state {
                ErasedStreamCoalesceState::Begin => {
                    this.state = ErasedStreamCoalesceState::Wrap(ctx.wrap(()));
                }
                ErasedStreamCoalesceState::Wrap(future) => {
                    let wrapped = ready!(Pin::new(future).poll(cx, &mut *ctx))
                        .map_err(|e| ProtocolError::new(CoalesceError::<U, C>::Wrap(e)))?;
                    this.state = ErasedStreamCoalesceState::Fork(ctx.fork(wrapped));
                }
                ErasedStreamCoalesceState::Fork(future) => {
                    let (target, handle) = ready!(Pin::new(future).poll(cx, &mut *ctx))
                        .map_err(|e| ProtocolError::new(CoalesceError::<U, C>::Fork(e)))?;
                    this.state = ErasedStreamCoalesceState::Write(target, handle);
                }
                ErasedStreamCoalesceState::Write(_, _) => {
                    let mut ctx = Pin::new(&mut *ctx);
                    ready!(ctx.as_mut().poll_ready(cx))
                        .map_err(|e| ProtocolError::new(CoalesceError::<U, C>::Write(e)))?;
                    let data = replace(&mut this.state, ErasedStreamCoalesceState::Done);
                    if let ErasedStreamCoalesceState::Write(target, data) = data {
                        ctx.write(Some(data))
                            .map_err(|e| ProtocolError::new(CoalesceError::<U, C>::Write(e)))?;
                        this.state = ErasedStreamCoalesceState::Flush(target);
                    } else {
                        panic!("invalid state in ErasedStreamCoalesce Write")
                    }
                }
                ErasedStreamCoalesceState::Flush(_) => {
                    ready!(Pin::new(&mut *ctx).poll_flush(cx))
                        .map_err(|e| ProtocolError::new(CoalesceError::<U, C>::Write(e)))?;
                    let data = replace(&mut this.state, ErasedStreamCoalesceState::Done);
                    if let ErasedStreamCoalesceState::Flush(target) = data {
                        this.state = ErasedStreamCoalesceState::Target(target);
                    } else {
                        panic!("invalid state in ErasedStreamUnravel Write")
                    }
                }
                ErasedStreamCoalesceState::Target(target) => {
                    let finalize = ready!(Pin::new(target).poll(cx, &mut *ctx))
                        .map_err(|e| ProtocolError::new(CoalesceError::<U, C>::Target(e)))?;
                    this.state =
                        ErasedStreamCoalesceState::Finalize(Finalize::finalize(ctx, finalize));
                }
                ErasedStreamCoalesceState::Finalize(future) => {
                    ready!(Pin::new(future).poll(cx, &mut *ctx))
                        .map_err(|e| ProtocolError::new(CoalesceError::<U, C>::Finalize(e)))?;
                    this.state = ErasedStreamCoalesceState::Read;
                }
                ErasedStreamCoalesceState::Read => {
                    let handle: Option<<C as Dispatch<<C as Notify<U>>::Notification>>::Handle> =
                        ready!(Pin::new(&mut *ctx).read(cx))
                            .map_err(|e| ProtocolError::new(CoalesceError::<U, C>::Read(e)))?;
                    if let Some(handle) = handle {
                        let join: <C as Join<<C as Notify<U>>::Notification>>::Future =
                            Join::<<C as Notify<U>>::Notification>::join(ctx, handle);
                        this.state = ErasedStreamCoalesceState::Join(join);
                    } else {
                        this.state = ErasedStreamCoalesceState::Done;
                        return Poll::Ready(None);
                    }
                }
                ErasedStreamCoalesceState::Join(future) => {
                    let wrapped = ready!(Pin::new(future).poll(cx, &mut *ctx))
                        .map_err(|e| ProtocolError::new(CoalesceError::<U, C>::Join(e)))?;
                    let unwrap: <C as Notify<U>>::Unwrap = Notify::<U>::unwrap(ctx, wrapped);
                    this.state = ErasedStreamCoalesceState::Unwrap(unwrap);
                }
                ErasedStreamCoalesceState::Unwrap(future) => {
                    let data = ready!(Pin::new(future).poll(cx, &mut *ctx))
                        .map_err(|e| ProtocolError::new(CoalesceError::<U, C>::Unwrap(e)))?;
                    this.state = ErasedStreamCoalesceState::Begin;
                    return Poll::Ready(Some(Ok(data)));
                }
                ErasedStreamCoalesceState::Done => {
                    panic!("erased Stream coalesce polled after completion")
                }
            }
        }
    }
}

pub enum StreamCoalesceState<T> {
    Read,
    Contextualize(T),
    Done,
}

#[derive(Debug, Error)]
#[bounds(
    where
        T: Error + 'static,
        E: Error + 'static,
)]
pub enum StreamCoalesceError<E, T> {
    #[error("failed to read handle for erased Stream context: {0}")]
    Read(T),
    #[error("failed to contextualize erased Stream content: {0}")]
    Contextualize(E),
}

pub struct StreamCoalesce<
    'a,
    O,
    P: Fn(ErasedStreamCoalesce<U, C::Context>) -> O,
    U,
    C: ?Sized + CloneContext,
> where
    C::Context: Unpin
        + Read<Option<<C::Context as Dispatch<<C::Context as Notify<U>>::Notification>>::Handle>>
        + Notify<()>
        + Notify<U>
        + Write<Option<<C::Context as Dispatch<<C::Context as Notify<()>>::Notification>>::Handle>>
        + Finalize<<C::Context as Fork<<C::Context as Notify<()>>::Notification>>::Finalize>
        + FinalizeImmediate<ErasedStreamComplete>,
    <C::Context as FinalizeImmediate<ErasedStreamComplete>>::Target: Unpin
        + Notify<()>
        + Write<
            Option<
                <<C::Context as FinalizeImmediate<ErasedStreamComplete>>::Target as Dispatch<
                    <<C::Context as FinalizeImmediate<ErasedStreamComplete>>::Target as Notify<
                        (),
                    >>::Notification,
                >>::Handle,
            >,
        >,
    <C::Context as Notify<()>>::Wrap: Unpin,
    <C::Context as Notify<U>>::Unwrap: Unpin,
    C::Context: Unpin,
    <C::Context as Fork<<C::Context as Notify<()>>::Notification>>::Future: Unpin,
    <C::Context as Fork<<C::Context as Notify<()>>::Notification>>::Target: Unpin,
    <C::Context as Fork<<C::Context as Notify<()>>::Notification>>::Finalize: Future<
        <C::Context as Finalize<
            <C::Context as Fork<<C::Context as Notify<()>>::Notification>>::Finalize,
        >>::Target,
    >,
    <C::Context as Finalize<
        <C::Context as Fork<<C::Context as Notify<()>>::Notification>>::Finalize,
    >>::Output: Unpin,
    <C::Context as Write<
        Option<<C::Context as Dispatch<<C::Context as Notify<()>>::Notification>>::Handle>,
    >>::Error: Error + Send + 'static,
    <<C::Context as Fork<<C::Context as Notify<()>>::Notification>>::Future as Future<
        C::Context,
    >>::Error: Error + Send + 'static,
    <<C::Context as Notify<()>>::Wrap as Future<C::Context>>::Error: Error + Send + 'static,
    <C::Context as Read<
        Option<<C::Context as Dispatch<<C::Context as Notify<U>>::Notification>>::Handle>,
    >>::Error: Error + Send + 'static,
    <<C::Context as Notify<U>>::Unwrap as Future<C::Context>>::Error: Error + Send + 'static,
    <C::Context as Join<<C::Context as Notify<U>>::Notification>>::Future: Unpin,
    <<C::Context as Join<<C::Context as Notify<U>>::Notification>>::Future as Future<C::Context>>::Error:
        Error + Send + 'static,
    <<C::Context as Fork<<C::Context as Notify<()>>::Notification>>::Target as Future<
        C::Context,
    >>::Error: Error + Send + 'static,
    <<C::Context as Finalize<
        <C::Context as Fork<<C::Context as Notify<()>>::Notification>>::Finalize,
    >>::Output as Future<C::Context>>::Error: Error + Send + 'static,
{
    conv: P,
    lifetime: PhantomData<&'a (O, U)>,
    state: StreamCoalesceState<C::JoinOutput>,
}

impl<'a, O, P: Fn(ErasedStreamCoalesce<U, C::Context>) -> O, U, C: ?Sized + CloneContext> Unpin
    for StreamCoalesce<'a, O, P, U, C>
where
    C::Context: Unpin
        + Read<Option<<C::Context as Dispatch<<C::Context as Notify<U>>::Notification>>::Handle>>
        + Notify<()>
        + Notify<U>
        + Write<Option<<C::Context as Dispatch<<C::Context as Notify<()>>::Notification>>::Handle>>
        + Finalize<<C::Context as Fork<<C::Context as Notify<()>>::Notification>>::Finalize>
        + FinalizeImmediate<ErasedStreamComplete>,
    <C::Context as Notify<()>>::Wrap: Unpin,
    <C::Context as FinalizeImmediate<ErasedStreamComplete>>::Target: Unpin
        + Notify<()>
        + Write<
            Option<
                <<C::Context as FinalizeImmediate<ErasedStreamComplete>>::Target as Dispatch<
                    <<C::Context as FinalizeImmediate<ErasedStreamComplete>>::Target as Notify<
                        (),
                    >>::Notification,
                >>::Handle,
            >,
        >,
    <C::Context as Notify<U>>::Unwrap: Unpin,
    C::Context: Unpin,
    <C::Context as Fork<<C::Context as Notify<()>>::Notification>>::Future: Unpin,
    <C::Context as Fork<<C::Context as Notify<()>>::Notification>>::Target: Unpin,
    <C::Context as Fork<<C::Context as Notify<()>>::Notification>>::Finalize: Future<
        <C::Context as Finalize<
            <C::Context as Fork<<C::Context as Notify<()>>::Notification>>::Finalize,
        >>::Target,
    >,
    <C::Context as Finalize<
        <C::Context as Fork<<C::Context as Notify<()>>::Notification>>::Finalize,
    >>::Output: Unpin,
    <C::Context as Write<
        Option<<C::Context as Dispatch<<C::Context as Notify<()>>::Notification>>::Handle>,
    >>::Error: Error + Send + 'static,
    <<C::Context as Fork<<C::Context as Notify<()>>::Notification>>::Future as Future<
        C::Context,
    >>::Error: Error + Send + 'static,
    <<C::Context as Notify<()>>::Wrap as Future<C::Context>>::Error: Error + Send + 'static,
    <C::Context as Read<
        Option<<C::Context as Dispatch<<C::Context as Notify<U>>::Notification>>::Handle>,
    >>::Error: Error + Send + 'static,
    <<C::Context as Notify<U>>::Unwrap as Future<C::Context>>::Error: Error + Send + 'static,
    <C::Context as Join<<C::Context as Notify<U>>::Notification>>::Future: Unpin,
    <<C::Context as Join<<C::Context as Notify<U>>::Notification>>::Future as Future<C::Context>>::Error:
        Error + Send + 'static,
    <<C::Context as Fork<<C::Context as Notify<()>>::Notification>>::Target as Future<
        C::Context,
    >>::Error: Error + Send + 'static,
    <<C::Context as Finalize<
        <C::Context as Fork<<C::Context as Notify<()>>::Notification>>::Finalize,
    >>::Output as Future<C::Context>>::Error: Error + Send + 'static,
{
}

impl<
    'a,
    O,
    P: Fn(ErasedStreamCoalesce<U, C::Context>) -> O,
    U,
    C: ?Sized + Read<<C as Contextualize>::Handle> + CloneContext,
> Future<C> for StreamCoalesce<'a, O, P, U, C>
where
    C: Unpin,
    C::JoinOutput: Unpin,
    C::Context: Unpin
        + Read<Option<<C::Context as Dispatch<<C::Context as Notify<U>>::Notification>>::Handle>>
        + Notify<()>
        + Notify<U>
        + Write<Option<<C::Context as Dispatch<<C::Context as Notify<()>>::Notification>>::Handle>>
        + Finalize<<C::Context as Fork<<C::Context as Notify<()>>::Notification>>::Finalize>
        + FinalizeImmediate<ErasedStreamComplete>,
    <C::Context as Notify<()>>::Wrap: Unpin,
    <C::Context as Notify<U>>::Unwrap: Unpin,
    C::Context: Unpin,
    <C::Context as Fork<<C::Context as Notify<()>>::Notification>>::Future: Unpin,
    <C::Context as Fork<<C::Context as Notify<()>>::Notification>>::Target: Unpin,
    <C::Context as FinalizeImmediate<ErasedStreamComplete>>::Target: Unpin
        + Notify<()>
        + Write<
            Option<
                <<C::Context as FinalizeImmediate<ErasedStreamComplete>>::Target as Dispatch<
                    <<C::Context as FinalizeImmediate<ErasedStreamComplete>>::Target as Notify<
                        (),
                    >>::Notification,
                >>::Handle,
            >,
        >,
    <C::Context as Fork<<C::Context as Notify<()>>::Notification>>::Finalize: Future<
        <C::Context as Finalize<
            <C::Context as Fork<<C::Context as Notify<()>>::Notification>>::Finalize,
        >>::Target,
    >,
    <C::Context as Finalize<
        <C::Context as Fork<<C::Context as Notify<()>>::Notification>>::Finalize,
    >>::Output: Unpin,
    <C::Context as Write<
        Option<<C::Context as Dispatch<<C::Context as Notify<()>>::Notification>>::Handle>,
    >>::Error: Error + Send + 'static,
    <<C::Context as Fork<<C::Context as Notify<()>>::Notification>>::Future as Future<
        C::Context,
    >>::Error: Error + Send + 'static,
    <<C::Context as Notify<()>>::Wrap as Future<C::Context>>::Error: Error + Send + 'static,
    <C::Context as Read<
        Option<<C::Context as Dispatch<<C::Context as Notify<U>>::Notification>>::Handle>,
    >>::Error: Error + Send + 'static,
    <<C::Context as Notify<U>>::Unwrap as Future<C::Context>>::Error: Error + Send + 'static,
    <C::Context as Join<<C::Context as Notify<U>>::Notification>>::Future: Unpin,
    <<C::Context as Join<<C::Context as Notify<U>>::Notification>>::Future as Future<C::Context>>::Error:
        Error + Send + 'static,
    <<C::Context as Fork<<C::Context as Notify<()>>::Notification>>::Target as Future<
        C::Context,
    >>::Error: Error + Send + 'static,
    <<C::Context as Finalize<
        <C::Context as Fork<<C::Context as Notify<()>>::Notification>>::Finalize,
    >>::Output as Future<C::Context>>::Error: Error + Send + 'static,
{
    type Ok = O;
    type Error = StreamCoalesceError<<C::JoinOutput as Future<C>>::Error, C::Error>;

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
            match &mut this.state {
                StreamCoalesceState::Read => {
                    let ct = Pin::new(&mut *ctx);
                    let handle = ready!(ct.read(cx)).map_err(StreamCoalesceError::Read)?;
                    this.state = StreamCoalesceState::Contextualize(ctx.join_owned(handle));
                }
                StreamCoalesceState::Contextualize(future) => {
                    let context = ready!(Pin::new(future).poll(cx, &mut *ctx))
                        .map_err(StreamCoalesceError::Contextualize)?;
                    replace(&mut this.state, StreamCoalesceState::Done);
                    return Poll::Ready(Ok((this.conv)(ErasedStreamCoalesce {
                        context,
                        state: ErasedStreamCoalesceState::Begin,
                    })));
                }
                StreamCoalesceState::Done => panic!("StreamCoalesce polled after completion"),
            }
        }
    }
}

macro_rules! marker_variants {
    ($(
        $($marker:ident)*
    ),+) => {
        $(
            impl<
                'a,
                C: ?Sized + Read<<C as Contextualize>::Handle> + CloneContext,
                U: FromError<ProtocolError> + 'a,
            > Coalesce<C> for Pin<Box<dyn Stream<Item = U> + 'a $(+ $marker)*>>
            where
                C: Unpin,
                ErasedStreamCoalesce<U, C::Context>: 'a $(+ $marker)*,
                C::JoinOutput: Unpin,
                C::Context: Unpin
                    + Read<Option<<C::Context as Dispatch<<C::Context as Notify<U>>::Notification>>::Handle>>
                    + Notify<()>
                    + Notify<U>
                    + Write<Option<<C::Context as Dispatch<<C::Context as Notify<()>>::Notification>>::Handle>>
                    + Finalize<<C::Context as Fork<<C::Context as Notify<()>>::Notification>>::Finalize>
                    + FinalizeImmediate<ErasedStreamComplete>
                    $(+ $marker)*,
                <C::Context as Notify<()>>::Wrap: Unpin,
                <C::Context as Notify<U>>::Unwrap: Unpin,
                C::Context: Unpin + 'a,
                <C::Context as Fork<<C::Context as Notify<()>>::Notification>>::Future: Unpin,
                <C::Context as Fork<<C::Context as Notify<()>>::Notification>>::Target: Unpin,
                <C::Context as Fork<<C::Context as Notify<()>>::Notification>>::Finalize: Future<
                    <C::Context as Finalize<
                        <C::Context as Fork<<C::Context as Notify<()>>::Notification>>::Finalize,
                    >>::Target,
                >,
                <C::Context as FinalizeImmediate<ErasedStreamComplete>>::Target: Unpin
                    + Notify<()>
                    + Write<
                        Option<
                            <<C::Context as FinalizeImmediate<ErasedStreamComplete>>::Target as Dispatch<
                                <<C::Context as FinalizeImmediate<ErasedStreamComplete>>::Target as Notify<
                                    (),
                                >>::Notification,
                            >>::Handle,
                        >,
                    >,
                <C::Context as Finalize<
                    <C::Context as Fork<<C::Context as Notify<()>>::Notification>>::Finalize,
                >>::Output: Unpin,
                <C::Context as Write<
                    Option<<C::Context as Dispatch<<C::Context as Notify<()>>::Notification>>::Handle>,
                >>::Error: Error + Send + 'static,
                <<C::Context as Fork<<C::Context as Notify<()>>::Notification>>::Future as Future<
                    C::Context,
                >>::Error: Error + Send + 'static,
                <<C::Context as Notify<()>>::Wrap as Future<C::Context>>::Error: Error + Send + 'static,
                <C::Context as Read<
                    Option<<C::Context as Dispatch<<C::Context as Notify<U>>::Notification>>::Handle>,
                >>::Error: Error + Send + 'static,
                <<C::Context as Notify<U>>::Unwrap as Future<C::Context>>::Error: Error + Send + 'static,
                <C::Context as Join<<C::Context as Notify<U>>::Notification>>::Future: Unpin,
                <<C::Context as Join<<C::Context as Notify<U>>::Notification>>::Future as Future<C::Context>>::Error:
                    Error + Send + 'static,
                <<C::Context as Fork<<C::Context as Notify<()>>::Notification>>::Target as Future<
                    C::Context,
                >>::Error: Error + Send + 'static,
                <<C::Context as Finalize<
                    <C::Context as Fork<<C::Context as Notify<()>>::Notification>>::Finalize,
                >>::Output as Future<C::Context>>::Error: Error + Send + 'static,
            {
                type Future = StreamCoalesce<'a, Self, fn(ErasedStreamCoalesce<U, C::Context>) -> Self, U, C>;

                fn coalesce() -> Self::Future {
                    StreamCoalesce {
                        lifetime: PhantomData,
                        state: StreamCoalesceState::Read,
                        conv: |stream| Box::pin(stream.map(|item| item.unwrap_or_else(|e| U::from_error(e)))),
                    }
                }
            }

            impl<'a, E, T: future::Future<Output = Result<Self, E>> + 'a $(+ $marker)*, U: FromError<E> $(+ $marker)*> Flatten<E, T>
                for Pin<Box<dyn Stream<Item = U> + 'a $(+ $marker)*>>
            {
                fn flatten(future: T) -> Self {
                    Box::pin(
                        future
                            .map(|out| match out {
                                Err(e) => Either::Left(once(ready(U::from_error(e)))),
                                Ok(item) => Either::Right(item),
                            })
                            .into_stream()
                            .flatten(),
                    )
                }
            }

            impl<'a, U: 'a, C: ?Sized + Write<<C as Contextualize>::Handle> + ReferenceContext + Unpin>
                Unravel<C> for Pin<Box<dyn Stream<Item = U> + 'a $(+ $marker)*>>
            where
                <C::Context as ContextReference<C>>::Target: Notify<()>
                    + Notify<U>
                    + Read<
                        Option<
                            <<C::Context as ContextReference<C>>::Target as Dispatch<
                                <<C::Context as ContextReference<C>>::Target as Notify<()>>::Notification,
                            >>::Handle,
                        >,
                    > + Write<
                        Option<
                            <<C::Context as ContextReference<C>>::Target as Dispatch<
                                <<C::Context as ContextReference<C>>::Target as Notify<U>>::Notification,
                            >>::Handle,
                        >,
                    >,
                U: Unpin,
                C: Unpin,
                C::ForkOutput: Unpin,
                <<C::Context as ContextReference<C>>::Target as Notify<U>>::Wrap: Unpin,
                <<C::Context as ContextReference<C>>::Target as Fork<
                    <<C::Context as ContextReference<C>>::Target as Notify<U>>::Notification,
                >>::Target: Unpin,
                <<C::Context as ContextReference<C>>::Target as Fork<
                    <<C::Context as ContextReference<C>>::Target as Notify<U>>::Notification,
                >>::Future: Unpin,
                <<C::Context as ContextReference<C>>::Target as Fork<
                    <<C::Context as ContextReference<C>>::Target as Notify<U>>::Notification,
                >>::Finalize: Unpin,
                <C::Context as ContextReference<C>>::Target: Unpin,
                <<C::Context as ContextReference<C>>::Target as Notify<()>>::Unwrap: Unpin,
                <<C::Context as ContextReference<C>>::Target as Join<
                    <<C::Context as ContextReference<C>>::Target as Notify<()>>::Notification,
                >>::Future: Unpin,
            {
                type Finalize = ErasedStreamUnravel<C, Self>;
                type Target = StreamUnravel<C, Self>;

                fn unravel(self) -> Self::Target {
                    StreamUnravel {
                        stream: Some(self),
                        context: StreamUnravelState::None(PhantomData),
                    }
                }
            }
        )*
    };
}

marker_variants! {
    ,
    Sync,
    Send, Sync Send
}
