use crate::{
    allocated::{Flatten, FromError, ProtocolError},
    future::MapErr,
    CloneContext, Coalesce, ContextReference, Contextualize, Dispatch, Finalize, FinalizeImmediate,
    Fork, ForkContextRef as Fcr, Future, FutureExt, Join, JoinContextOwned as Jco, Notification,
    Notify, Read, RefContextTarget, ReferenceContext, Unravel, Write,
};
use alloc::{boxed::Box, vec, vec::Vec};
use core::{
    borrow::BorrowMut,
    future,
    mem::replace,
    pin::Pin,
    task::{Context, Poll},
};
use core_error::Error;
use futures::{
    future::{ready, Either},
    ready,
    stream::once,
    FutureExt as _, Stream, StreamExt,
};
use thiserror::Error;

type ForkContextRef<C, T> = Fcr<
    C,
    ErasedStreamUnravel<C, T>,
    T,
    fn(T, <C as ReferenceContext>::Context) -> ErasedStreamUnravel<C, T>,
>;

type JoinContextOwned<O, C> = Jco<C, O, fn(<C as CloneContext>::Context) -> O>;

type FinalizeTarget<C> =
    <C as Finalize<<C as Fork<<C as Notify<()>>::Notification>>::Finalize>>::Target;

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
        C: Error + 'static,
)]
pub enum StreamUnravelError<A, B, C, Z, T, I, U, V, W> {
    #[error("failed to read argument handle for erased stream: {0}")]
    Read(#[source] A),
    #[error("failed to join argument for erased stream: {0}")]
    Join(#[source] C),
    #[error("failed to unwrap argument notification for erased stream: {0}")]
    Unwrap(#[source] B),
    #[error("failed to contextualize erased stream: {0}")]
    Contextualize(#[source] Z),
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

type UnravelError<C, U> = StreamUnravelError<
    <RefContextTarget<C> as Read<
        Option<<RefContextTarget<C> as Dispatch<Notification<RefContextTarget<C>, ()>>>::Handle>,
    >>::Error,
    <<RefContextTarget<C> as Notify<()>>::Unwrap as Future<RefContextTarget<C>>>::Error,
    <<RefContextTarget<C> as Join<Notification<RefContextTarget<C>, ()>>>::Future as Future<
        RefContextTarget<C>,
    >>::Error,
    <ForkContextRef<C, U> as Future<C>>::Error,
    <RefContextTarget<C> as Write<
        Option<
            <RefContextTarget<C> as Dispatch<
                Notification<RefContextTarget<C>, <U as Stream>::Item>,
            >>::Handle,
        >,
    >>::Error,
    <<RefContextTarget<C> as Notify<<U as Stream>::Item>>::Wrap as Future<RefContextTarget<C>>>::Error,
    <<RefContextTarget<C> as Fork<Notification<RefContextTarget<C>, <U as Stream>::Item>>>::Future as Future<
        RefContextTarget<C>,
    >>::Error,
    <<RefContextTarget<C> as Fork<Notification<RefContextTarget<C>, <U as Stream>::Item>>>::Target as Future<
        RefContextTarget<C>,
    >>::Error,
    <<RefContextTarget<C> as Fork<Notification<RefContextTarget<C>, <U as Stream>::Item>>>::Finalize as Future<
        RefContextTarget<C>,
    >>::Error,
>;

pub enum ErasedStreamUnravelState<C: ?Sized + ReferenceContext, U>
where
    RefContextTarget<C>: Notify<()>
        + Notify<U>
        + Read<
            Option<
                <RefContextTarget<C> as Dispatch<Notification<RefContextTarget<C>, ()>>>::Handle,
            >,
        > + Write<
            Option<<RefContextTarget<C> as Dispatch<Notification<RefContextTarget<C>, U>>>::Handle>,
        >,
{
    Read,
    Join(<RefContextTarget<C> as Join<Notification<RefContextTarget<C>, ()>>>::Future),
    Unwrap(<RefContextTarget<C> as Notify<()>>::Unwrap),
    Next,
    Wrap(<RefContextTarget<C> as Notify<U>>::Wrap),
    Fork(<RefContextTarget<C> as Fork<Notification<RefContextTarget<C>, U>>>::Future),
    WriteNone,
    Write(
        <RefContextTarget<C> as Fork<Notification<RefContextTarget<C>, U>>>::Target,
        <RefContextTarget<C> as Dispatch<Notification<RefContextTarget<C>, U>>>::Handle,
    ),
    Flush(<RefContextTarget<C> as Fork<Notification<RefContextTarget<C>, U>>>::Target),
    FlushNone,
    Target(<RefContextTarget<C> as Fork<Notification<RefContextTarget<C>, U>>>::Target),
    Done,
}

pub struct ErasedStreamUnravel<C: ?Sized + ReferenceContext, U: Stream>
where
    RefContextTarget<C>: Notify<()>
        + Notify<U::Item>
        + Read<
            Option<
                <RefContextTarget<C> as Dispatch<Notification<RefContextTarget<C>, ()>>>::Handle,
            >,
        > + Write<
            Option<
                <RefContextTarget<C> as Dispatch<Notification<RefContextTarget<C>, U::Item>>>::Handle,
            >,
        >,
{
    state: ErasedStreamUnravelState<C, U::Item>,
    stream: U,
    pending:
        Vec<<RefContextTarget<C> as Fork<Notification<RefContextTarget<C>, U::Item>>>::Finalize>,
    context: C::Context,
}

impl<C: ?Sized + ReferenceContext, U: Stream> Unpin for ErasedStreamUnravel<C, U> where
    RefContextTarget<C>: Notify<()>
        + Notify<U::Item>
        + Read<
            Option<
                <RefContextTarget<C> as Dispatch<Notification<RefContextTarget<C>, ()>>>::Handle,
            >,
        > + Write<
            Option<
                <RefContextTarget<C> as Dispatch<Notification<RefContextTarget<C>, U::Item>>>::Handle,
            >,
        >
{
}

impl<C: ?Sized + Write<<C as Contextualize>::Handle> + Unpin + ReferenceContext, U: Stream>
    Future<C> for ErasedStreamUnravel<C, U>
where
    RefContextTarget<C>: Notify<()>
        + Notify<U::Item>
        + Read<
            Option<
                <RefContextTarget<C> as Dispatch<Notification<RefContextTarget<C>, ()>>>::Handle,
            >,
        > + Write<
            Option<
                <RefContextTarget<C> as Dispatch<Notification<RefContextTarget<C>, U::Item>>>::Handle,
            >,
        >,
    U: Unpin,
    <RefContextTarget<C> as Notify<U::Item>>::Wrap: Unpin,
    <RefContextTarget<C> as Fork<Notification<RefContextTarget<C>, U::Item>>>::Target: Unpin,
    <RefContextTarget<C> as Fork<Notification<RefContextTarget<C>, U::Item>>>::Future: Unpin,
    <RefContextTarget<C> as Fork<Notification<RefContextTarget<C>, U::Item>>>::Finalize: Unpin,
    RefContextTarget<C>: Unpin,
    C::ForkOutput: Unpin,
    <RefContextTarget<C> as Notify<()>>::Unwrap: Unpin,
    <RefContextTarget<C> as Join<Notification<RefContextTarget<C>, ()>>>::Future: Unpin,
{
    type Ok = ();
    type Error = UnravelError<C, U>;

    fn poll<R: BorrowMut<C>>(
        mut self: Pin<&mut Self>,
        cx: &mut Context,
        ctx: R,
    ) -> Poll<Result<Self::Ok, Self::Error>> {
        use ErasedStreamUnravelState::*;

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
                Read => {
                    let mut ct = Pin::new(&mut *ctx);
                    let handle = ready!(ct.as_mut().read(cx)).map_err(StreamUnravelError::Read)?;
                    if let Some(handle) = handle {
                        this.state =
                            Join(crate::Join::<Notification<RefContextTarget<C>, ()>>::join(
                                &mut *ctx, handle,
                            ));
                    } else {
                        this.state = Done;
                        return Poll::Ready(Ok(()));
                    }
                }
                Join(future) => {
                    let notification = ready!(Pin::new(future).poll(cx, &mut *ctx))
                        .map_err(StreamUnravelError::Join)?;
                    this.state = Unwrap(<RefContextTarget<C> as Notify<()>>::unwrap(
                        &mut *ctx,
                        notification,
                    ));
                }
                Unwrap(future) => {
                    ready!(Pin::new(future).poll(cx, &mut *ctx))
                        .map_err(StreamUnravelError::Unwrap)?;
                    this.state = Next;
                }
                Next => {
                    let item = ready!(Pin::new(&mut this.stream).poll_next(cx));
                    if let Some(item) = item {
                        this.state = Wrap(ctx.wrap(item));
                    } else {
                        this.state = WriteNone;
                    }
                }
                Wrap(future) => {
                    let item = ready!(Pin::new(future).poll(cx, &mut *ctx))
                        .map_err(StreamUnravelError::Notify)?;
                    this.state = Fork(ctx.fork(item));
                }
                Fork(future) => {
                    let (target, handle) = ready!(Pin::new(&mut *future).poll(cx, &mut *ctx))
                        .map_err(StreamUnravelError::Dispatch)?;
                    this.state = Write(target, handle);
                }
                Write(_, _) => {
                    let mut ctx = Pin::new(&mut *ctx);
                    ready!(ctx.as_mut().poll_ready(cx)).map_err(StreamUnravelError::Transport)?;
                    let data = replace(&mut this.state, Done);
                    if let Write(target, data) = data {
                        ctx.write(Some(data))
                            .map_err(StreamUnravelError::Transport)?;
                        this.state = Flush(target);
                    } else {
                        panic!("invalid state in ErasedStreamUnravel Write")
                    }
                }
                WriteNone => {
                    let mut ctx = Pin::new(&mut *ctx);
                    ready!(ctx.as_mut().poll_ready(cx)).map_err(StreamUnravelError::Transport)?;
                    ctx.write(None).map_err(StreamUnravelError::Transport)?;
                    this.state = FlushNone;
                }
                Flush(_) => {
                    ready!(Pin::new(&mut *ctx).poll_flush(cx))
                        .map_err(StreamUnravelError::Transport)?;
                    let data = replace(&mut this.state, Done);
                    if let Flush(target) = data {
                        this.state = Target(target);
                    } else {
                        panic!("invalid state in ErasedStreamUnravel Write")
                    }
                }
                FlushNone => {
                    ready!(Pin::new(&mut *ctx).poll_flush(cx))
                        .map_err(StreamUnravelError::Transport)?;
                    this.state = Done;
                }
                Target(target) => {
                    let mut finalize = ready!(Pin::new(target).poll(cx, &mut *ctx))
                        .map_err(StreamUnravelError::Target)?;
                    if let Poll::Pending = Pin::new(&mut finalize)
                        .poll(cx, &mut *ctx)
                        .map_err(StreamUnravelError::Finalize)?
                    {
                        this.pending.push(finalize);
                    }
                    this.state = Read;
                }
                Done => {
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

pub enum ErasedStreamCoalesceState<
    U,
    C: Notify<()>
        + Notify<U>
        + Write<Option<<C as Dispatch<<C as Notify<()>>::Notification>>::Handle>>
        + Read<Option<<C as Dispatch<<C as Notify<U>>::Notification>>::Handle>>
        + Finalize<<C as Fork<<C as Notify<()>>::Notification>>::Finalize>,
> where
    <C as Fork<<C as Notify<()>>::Notification>>::Finalize: Future<FinalizeTarget<C>>,
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
    <C as Fork<<C as Notify<()>>::Notification>>::Finalize: Future<FinalizeTarget<C>>,
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
        use ErasedStreamComplete::*;

        let this = &mut *self;

        loop {
            match this {
                Write => {
                    let mut ctx = Pin::new(ctx.borrow_mut());
                    ready!(ctx.as_mut().poll_ready(cx))?;
                    ctx.write(None)?;
                    *this = Flush;
                }
                Flush => {
                    ready!(Pin::new(ctx.borrow_mut()).poll_flush(cx))?;
                    *this = Done;
                    return Poll::Ready(Ok(()));
                }
                Done => panic!("erased Stream terminator polled after completion"),
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
    <C as Fork<<C as Notify<()>>::Notification>>::Finalize: Future<FinalizeTarget<C>>,
{
    fn drop(&mut self) {
        let _ = self.context.finalize_immediate(ErasedStreamComplete::Write);
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
    <C as Fork<<C as Notify<()>>::Notification>>::Finalize: Future<FinalizeTarget<C>>,
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
    <C as Fork<<C as Notify<()>>::Notification>>::Finalize: Future<FinalizeTarget<C>>,
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
        use ErasedStreamCoalesceState::*;

        let this = &mut *self;
        let ctx = this.context.borrow_mut();

        loop {
            match &mut this.state {
                Begin => {
                    this.state = Wrap(ctx.wrap(()));
                }
                Wrap(future) => {
                    let wrapped = ready!(Pin::new(future).poll(cx, &mut *ctx))
                        .map_err(|e| ProtocolError::new(CoalesceError::<U, C>::Wrap(e)))?;
                    this.state = Fork(ctx.fork(wrapped));
                }
                Fork(future) => {
                    let (target, handle) = ready!(Pin::new(future).poll(cx, &mut *ctx))
                        .map_err(|e| ProtocolError::new(CoalesceError::<U, C>::Fork(e)))?;
                    this.state = Write(target, handle);
                }
                Write(_, _) => {
                    let mut ctx = Pin::new(&mut *ctx);
                    ready!(ctx.as_mut().poll_ready(cx))
                        .map_err(|e| ProtocolError::new(CoalesceError::<U, C>::Write(e)))?;
                    let data = replace(&mut this.state, Done);
                    if let Write(target, data) = data {
                        ctx.write(Some(data))
                            .map_err(|e| ProtocolError::new(CoalesceError::<U, C>::Write(e)))?;
                        this.state = Flush(target);
                    } else {
                        panic!("invalid state in ErasedStreamCoalesce Write")
                    }
                }
                Flush(_) => {
                    ready!(Pin::new(&mut *ctx).poll_flush(cx))
                        .map_err(|e| ProtocolError::new(CoalesceError::<U, C>::Write(e)))?;
                    let data = replace(&mut this.state, Done);
                    if let Flush(target) = data {
                        this.state = Target(target);
                    } else {
                        panic!("invalid state in ErasedStreamUnravel Write")
                    }
                }
                Target(target) => {
                    let finalize = ready!(Pin::new(target).poll(cx, &mut *ctx))
                        .map_err(|e| ProtocolError::new(CoalesceError::<U, C>::Target(e)))?;
                    this.state = Finalize(ctx.finalize(finalize));
                }
                Finalize(future) => {
                    ready!(Pin::new(future).poll(cx, &mut *ctx))
                        .map_err(|e| ProtocolError::new(CoalesceError::<U, C>::Finalize(e)))?;
                    this.state = Read;
                }
                Read => {
                    let handle: Option<<C as Dispatch<<C as Notify<U>>::Notification>>::Handle> =
                        ready!(Pin::new(&mut *ctx).read(cx))
                            .map_err(|e| ProtocolError::new(CoalesceError::<U, C>::Read(e)))?;
                    if let Some(handle) = handle {
                        let join: <C as crate::Join<<C as Notify<U>>::Notification>>::Future =
                            crate::Join::<<C as Notify<U>>::Notification>::join(ctx, handle);
                        this.state = Join(join);
                    } else {
                        this.state = Done;
                        return Poll::Ready(None);
                    }
                }
                Join(future) => {
                    let wrapped = ready!(Pin::new(future).poll(cx, &mut *ctx))
                        .map_err(|e| ProtocolError::new(CoalesceError::<U, C>::Join(e)))?;
                    let unwrap: <C as Notify<U>>::Unwrap = Notify::<U>::unwrap(ctx, wrapped);
                    this.state = Unwrap(unwrap);
                }
                Unwrap(future) => {
                    let data = ready!(Pin::new(future).poll(cx, &mut *ctx))
                        .map_err(|e| ProtocolError::new(CoalesceError::<U, C>::Unwrap(e)))?;
                    this.state = Begin;
                    return Poll::Ready(Some(Ok(data)));
                }
                Done => panic!("erased Stream coalesce polled after completion"),
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
                type Future = JoinContextOwned<Self, C>;

                fn coalesce() -> Self::Future {
                    JoinContextOwned::new(|context| {
                        Box::pin(ErasedStreamCoalesce {
                            context,
                            state: ErasedStreamCoalesceState::Begin
                        }.map(|item| item.unwrap_or_else(|e| U::from_error(e))))
                    })
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
                RefContextTarget<C>: Notify<()>
                    + Notify<U>
                    + Read<
                        Option<
                            <RefContextTarget<C> as Dispatch<
                                Notification<RefContextTarget<C>, ()>,
                            >>::Handle,
                        >,
                    > + Write<
                        Option<
                            <RefContextTarget<C> as Dispatch<
                                Notification<RefContextTarget<C>, U>,
                            >>::Handle,
                        >,
                    >,
                U: Unpin,
                C: Unpin,
                C::ForkOutput: Unpin,
                <RefContextTarget<C> as Notify<U>>::Wrap: Unpin,
                <RefContextTarget<C> as Fork<
                    Notification<RefContextTarget<C>, U>,
                >>::Target: Unpin,
                <RefContextTarget<C> as Fork<
                    Notification<RefContextTarget<C>, U>,
                >>::Future: Unpin,
                <RefContextTarget<C> as Fork<
                    Notification<RefContextTarget<C>, U>,
                >>::Finalize: Unpin,
                RefContextTarget<C>: Unpin,
                <RefContextTarget<C> as Notify<()>>::Unwrap: Unpin,
                <RefContextTarget<C> as Join<
                    Notification<RefContextTarget<C>, ()>,
                >>::Future: Unpin,
            {
                type Finalize = ErasedStreamUnravel<C, Self>;
                type Target = MapErr<ForkContextRef<C, Self>, fn(<ForkContextRef<C, Self> as Future<C>>::Error) -> UnravelError<C, Self>>;

                fn unravel(self) -> Self::Target {
                    ForkContextRef::new(self, |stream, context| {
                        ErasedStreamUnravel {
                            context,
                            state: ErasedStreamUnravelState::Read,
                            pending: vec![],
                            stream,
                        }
                    }).map_err(StreamUnravelError::Contextualize)
                }
            }

            impl<'a, E, U: FromError<E> + 'a $(+ $marker)*> FromError<E> for Pin<Box<dyn Stream<Item = U> + 'a $(+ $marker)*>> {
                fn from_error(error: E) -> Self {
                    Box::pin(once(ready(U::from_error(error))))
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
