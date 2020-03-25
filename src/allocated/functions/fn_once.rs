use crate::{
    allocated::{Flatten, ProtocolError},
    CloneContext, Coalesce, ContextReference, Contextualize, Dispatch, Finalize, Fork, Future,
    Join, Notify, Read, ReferenceContext, Unravel, Write,
};
use alloc::boxed::Box;
use core::{
    borrow::BorrowMut,
    future,
    marker::PhantomData,
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
        I: Error + 'static,
        U: Error + 'static,
        V: Error + 'static,
        W: Error + 'static,
        Z: Error + 'static,
        K: Error + 'static,
        C: Error + 'static,
)]
pub enum FnOnceUnravelError<A, B, C, Z, K, T, I, U, V, W> {
    #[error("failed to read argument handle for erased FnOnce: {0}")]
    Read(#[source] A),
    #[error("failed to join argument for erased FnOnce: {0}")]
    Join(#[source] C),
    #[error("failed to unwrap argument notification for erased FnOnce: {0}")]
    Unwrap(#[source] B),
    #[error("failed to contextualize erased FnOnce: {0}")]
    Contextualize(#[source] Z),
    #[error("failed to write context handle for erased FnOnce: {0}")]
    Write(#[source] K),
    #[error("failed to write handle for erased FnOnce: {0}")]
    Transport(#[source] T),
    #[error("failed to create notification wrapper for erased FnOnce return: {0}")]
    Notify(#[source] I),
    #[error("failed to fork erased FnOnce return: {0}")]
    Dispatch(#[source] U),
    #[error("failed to target erased FnOnce return: {0}")]
    Target(#[source] V),
    #[error("failed to finalize erased FnOnce return: {0}")]
    Finalize(#[source] W),
}

pub enum ErasedFnOnceUnravelState<C: ?Sized + ReferenceContext, T, U>
where
    <C::Context as ContextReference<C>>::Target: Notify<T>
        + Notify<U>
        + Read<
            <<C::Context as ContextReference<C>>::Target as Dispatch<
                <<C::Context as ContextReference<C>>::Target as Notify<T>>::Notification,
            >>::Handle,
        > + Write<
            <<C::Context as ContextReference<C>>::Target as Dispatch<
                <<C::Context as ContextReference<C>>::Target as Notify<U>>::Notification,
            >>::Handle,
        >,
{
    Read,
    Join(
        <<C::Context as ContextReference<C>>::Target as Join<
            <<C::Context as ContextReference<C>>::Target as Notify<T>>::Notification,
        >>::Future,
    ),
    Unwrap(<<C::Context as ContextReference<C>>::Target as Notify<T>>::Unwrap),
    Wrap(<<C::Context as ContextReference<C>>::Target as Notify<U>>::Wrap),
    Fork(
        <<C::Context as ContextReference<C>>::Target as Fork<
            <<C::Context as ContextReference<C>>::Target as Notify<U>>::Notification,
        >>::Future,
    ),
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
    Target(
        <<C::Context as ContextReference<C>>::Target as Fork<
            <<C::Context as ContextReference<C>>::Target as Notify<U>>::Notification,
        >>::Target,
    ),
    Finalize(
        <<C::Context as ContextReference<C>>::Target as Fork<
            <<C::Context as ContextReference<C>>::Target as Notify<U>>::Notification,
        >>::Finalize,
    ),
    Done,
}

pub struct ErasedFnOnceUnravel<C: ?Sized + ReferenceContext, T, U, F, M: Fn(F, T) -> U>
where
    <C::Context as ContextReference<C>>::Target: Notify<T>
        + Notify<U>
        + Read<
            <<C::Context as ContextReference<C>>::Target as Dispatch<
                <<C::Context as ContextReference<C>>::Target as Notify<T>>::Notification,
            >>::Handle,
        > + Write<
            <<C::Context as ContextReference<C>>::Target as Dispatch<
                <<C::Context as ContextReference<C>>::Target as Notify<U>>::Notification,
            >>::Handle,
        >,
{
    state: ErasedFnOnceUnravelState<C, T, U>,
    call: Option<F>,
    conv: M,
    context: C::Context,
}

impl<C: ?Sized + ReferenceContext, T, U, F, M: Fn(F, T) -> U> Unpin
    for ErasedFnOnceUnravel<C, T, U, F, M>
where
    <C::Context as ContextReference<C>>::Target: Notify<T>
        + Notify<U>
        + Read<
            <<C::Context as ContextReference<C>>::Target as Dispatch<
                <<C::Context as ContextReference<C>>::Target as Notify<T>>::Notification,
            >>::Handle,
        > + Write<
            <<C::Context as ContextReference<C>>::Target as Dispatch<
                <<C::Context as ContextReference<C>>::Target as Notify<U>>::Notification,
            >>::Handle,
        >,
{
}

impl<C: ?Sized + Write<<C as Contextualize>::Handle> + ReferenceContext, T, U, F, M: Fn(F, T) -> U>
    Future<C> for ErasedFnOnceUnravel<C, T, U, F, M>
where
    <C::Context as ContextReference<C>>::Target: Notify<T>
        + Notify<U>
        + Read<
            <<C::Context as ContextReference<C>>::Target as Dispatch<
                <<C::Context as ContextReference<C>>::Target as Notify<T>>::Notification,
            >>::Handle,
        > + Write<
            <<C::Context as ContextReference<C>>::Target as Dispatch<
                <<C::Context as ContextReference<C>>::Target as Notify<U>>::Notification,
            >>::Handle,
        >,
    U: Unpin,
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
    <<C::Context as ContextReference<C>>::Target as Notify<T>>::Unwrap: Unpin,
    <<C::Context as ContextReference<C>>::Target as Join<
        <<C::Context as ContextReference<C>>::Target as Notify<T>>::Notification,
    >>::Future: Unpin,
{
    type Ok = ();
    type Error = FnOnceUnravelError<
        <<C::Context as ContextReference<C>>::Target as Read<
            <<C::Context as ContextReference<C>>::Target as Dispatch<
                <<C::Context as ContextReference<C>>::Target as Notify<T>>::Notification,
            >>::Handle,
        >>::Error,
        <<<C::Context as ContextReference<C>>::Target as Notify<T>>::Unwrap as Future<
            <C::Context as ContextReference<C>>::Target,
        >>::Error,
        <<<C::Context as ContextReference<C>>::Target as Join<
            <<C::Context as ContextReference<C>>::Target as Notify<T>>::Notification,
        >>::Future as Future<<C::Context as ContextReference<C>>::Target>>::Error,
        <C::ForkOutput as Future<C>>::Error,
        C::Error,
        <<C::Context as ContextReference<C>>::Target as Write<
            <<C::Context as ContextReference<C>>::Target as Dispatch<
                <<C::Context as ContextReference<C>>::Target as Notify<U>>::Notification,
            >>::Handle,
        >>::Error,
        <<<C::Context as ContextReference<C>>::Target as Notify<U>>::Wrap as Future<
            <C::Context as ContextReference<C>>::Target,
        >>::Error,
        <<<C::Context as ContextReference<C>>::Target as Fork<
            <<C::Context as ContextReference<C>>::Target as Notify<U>>::Notification,
        >>::Future as Future<<C::Context as ContextReference<C>>::Target>>::Error,
        <<<C::Context as ContextReference<C>>::Target as Fork<
            <<C::Context as ContextReference<C>>::Target as Notify<U>>::Notification,
        >>::Target as Future<<C::Context as ContextReference<C>>::Target>>::Error,
        <<<C::Context as ContextReference<C>>::Target as Fork<
            <<C::Context as ContextReference<C>>::Target as Notify<U>>::Notification,
        >>::Finalize as Future<<C::Context as ContextReference<C>>::Target>>::Error,
    >;

    fn poll<R: BorrowMut<C>>(
        mut self: Pin<&mut Self>,
        cx: &mut Context,
        ctx: R,
    ) -> Poll<Result<Self::Ok, Self::Error>> {
        let this = &mut *self;

        let ctx = this.context.with(ctx);

        loop {
            match &mut this.state {
                ErasedFnOnceUnravelState::Read => {
                    let mut ct = Pin::new(&mut *ctx);
                    let handle = ready!(ct.as_mut().read(cx)).map_err(FnOnceUnravelError::Read)?;
                    this.state = ErasedFnOnceUnravelState::Join(Join::<
                        <<C::Context as ContextReference<C>>::Target as Notify<T>>::Notification,
                    >::join(
                        &mut *ctx, handle
                    ));
                }
                ErasedFnOnceUnravelState::Join(future) => {
                    let notification = ready!(Pin::new(future).poll(cx, &mut *ctx))
                        .map_err(FnOnceUnravelError::Join)?;
                    this.state = ErasedFnOnceUnravelState::Unwrap(
                        <<C::Context as ContextReference<C>>::Target as Notify<T>>::unwrap(
                            &mut *ctx,
                            notification,
                        ),
                    );
                }
                ErasedFnOnceUnravelState::Unwrap(future) => {
                    let item = ready!(Pin::new(future).poll(cx, &mut *ctx))
                        .map_err(FnOnceUnravelError::Unwrap)?;
                    this.state = ErasedFnOnceUnravelState::Wrap(
                        ctx.wrap((this.conv)(this.call.take().unwrap(), item)),
                    );
                }
                ErasedFnOnceUnravelState::Wrap(future) => {
                    let item = ready!(Pin::new(future).poll(cx, &mut *ctx))
                        .map_err(FnOnceUnravelError::Notify)?;
                    this.state = ErasedFnOnceUnravelState::Fork(ctx.fork(item));
                }
                ErasedFnOnceUnravelState::Fork(future) => {
                    let (target, handle) = ready!(Pin::new(&mut *future).poll(cx, &mut *ctx))
                        .map_err(FnOnceUnravelError::Dispatch)?;
                    this.state = ErasedFnOnceUnravelState::Write(target, handle);
                }
                ErasedFnOnceUnravelState::Write(_, _) => {
                    let mut ctx = Pin::new(&mut *ctx);
                    ready!(ctx.as_mut().poll_ready(cx)).map_err(FnOnceUnravelError::Transport)?;
                    let data = replace(&mut this.state, ErasedFnOnceUnravelState::Done);
                    if let ErasedFnOnceUnravelState::Write(target, data) = data {
                        ctx.write(data).map_err(FnOnceUnravelError::Transport)?;
                        this.state = ErasedFnOnceUnravelState::Flush(target);
                    } else {
                        panic!("invalid state in ErasedFnOnceUnravel Write")
                    }
                }
                ErasedFnOnceUnravelState::Flush(_) => {
                    ready!(Pin::new(&mut *ctx).poll_flush(cx))
                        .map_err(FnOnceUnravelError::Transport)?;
                    let data = replace(&mut this.state, ErasedFnOnceUnravelState::Done);
                    if let ErasedFnOnceUnravelState::Flush(target) = data {
                        this.state = ErasedFnOnceUnravelState::Target(target);
                    } else {
                        panic!("invalid state in ErasedFnOnceUnravel Write")
                    }
                }
                ErasedFnOnceUnravelState::Target(target) => {
                    let finalize = ready!(Pin::new(target).poll(cx, &mut *ctx))
                        .map_err(FnOnceUnravelError::Target)?;
                    this.state = ErasedFnOnceUnravelState::Finalize(finalize);
                }
                ErasedFnOnceUnravelState::Finalize(finalize) => {
                    ready!(Pin::new(finalize).poll(cx, &mut *ctx))
                        .map_err(FnOnceUnravelError::Finalize)?;
                    this.state = ErasedFnOnceUnravelState::Done;
                    return Poll::Ready(Ok(()));
                }
                ErasedFnOnceUnravelState::Done => {
                    panic!("ErasedFnOnceUnravel polled after completion")
                }
            }
        }
    }
}

enum FnOnceUnravelState<C: ?Sized + Write<<C as Contextualize>::Handle> + ReferenceContext, T, U>
where
    <C::Context as ContextReference<C>>::Target: Notify<T>
        + Notify<U>
        + Read<
            <<C::Context as ContextReference<C>>::Target as Dispatch<
                <<C::Context as ContextReference<C>>::Target as Notify<T>>::Notification,
            >>::Handle,
        > + Write<
            <<C::Context as ContextReference<C>>::Target as Dispatch<
                <<C::Context as ContextReference<C>>::Target as Notify<U>>::Notification,
            >>::Handle,
        >,
    U: Unpin,
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
    <<C::Context as ContextReference<C>>::Target as Notify<T>>::Unwrap: Unpin,
    <<C::Context as ContextReference<C>>::Target as Join<
        <<C::Context as ContextReference<C>>::Target as Notify<T>>::Notification,
    >>::Future: Unpin,
{
    None(PhantomData<(T, U)>),
    Context(C::ForkOutput),
    Write(C::Context, C::Handle),
    Flush(C::Context),
}

pub struct FnOnceUnravel<
    C: ?Sized + Write<<C as Contextualize>::Handle> + ReferenceContext,
    T,
    U,
    F,
    M: Fn(F, T) -> U,
> where
    <C::Context as ContextReference<C>>::Target: Notify<T>
        + Notify<U>
        + Read<
            <<C::Context as ContextReference<C>>::Target as Dispatch<
                <<C::Context as ContextReference<C>>::Target as Notify<T>>::Notification,
            >>::Handle,
        > + Write<
            <<C::Context as ContextReference<C>>::Target as Dispatch<
                <<C::Context as ContextReference<C>>::Target as Notify<U>>::Notification,
            >>::Handle,
        >,
    U: Unpin,
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
    <<C::Context as ContextReference<C>>::Target as Notify<T>>::Unwrap: Unpin,
    <<C::Context as ContextReference<C>>::Target as Join<
        <<C::Context as ContextReference<C>>::Target as Notify<T>>::Notification,
    >>::Future: Unpin,
{
    call: Option<F>,
    context: FnOnceUnravelState<C, T, U>,
    conv: Option<M>,
}

impl<C: ?Sized + Write<<C as Contextualize>::Handle> + ReferenceContext, T, U, F, M: Fn(F, T) -> U> Unpin
    for FnOnceUnravel<C, T, U, F, M>
where
    <C::Context as ContextReference<C>>::Target: Notify<T>
        + Notify<U>
        + Read<
            <<C::Context as ContextReference<C>>::Target as Dispatch<
                <<C::Context as ContextReference<C>>::Target as Notify<T>>::Notification,
            >>::Handle,
        > + Write<
            <<C::Context as ContextReference<C>>::Target as Dispatch<
                <<C::Context as ContextReference<C>>::Target as Notify<U>>::Notification,
            >>::Handle,
        >,
    U: Unpin,
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
    <<C::Context as ContextReference<C>>::Target as Notify<T>>::Unwrap: Unpin,
    <<C::Context as ContextReference<C>>::Target as Join<
        <<C::Context as ContextReference<C>>::Target as Notify<T>>::Notification,
    >>::Future: Unpin,
    C::ForkOutput: Unpin,
    C: Unpin,
{
}

impl<C: ?Sized + Write<<C as Contextualize>::Handle> + ReferenceContext, T, U, F, M: Fn(F, T) -> U>
    Future<C> for FnOnceUnravel<C, T, U, F, M>
where
    <C::Context as ContextReference<C>>::Target: Notify<T>
        + Notify<U>
        + Read<
            <<C::Context as ContextReference<C>>::Target as Dispatch<
                <<C::Context as ContextReference<C>>::Target as Notify<T>>::Notification,
            >>::Handle,
        > + Write<
            <<C::Context as ContextReference<C>>::Target as Dispatch<
                <<C::Context as ContextReference<C>>::Target as Notify<U>>::Notification,
            >>::Handle,
        >,
    U: Unpin,
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
    <<C::Context as ContextReference<C>>::Target as Notify<T>>::Unwrap: Unpin,
    <<C::Context as ContextReference<C>>::Target as Join<
        <<C::Context as ContextReference<C>>::Target as Notify<T>>::Notification,
    >>::Future: Unpin,
    C::ForkOutput: Unpin,
    C: Unpin,
{
    type Ok = ErasedFnOnceUnravel<C, T, U, F, M>;
    type Error = FnOnceUnravelError<
        <<C::Context as ContextReference<C>>::Target as Read<
            <<C::Context as ContextReference<C>>::Target as Dispatch<
                <<C::Context as ContextReference<C>>::Target as Notify<T>>::Notification,
            >>::Handle,
        >>::Error,
        <<<C::Context as ContextReference<C>>::Target as Notify<T>>::Unwrap as Future<
            <C::Context as ContextReference<C>>::Target,
        >>::Error,
        <<<C::Context as ContextReference<C>>::Target as Join<
            <<C::Context as ContextReference<C>>::Target as Notify<T>>::Notification,
        >>::Future as Future<<C::Context as ContextReference<C>>::Target>>::Error,
        <C::ForkOutput as Future<C>>::Error,
        C::Error,
        <<C::Context as ContextReference<C>>::Target as Write<
            <<C::Context as ContextReference<C>>::Target as Dispatch<
                <<C::Context as ContextReference<C>>::Target as Notify<U>>::Notification,
            >>::Handle,
        >>::Error,
        <<<C::Context as ContextReference<C>>::Target as Notify<U>>::Wrap as Future<
            <C::Context as ContextReference<C>>::Target,
        >>::Error,
        <<<C::Context as ContextReference<C>>::Target as Fork<
            <<C::Context as ContextReference<C>>::Target as Notify<U>>::Notification,
        >>::Future as Future<<C::Context as ContextReference<C>>::Target>>::Error,
        <<<C::Context as ContextReference<C>>::Target as Fork<
            <<C::Context as ContextReference<C>>::Target as Notify<U>>::Notification,
        >>::Target as Future<<C::Context as ContextReference<C>>::Target>>::Error,
        <<<C::Context as ContextReference<C>>::Target as Fork<
            <<C::Context as ContextReference<C>>::Target as Notify<U>>::Notification,
        >>::Finalize as Future<<C::Context as ContextReference<C>>::Target>>::Error,
    >;

    fn poll<R: BorrowMut<C>>(
        mut self: Pin<&mut Self>,
        cx: &mut Context,
        mut ctx: R,
    ) -> Poll<Result<Self::Ok, Self::Error>> {
        let this = &mut *self;

        loop {
            if this.call.is_some() {
                match &mut this.context {
                    FnOnceUnravelState::None(_) => {
                        this.context = FnOnceUnravelState::Context(ctx.borrow_mut().fork_ref())
                    }
                    FnOnceUnravelState::Context(future) => {
                        let (context, handle) = ready!(Pin::new(future).poll(cx, ctx.borrow_mut()))
                            .map_err(FnOnceUnravelError::Contextualize)?;
                        this.context = FnOnceUnravelState::Write(context, handle);
                    }
                    FnOnceUnravelState::Write(_, _) => {
                        let mut ctx = Pin::new(ctx.borrow_mut());
                        ready!(ctx.as_mut().poll_ready(cx)).map_err(FnOnceUnravelError::Write)?;
                        let data =
                            replace(&mut this.context, FnOnceUnravelState::None(PhantomData));
                        if let FnOnceUnravelState::Write(context, handle) = data {
                            ctx.write(handle).map_err(FnOnceUnravelError::Write)?;
                            this.context = FnOnceUnravelState::Flush(context);
                        } else {
                            panic!("invalid state")
                        }
                    }
                    FnOnceUnravelState::Flush(_) => {
                        let mut ctx = Pin::new(ctx.borrow_mut());
                        ready!(ctx.as_mut().poll_flush(cx)).map_err(FnOnceUnravelError::Write)?;
                        let data =
                            replace(&mut this.context, FnOnceUnravelState::None(PhantomData));
                        if let FnOnceUnravelState::Flush(context) = data {
                            return Poll::Ready(Ok(ErasedFnOnceUnravel {
                                context,
                                call: this.call.take(),
                                conv: this.conv.take().unwrap(),
                                state: ErasedFnOnceUnravelState::Read,
                            }));
                        } else {
                            panic!("invalid state")
                        }
                    }
                }
            } else {
                panic!("FnOnceUnravel polled after completion")
            }
        }
    }
}

pub enum ErasedFnOnceCoalesceState<
    T,
    U,
    C: Notify<T>
        + Notify<U>
        + Write<<C as Dispatch<<C as Notify<T>>::Notification>>::Handle>
        + Read<<C as Dispatch<<C as Notify<U>>::Notification>>::Handle>
        + Finalize<<C as Fork<<C as Notify<T>>::Notification>>::Finalize>,
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
    Join(<C as Join<<C as Notify<U>>::Notification>>::Future),
    Unwrap(<C as Notify<U>>::Unwrap),
    Done,
}

pub struct ErasedFnOnceCoalesce<
    T,
    U,
    C: Notify<T>
        + Notify<U>
        + Write<<C as Dispatch<<C as Notify<T>>::Notification>>::Handle>
        + Read<<C as Dispatch<<C as Notify<U>>::Notification>>::Handle>
        + Finalize<<C as Fork<<C as Notify<T>>::Notification>>::Finalize>,
> where
    <C as Fork<<C as Notify<T>>::Notification>>::Finalize:
        Future<<C as Finalize<<C as Fork<<C as Notify<T>>::Notification>>::Finalize>>::Target>,
{
    state: ErasedFnOnceCoalesceState<T, U, C>,
    context: C,
}

impl<
    T,
    U,
    C: Notify<T>
        + Notify<U>
        + Write<<C as Dispatch<<C as Notify<T>>::Notification>>::Handle>
        + Read<<C as Dispatch<<C as Notify<U>>::Notification>>::Handle>
        + Finalize<<C as Fork<<C as Notify<T>>::Notification>>::Finalize>,
> Unpin for ErasedFnOnceCoalesce<T, U, C>
where
    <C as Fork<<C as Notify<T>>::Notification>>::Finalize:
        Future<<C as Finalize<<C as Fork<<C as Notify<T>>::Notification>>::Finalize>>::Target>,
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
pub enum ErasedFnOnceCoalesceError<A, C, B, T, I, U, V, W> {
    #[error("failed to write argument handle for erased FnOnce: {0}")]
    Write(#[source] A),
    #[error("failed to fork argument for erased FnOnce: {0}")]
    Fork(#[source] C),
    #[error("failed to wrap argument notification for erased FnOnce: {0}")]
    Wrap(#[source] B),
    #[error("failed to read handle for erased FnOnce return: {0}")]
    Read(#[source] T),
    #[error("failed to create notification wrapper for erased FnOnce return: {0}")]
    Unwrap(#[source] I),
    #[error("failed to join erased FnOnce return: {0}")]
    Join(#[source] U),
    #[error("failed to target erased FnOnce argument: {0}")]
    Target(#[source] V),
    #[error("failed to finalize erased FnOnce argument: {0}")]
    Finalize(#[source] W),
}

type CoalesceError<T, U, C> = ErasedFnOnceCoalesceError<
    <C as Write<<C as Dispatch<<C as Notify<T>>::Notification>>::Handle>>::Error,
    <<C as Fork<<C as Notify<T>>::Notification>>::Future as Future<C>>::Error,
    <<C as Notify<T>>::Wrap as Future<C>>::Error,
    <C as Read<<C as Dispatch<<C as Notify<U>>::Notification>>::Handle>>::Error,
    <<C as Notify<U>>::Unwrap as Future<C>>::Error,
    <<C as Join<<C as Notify<U>>::Notification>>::Future as Future<C>>::Error,
    <<C as Fork<<C as Notify<T>>::Notification>>::Target as Future<C>>::Error,
    <<C as Finalize<<C as Fork<<C as Notify<T>>::Notification>>::Finalize>>::Output as Future<C>>::Error,
>;

impl<
    T,
    U,
    C: Notify<T>
        + Notify<U>
        + Write<<C as Dispatch<<C as Notify<T>>::Notification>>::Handle>
        + Read<<C as Dispatch<<C as Notify<U>>::Notification>>::Handle>
        + Finalize<<C as Fork<<C as Notify<T>>::Notification>>::Finalize>,
> future::Future for ErasedFnOnceCoalesce<T, U, C>
where
    <C as Notify<T>>::Wrap: Unpin,
    <C as Notify<U>>::Unwrap: Unpin,
    C: Unpin,
    <C as Fork<<C as Notify<T>>::Notification>>::Future: Unpin,
    <C as Fork<<C as Notify<T>>::Notification>>::Target: Unpin,
    <C as Fork<<C as Notify<T>>::Notification>>::Finalize:
        Future<<C as Finalize<<C as Fork<<C as Notify<T>>::Notification>>::Finalize>>::Target>,
    <C as Finalize<<C as Fork<<C as Notify<T>>::Notification>>::Finalize>>::Output: Unpin,
    <C as Write<<C as Dispatch<<C as Notify<T>>::Notification>>::Handle>>::Error:
        Error + Send + 'static,
    <<C as Fork<<C as Notify<T>>::Notification>>::Future as Future<C>>::Error:
        Error + Send + 'static,
    <<C as Notify<T>>::Wrap as Future<C>>::Error: Error + Send + 'static,
    <C as Read<<C as Dispatch<<C as Notify<U>>::Notification>>::Handle>>::Error:
        Error + Send + 'static,
    <<C as Notify<U>>::Unwrap as Future<C>>::Error: Error + Send + 'static,
    <C as Join<<C as Notify<U>>::Notification>>::Future: Unpin,
    <<C as Join<<C as Notify<U>>::Notification>>::Future as Future<C>>::Error:
        Error + Send + 'static,
    <<C as Fork<<C as Notify<T>>::Notification>>::Target as Future<C>>::Error:
        Error + Send + 'static,
    <<C as Finalize<<C as Fork<<C as Notify<T>>::Notification>>::Finalize>>::Output as Future<C>>::Error:
        Error + Send + 'static,
{
    type Output = Result<U, ProtocolError>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        let this = &mut *self;
        let ctx = this.context.borrow_mut();

        loop {
            match &mut this.state {
                ErasedFnOnceCoalesceState::Wrap(future) => {
                    let wrapped = ready!(Pin::new(future).poll(cx, &mut *ctx))
                        .map_err(|e| ProtocolError::new(CoalesceError::<T, U, C>::Wrap(e)))?;
                    this.state = ErasedFnOnceCoalesceState::Fork(ctx.fork(wrapped));
                }
                ErasedFnOnceCoalesceState::Fork(future) => {
                    let (target, handle) = ready!(Pin::new(future).poll(cx, &mut *ctx))
                        .map_err(|e| ProtocolError::new(CoalesceError::<T, U, C>::Fork(e)))?;
                    this.state = ErasedFnOnceCoalesceState::Write(target, handle);
                }
                ErasedFnOnceCoalesceState::Write(_, _) => {
                    let mut ctx = Pin::new(&mut *ctx);
                    ready!(ctx.as_mut().poll_ready(cx))
                        .map_err(|e| ProtocolError::new(CoalesceError::<T, U, C>::Write(e)))?;
                    let data = replace(&mut this.state, ErasedFnOnceCoalesceState::Done);
                    if let ErasedFnOnceCoalesceState::Write(target, data) = data {
                        ctx.write(data)
                            .map_err(|e| ProtocolError::new(CoalesceError::<T, U, C>::Write(e)))?;
                        this.state = ErasedFnOnceCoalesceState::Flush(target);
                    } else {
                        panic!("invalid state in ErasedFnOnceCoalesce Write")
                    }
                }
                ErasedFnOnceCoalesceState::Flush(_) => {
                    ready!(Pin::new(&mut *ctx).poll_ready(cx))
                        .map_err(|e| ProtocolError::new(CoalesceError::<T, U, C>::Write(e)))?;
                    let data = replace(&mut this.state, ErasedFnOnceCoalesceState::Done);
                    if let ErasedFnOnceCoalesceState::Flush(target) = data {
                        this.state = ErasedFnOnceCoalesceState::Target(target);
                    } else {
                        panic!("invalid state in ErasedFnOnceUnravel Write")
                    }
                }
                ErasedFnOnceCoalesceState::Target(target) => {
                    let finalize = ready!(Pin::new(target).poll(cx, &mut *ctx))
                        .map_err(|e| ProtocolError::new(CoalesceError::<T, U, C>::Target(e)))?;
                    this.state = ErasedFnOnceCoalesceState::Finalize(ctx.finalize(finalize));
                }
                ErasedFnOnceCoalesceState::Finalize(future) => {
                    ready!(Pin::new(future).poll(cx, &mut *ctx))
                        .map_err(|e| ProtocolError::new(CoalesceError::<T, U, C>::Finalize(e)))?;
                    this.state = ErasedFnOnceCoalesceState::Read;
                }
                ErasedFnOnceCoalesceState::Read => {
                    let handle: <C as Dispatch<<C as Notify<U>>::Notification>>::Handle =
                        ready!(Pin::new(&mut *ctx).read(cx))
                            .map_err(|e| ProtocolError::new(CoalesceError::<T, U, C>::Read(e)))?;
                    let join: <C as Join<<C as Notify<U>>::Notification>>::Future =
                        Join::<<C as Notify<U>>::Notification>::join(ctx, handle);
                    this.state = ErasedFnOnceCoalesceState::Join(join);
                }
                ErasedFnOnceCoalesceState::Join(future) => {
                    let wrapped = ready!(Pin::new(future).poll(cx, &mut *ctx))
                        .map_err(|e| ProtocolError::new(CoalesceError::<T, U, C>::Join(e)))?;
                    let unwrap: <C as Notify<U>>::Unwrap = Notify::<U>::unwrap(ctx, wrapped);
                    this.state = ErasedFnOnceCoalesceState::Unwrap(unwrap);
                }
                ErasedFnOnceCoalesceState::Unwrap(future) => {
                    let data = ready!(Pin::new(future).poll(cx, &mut *ctx))
                        .map_err(|e| ProtocolError::new(CoalesceError::<T, U, C>::Unwrap(e)))?;
                    this.state = ErasedFnOnceCoalesceState::Done;
                    return Poll::Ready(Ok(data));
                }
                ErasedFnOnceCoalesceState::Done => {
                    panic!("erased FnOnce coalesce polled after completion")
                }
            }
        }
    }
}

pub enum FnOnceCoalesceState<T> {
    Read,
    Contextualize(T),
    Done,
}

pub struct ErasedFnOnce<T, U, C> {
    context: C,
    data: PhantomData<(T, U)>,
}

impl<
    T,
    U: Flatten<ProtocolError, ErasedFnOnceCoalesce<T, U, C>>,
    C: Notify<T>
        + Notify<U>
        + Write<<C as Dispatch<<C as Notify<T>>::Notification>>::Handle>
        + Read<<C as Dispatch<<C as Notify<U>>::Notification>>::Handle>
        + Finalize<<C as Fork<<C as Notify<T>>::Notification>>::Finalize>,
> ErasedFnOnce<T, U, C>
where
    <C as Notify<T>>::Wrap: Unpin,
    <C as Notify<U>>::Unwrap: Unpin,
    C: Unpin,
    <C as Fork<<C as Notify<T>>::Notification>>::Future: Unpin,
    <C as Fork<<C as Notify<T>>::Notification>>::Target: Unpin,
    <C as Fork<<C as Notify<T>>::Notification>>::Finalize:
        Future<<C as Finalize<<C as Fork<<C as Notify<T>>::Notification>>::Finalize>>::Target>,
    <C as Finalize<<C as Fork<<C as Notify<T>>::Notification>>::Finalize>>::Output: Unpin,
    <C as Write<<C as Dispatch<<C as Notify<T>>::Notification>>::Handle>>::Error:
        Error + Send + 'static,
    <<C as Fork<<C as Notify<T>>::Notification>>::Future as Future<C>>::Error:
        Error + Send + 'static,
    <<C as Notify<T>>::Wrap as Future<C>>::Error: Error + Send + 'static,
    <C as Read<<C as Dispatch<<C as Notify<U>>::Notification>>::Handle>>::Error:
        Error + Send + 'static,
    <<C as Notify<U>>::Unwrap as Future<C>>::Error: Error + Send + 'static,
    <<C as Join<<C as Notify<U>>::Notification>>::Future as Future<C>>::Error:
        Error + Send + 'static,
    <<C as Fork<<C as Notify<T>>::Notification>>::Target as Future<C>>::Error:
        Error + Send + 'static,
    <C as Join<<C as Notify<U>>::Notification>>::Future: Unpin,
    <<C as Finalize<<C as Fork<<C as Notify<T>>::Notification>>::Finalize>>::Output as Future<C>>::Error:
        Error + Send + 'static,
{
    fn call(mut self, args: T) -> U {
        U::flatten(ErasedFnOnceCoalesce {
            state: ErasedFnOnceCoalesceState::Wrap(self.context.wrap(args)),
            context: self.context,
        })
    }
}

#[derive(Debug, Error)]
#[bounds(
    where
        T: Error + 'static,
        E: Error + 'static,
)]
pub enum FnOnceCoalesceError<E, T> {
    #[error("failed to read handle for erased FnOnce context: {0}")]
    Read(T),
    #[error("failed to contextualize erased FnOnce content: {0}")]
    Contextualize(E),
}

pub struct FnOnceCoalesce<
    'a,
    O,
    P: Fn(ErasedFnOnce<T, U, C::Context>) -> O,
    T: Unpin,
    U,
    C: ?Sized + CloneContext,
> where
    C::Context: Unpin
        + Read<<C::Context as Dispatch<<C::Context as Notify<T>>::Notification>>::Handle>
        + Notify<T>,
    <C::Context as Fork<<C::Context as Notify<T>>::Notification>>::Future: Unpin,
    <C::Context as Notify<T>>::Wrap: Unpin,
{
    conv: P,
    lifetime: PhantomData<&'a (O, T, U)>,
    state: FnOnceCoalesceState<C::JoinOutput>,
}

impl<
    'a,
    O,
    P: Fn(ErasedFnOnce<T, U, C::Context>) -> O,
    T: Unpin,
    U,
    C: ?Sized + Read<<C as Contextualize>::Handle> + CloneContext,
> Future<C> for FnOnceCoalesce<'a, O, P, T, U, C>
where
    C::JoinOutput: Unpin,
    C: Unpin,
    C::Context: Unpin,
    P: Unpin,
    <C::Context as Notify<T>>::Wrap: Unpin,
    C::Context: Unpin
        + Read<<C::Context as Dispatch<<C::Context as Notify<T>>::Notification>>::Handle>
        + Notify<T>,
    <C::Context as Fork<<C::Context as Notify<T>>::Notification>>::Future: Unpin,
{
    type Ok = O;
    type Error = FnOnceCoalesceError<<C::JoinOutput as Future<C>>::Error, C::Error>;

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
                FnOnceCoalesceState::Read => {
                    let ct = Pin::new(&mut *ctx);
                    let handle = ready!(ct.read(cx)).map_err(FnOnceCoalesceError::Read)?;
                    this.state = FnOnceCoalesceState::Contextualize(ctx.join_owned(handle));
                }
                FnOnceCoalesceState::Contextualize(future) => {
                    let context = ready!(Pin::new(future).poll(cx, &mut *ctx))
                        .map_err(FnOnceCoalesceError::Contextualize)?;
                    replace(&mut this.state, FnOnceCoalesceState::Done);
                    return Poll::Ready(Ok((this.conv)(ErasedFnOnce {
                        context,
                        data: PhantomData,
                    })));
                }
                FnOnceCoalesceState::Done => panic!("FnOnceCoalesce polled after completion"),
            }
        }
    }
}

macro_rules! tuple_impls {
    ($(($($marker:ident)*) => (
        $($n:tt $name:ident)*
    ))+) => {
        $(
            impl<
                'a,
                $($name: 'a,)*
                U: 'a,
                C: ?Sized + Write<<C as Contextualize>::Handle> + ReferenceContext + Unpin,
            > Unravel<C> for Box<dyn FnOnce($($name,)*) -> U + 'a $(+ $marker)*>
            where
                <C::Context as ContextReference<C>>::Target: Notify<($($name,)*)>
                    + Notify<U>
                    + Read<
                        <<C::Context as ContextReference<C>>::Target as Dispatch<
                            <<C::Context as ContextReference<C>>::Target as Notify<($($name,)*)>>::Notification,
                        >>::Handle,
                    > + Write<
                        <<C::Context as ContextReference<C>>::Target as Dispatch<
                            <<C::Context as ContextReference<C>>::Target as Notify<U>>::Notification,
                        >>::Handle,
                    >,
                U: Unpin,
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
                <<C::Context as ContextReference<C>>::Target as Notify<($($name,)*)>>::Unwrap: Unpin,
                <<C::Context as ContextReference<C>>::Target as Join<
                    <<C::Context as ContextReference<C>>::Target as Notify<($($name,)*)>>::Notification,
                >>::Future: Unpin,
                C::ForkOutput: Unpin,
                C: Unpin,
            {
                type Finalize = ErasedFnOnceUnravel<
                    C,
                    ($($name,)*),
                    U,
                    Box<dyn FnOnce($($name,)*) -> U + 'a>,
                    fn(Box<dyn FnOnce($($name,)*) -> U + 'a>, ($($name,)*)) -> U,
                >;
                type Target = FnOnceUnravel<
                    C,
                    ($($name,)*),
                    U,
                    Box<dyn FnOnce($($name,)*) -> U + 'a>,
                    fn(Box<dyn FnOnce($($name,)*) -> U + 'a>, ($($name,)*)) -> U,
                >;

                #[allow(unused_variables)]
                fn unravel(self) -> Self::Target {
                    FnOnceUnravel {
                        call: Some(self),
                        conv: Some(|call, data| (call)($(data.$n,)*)),
                        context: FnOnceUnravelState::None(PhantomData),
                    }
                }
            }

            impl<
                    'a,
                    $($name: Unpin + 'a,)*
                    U: Flatten<ProtocolError, ErasedFnOnceCoalesce<($($name,)*), U, C::Context>> + 'a $(+ $marker)*,
                    C: Read<<C as Contextualize>::Handle> + CloneContext,
                > Coalesce<C> for Box<dyn FnOnce($($name,)*) -> U + 'a $(+ $marker)*>
            where
                ($($name,)*): 'a $(+ $marker)*,
                C::JoinOutput: Unpin,
                C: Unpin,
                C::Context: 'a $(+ $marker)*,
                <C::Context as Notify<($($name,)*)>>::Wrap: Unpin,
                <C::Context as Notify<U>>::Unwrap: Unpin,
                C::Context: Unpin
                    + Read<<C::Context as Dispatch<<C::Context as Notify<($($name,)*)>>::Notification>>::Handle>
                    + Notify<($($name,)*)>
                    + Notify<U>
                    + Write<<C::Context as Dispatch<<C::Context as Notify<($($name,)*)>>::Notification>>::Handle>
                    + Read<<C::Context as Dispatch<<C::Context as Notify<U>>::Notification>>::Handle>
                    + Finalize<<C::Context as Fork<<C::Context as Notify<($($name,)*)>>::Notification>>::Finalize>,
                <C::Context as Fork<<C::Context as Notify<($($name,)*)>>::Notification>>::Future: Unpin,
                <C::Context as Fork<<C::Context as Notify<($($name,)*)>>::Notification>>::Target: Unpin,
                <C::Context as Fork<<C::Context as Notify<($($name,)*)>>::Notification>>::Finalize: Future<
                    <C::Context as Finalize<
                        <C::Context as Fork<<C::Context as Notify<($($name,)*)>>::Notification>>::Finalize,
                    >>::Target,
                >,
                <C::Context as Finalize<
                    <C::Context as Fork<<C::Context as Notify<($($name,)*)>>::Notification>>::Finalize,
                >>::Output: Unpin,
                <C::Context as Write<<C::Context as Dispatch<<C::Context as Notify<($($name,)*)>>::Notification>>::Handle>>::Error: Error + Send + 'static,
                <<C::Context as Fork<<C::Context as Notify<($($name,)*)>>::Notification>>::Future as Future<C::Context>>::Error: Error + Send + 'static,
                <<C::Context as Notify<($($name,)*)>>::Wrap as Future<C::Context>>::Error: Error + Send + 'static,
                <C::Context as Read<<C::Context as Dispatch<<C::Context as Notify<U>>::Notification>>::Handle>>::Error: Error + Send + 'static,
                <<C::Context as Notify<U>>::Unwrap as Future<C::Context>>::Error: Error + Send + 'static,
                <<C::Context as Join<<C::Context as Notify<U>>::Notification>>::Future as Future<C::Context>>::Error: Error + Send + 'static,
                <<C::Context as Fork<<C::Context as Notify<($($name,)*)>>::Notification>>::Target as Future<C::Context>>::Error: Error + Send + 'static,
                <<C::Context as Finalize<<C::Context as Fork<<C::Context as Notify<($($name,)*)>>::Notification>>::Finalize>>::Output as Future<C::Context>>::Error: Error + Send + 'static,
                <C::Context as Join<<C::Context as Notify<U>>::Notification>>::Future: Unpin,
            {
                type Future = FnOnceCoalesce<'a, Self, fn(ErasedFnOnce<($($name,)*), U, C::Context>) -> Self, ($($name,)*), U, C>;

                #[allow(non_snake_case)]
                fn coalesce() -> Self::Future {
                    FnOnceCoalesce {
                        lifetime: PhantomData,
                        conv: |erased| Box::new(move |$($name,)*| erased.call(($($name,)*))),
                        state: FnOnceCoalesceState::Read,
                    }
                }
            }
        )+
    };
}

macro_rules! marker_variants {
    ($(
        $($marker:ident)*
    ),+) => {
        $(
            tuple_impls! {
                ($($marker)*) => ()
                ($($marker)*) => (0 T0)
                ($($marker)*) => (0 T0 1 T1)
                ($($marker)*) => (0 T0 1 T1 2 T2)
                ($($marker)*) => (0 T0 1 T1 2 T2 3 T3)
                ($($marker)*) => (0 T0 1 T1 2 T2 3 T3 4 T4)
                ($($marker)*) => (0 T0 1 T1 2 T2 3 T3 4 T4 5 T5)
                ($($marker)*) => (0 T0 1 T1 2 T2 3 T3 4 T4 5 T5 6 T6)
                ($($marker)*) => (0 T0 1 T1 2 T2 3 T3 4 T4 5 T5 6 T6 7 T7)
                ($($marker)*) => (0 T0 1 T1 2 T2 3 T3 4 T4 5 T5 6 T6 7 T7 8 T8)
                ($($marker)*) => (0 T0 1 T1 2 T2 3 T3 4 T4 5 T5 6 T6 7 T7 8 T8 9 T9)
                ($($marker)*) => (0 T0 1 T1 2 T2 3 T3 4 T4 5 T5 6 T6 7 T7 8 T8 9 T9 10 T10)
                ($($marker)*) => (0 T0 1 T1 2 T2 3 T3 4 T4 5 T5 6 T6 7 T7 8 T8 9 T9 10 T10 11 T11)
                ($($marker)*) => (0 T0 1 T1 2 T2 3 T3 4 T4 5 T5 6 T6 7 T7 8 T8 9 T9 10 T10 11 T11 12 T12)
                ($($marker)*) => (0 T0 1 T1 2 T2 3 T3 4 T4 5 T5 6 T6 7 T7 8 T8 9 T9 10 T10 11 T11 12 T12 13 T13)
                ($($marker)*) => (0 T0 1 T1 2 T2 3 T3 4 T4 5 T5 6 T6 7 T7 8 T8 9 T9 10 T10 11 T11 12 T12 13 T13 14 T14)
                ($($marker)*) => (0 T0 1 T1 2 T2 3 T3 4 T4 5 T5 6 T6 7 T7 8 T8 9 T9 10 T10 11 T11 12 T12 13 T13 14 T14 15 T15)
            }
        )*
    };
}

marker_variants! {
    ,
    Sync,
    Send, Sync Send
}
