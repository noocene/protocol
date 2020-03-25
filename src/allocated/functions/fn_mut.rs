use crate::{
    allocated::{Flatten, ProtocolError},
    CloneContext, Coalesce, ContextReference, Contextualize, Dispatch, Finalize, FinalizeImmediate,
    Fork, Future, Join, Notify, Read, ReferenceContext, ShareContext, Unravel, Write,
};
use alloc::{vec, boxed::Box, vec::Vec};
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
        F: Error + 'static,
        P: Error + 'static,
)]
pub enum FnMutUnravelError<A, B, C, Z, K, T, I, U, V, W, F, P> {
    #[error("failed to read argument handle for erased FnMut: {0}")]
    Read(#[source] A),
    #[error("failed to join argument for erased FnMut: {0}")]
    Join(#[source] C),
    #[error("failed to unwrap argument notification for erased FnMut: {0}")]
    Unwrap(#[source] B),
    #[error("failed to contextualize erased FnMut: {0}")]
    Contextualize(#[source] Z),
    #[error("failed to contextualize erased FnMut item: {0}")]
    SubContextualize(#[source] P),
    #[error("failed to write context handle for erased FnMut: {0}")]
    Write(#[source] K),
    #[error("failed to write handle for erased FnMut: {0}")]
    Transport(#[source] T),
    #[error("failed to read handle for erased FnMut: {0}")]
    ReadHandle(#[source] F),
    #[error("failed to create notification wrapper for erased FnMut return: {0}")]
    Notify(#[source] I),
    #[error("failed to fork erased FnMut return: {0}")]
    Dispatch(#[source] U),
    #[error("failed to target erased FnMut return: {0}")]
    Target(#[source] V),
    #[error("failed to finalize erased FnMut return: {0}")]
    Finalize(#[source] W),
}

pub enum ErasedFnMutUnravelInstanceState<C: ?Sized + ReferenceContext, T, U>
where
    <C::Context as ContextReference<C>>::Target: ReferenceContext,
    <<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
        <C::Context as ContextReference<C>>::Target,
    >>::Target: Notify<T>
        + Notify<U>
        + Read<
            <<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                <C::Context as ContextReference<C>>::Target,
            >>::Target as Dispatch<
                <<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                    <C::Context as ContextReference<C>>::Target,
                >>::Target as Notify<T>>::Notification,
            >>::Handle,
        > + Write<
            <<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                <C::Context as ContextReference<C>>::Target,
            >>::Target as Dispatch<
                <<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                    <C::Context as ContextReference<C>>::Target,
                >>::Target as Notify<U>>::Notification,
            >>::Handle,
        >,
{
    Read,
    Join(
        <<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
            <C::Context as ContextReference<C>>::Target,
        >>::Target as Join<
            <<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                <C::Context as ContextReference<C>>::Target,
            >>::Target as Notify<T>>::Notification,
        >>::Future,
    ),
    Unwrap(
        <<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
            <C::Context as ContextReference<C>>::Target,
        >>::Target as Notify<T>>::Unwrap,
    ),
    Wrap(
        <<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
            <C::Context as ContextReference<C>>::Target,
        >>::Target as Notify<U>>::Wrap,
    ),
    Fork(
        <<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
            <C::Context as ContextReference<C>>::Target,
        >>::Target as Fork<
            <<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                <C::Context as ContextReference<C>>::Target,
            >>::Target as Notify<U>>::Notification,
        >>::Future,
    ),
    Write(
        <<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
            <C::Context as ContextReference<C>>::Target,
        >>::Target as Fork<
            <<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                <C::Context as ContextReference<C>>::Target,
            >>::Target as Notify<U>>::Notification,
        >>::Target,
        <<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
            <C::Context as ContextReference<C>>::Target,
        >>::Target as Dispatch<
            <<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                <C::Context as ContextReference<C>>::Target,
            >>::Target as Notify<U>>::Notification,
        >>::Handle,
    ),
    Flush(
        <<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
            <C::Context as ContextReference<C>>::Target,
        >>::Target as Fork<
            <<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                <C::Context as ContextReference<C>>::Target,
            >>::Target as Notify<U>>::Notification,
        >>::Target,
    ),
    Target(
        <<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
            <C::Context as ContextReference<C>>::Target,
        >>::Target as Fork<
            <<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                <C::Context as ContextReference<C>>::Target,
            >>::Target as Notify<U>>::Notification,
        >>::Target,
    ),
    Finalize(
        <<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
            <C::Context as ContextReference<C>>::Target,
        >>::Target as Fork<
            <<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                <C::Context as ContextReference<C>>::Target,
            >>::Target as Notify<U>>::Notification,
        >>::Finalize,
    ),
    Done,
}

pub struct ErasedFnMutUnravelInstance<C: ?Sized + ReferenceContext, T, U, F, M>
where
    <C::Context as ContextReference<C>>::Target: ReferenceContext,
    <<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
        <C::Context as ContextReference<C>>::Target,
    >>::Target: Notify<T>
        + Notify<U>
        + Read<
            <<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                <C::Context as ContextReference<C>>::Target,
            >>::Target as Dispatch<
                <<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                    <C::Context as ContextReference<C>>::Target,
                >>::Target as Notify<T>>::Notification,
            >>::Handle,
        > + Write<
            <<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                <C::Context as ContextReference<C>>::Target,
            >>::Target as Dispatch<
                <<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                    <C::Context as ContextReference<C>>::Target,
                >>::Target as Notify<U>>::Notification,
            >>::Handle,
        >,
{
    state: ErasedFnMutUnravelInstanceState<C, T, U>,
    marker: PhantomData<(F, M)>,
    context: <<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context,
}

impl<C: ?Sized + ReferenceContext, T, U, F, M> Unpin for ErasedFnMutUnravelInstance<C, T, U, F, M>
where
    <C::Context as ContextReference<C>>::Target: ReferenceContext,
    <<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
        <C::Context as ContextReference<C>>::Target,
    >>::Target: Notify<T>
        + Notify<U>
        + Read<
            <<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                <C::Context as ContextReference<C>>::Target,
            >>::Target as Dispatch<
                <<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                    <C::Context as ContextReference<C>>::Target,
                >>::Target as Notify<T>>::Notification,
            >>::Handle,
        > + Write<
            <<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                <C::Context as ContextReference<C>>::Target,
            >>::Target as Dispatch<
                <<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                    <C::Context as ContextReference<C>>::Target,
                >>::Target as Notify<U>>::Notification,
            >>::Handle,
        >,
{
}

impl<C: ?Sized + ReferenceContext + Write<<C as Contextualize>::Handle>, T, U, F, M>
    ErasedFnMutUnravelInstance<C, T, U, F, M>
where
    for<'a> M: Fn(&'a mut F, T) -> U,
    <C::Context as ContextReference<C>>::Target: ReferenceContext
        + Read<Option<<<C::Context as ContextReference<C>>::Target as Contextualize>::Handle>>,
    <<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
        <C::Context as ContextReference<C>>::Target,
    >>::Target: Notify<T>
        + Notify<U>
        + Read<
            <<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                <C::Context as ContextReference<C>>::Target,
            >>::Target as Dispatch<
                <<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                    <C::Context as ContextReference<C>>::Target,
                >>::Target as Notify<T>>::Notification,
            >>::Handle,
        > + Write<
            <<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                <C::Context as ContextReference<C>>::Target,
            >>::Target as Dispatch<
                <<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                    <C::Context as ContextReference<C>>::Target,
                >>::Target as Notify<U>>::Notification,
            >>::Handle,
        >,
    <<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
        <C::Context as ContextReference<C>>::Target,
    >>::Target: Unpin,
    <<<<<C as ReferenceContext>::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
        <<C as ReferenceContext>::Context as ContextReference<C>>::Target,
    >>::Target as Join<
        <<<<<C as ReferenceContext>::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
            <<C as ReferenceContext>::Context as ContextReference<C>>::Target,
        >>::Target as Notify<T>>::Notification,
    >>::Future: Unpin,
    <<<<<C as ReferenceContext>::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
        <<C as ReferenceContext>::Context as ContextReference<C>>::Target,
    >>::Target as Notify<T>>::Unwrap: Unpin,
    <<<<<C as ReferenceContext>::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
        <<C as ReferenceContext>::Context as ContextReference<C>>::Target,
    >>::Target as Fork<
        <<<<<C as ReferenceContext>::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
            <<C as ReferenceContext>::Context as ContextReference<C>>::Target,
        >>::Target as Notify<U>>::Notification,
    >>::Target: Unpin,
    <<<<<C as ReferenceContext>::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
        <<C as ReferenceContext>::Context as ContextReference<C>>::Target,
    >>::Target as Fork<
        <<<<<C as ReferenceContext>::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
            <<C as ReferenceContext>::Context as ContextReference<C>>::Target,
        >>::Target as Notify<U>>::Notification,
    >>::Finalize: Unpin,
    <<<<<C as ReferenceContext>::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
        <<C as ReferenceContext>::Context as ContextReference<C>>::Target,
    >>::Target as Notify<U>>::Wrap: Unpin,
    <<<<<C as ReferenceContext>::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
        <<C as ReferenceContext>::Context as ContextReference<C>>::Target,
    >>::Target as Fork<
        <<<<<C as ReferenceContext>::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
            <<C as ReferenceContext>::Context as ContextReference<C>>::Target,
        >>::Target as Notify<U>>::Notification,
    >>::Future: Unpin,
{
    fn poll<R: BorrowMut<<C::Context as ContextReference<C>>::Target>>(
        mut self: Pin<&mut Self>,
        cx: &mut Context,
        ctx: R,
        conv: &M,
        call: &mut F,
    ) -> Poll<
        Result<
            (),
            FnMutUnravelError<
                <<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                    <C::Context as ContextReference<C>>::Target,
                >>::Target as Read<
                    <<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                        <C::Context as ContextReference<C>>::Target,
                    >>::Target as Dispatch<
                        <<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                            <C::Context as ContextReference<C>>::Target,
                        >>::Target as Notify<T>>::Notification,
                    >>::Handle,
                >>::Error,
                <<<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                    <C::Context as ContextReference<C>>::Target,
                >>::Target as Notify<T>>::Unwrap as Future<
                    <<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                        <C::Context as ContextReference<C>>::Target,
                    >>::Target,
                >>::Error,
                <<<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                    <C::Context as ContextReference<C>>::Target,
                >>::Target as Join<
                    <<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                        <C::Context as ContextReference<C>>::Target,
                    >>::Target as Notify<T>>::Notification,
                >>::Future as Future<
                    <<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                        <C::Context as ContextReference<C>>::Target,
                    >>::Target,
                >>::Error,
                <C::ForkOutput as Future<C>>::Error,
                C::Error,
                <<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                    <C::Context as ContextReference<C>>::Target,
                >>::Target as Write<
                    <<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                        <C::Context as ContextReference<C>>::Target,
                    >>::Target as Dispatch<
                        <<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                            <C::Context as ContextReference<C>>::Target,
                        >>::Target as Notify<U>>::Notification,
                    >>::Handle,
                >>::Error,
                <<<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                    <C::Context as ContextReference<C>>::Target,
                >>::Target as Notify<U>>::Wrap as Future<
                    <<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                        <C::Context as ContextReference<C>>::Target,
                    >>::Target,
                >>::Error,
                <<<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                    <C::Context as ContextReference<C>>::Target,
                >>::Target as Fork<
                    <<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                        <C::Context as ContextReference<C>>::Target,
                    >>::Target as Notify<U>>::Notification,
                >>::Future as Future<
                    <<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                        <C::Context as ContextReference<C>>::Target,
                    >>::Target,
                >>::Error,
                <<<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                    <C::Context as ContextReference<C>>::Target,
                >>::Target as Fork<
                    <<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                        <C::Context as ContextReference<C>>::Target,
                    >>::Target as Notify<U>>::Notification,
                >>::Target as Future<
                    <<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                        <C::Context as ContextReference<C>>::Target,
                    >>::Target,
                >>::Error,
                <<<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                    <C::Context as ContextReference<C>>::Target,
                >>::Target as Fork<
                    <<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                        <C::Context as ContextReference<C>>::Target,
                    >>::Target as Notify<U>>::Notification,
                >>::Finalize as Future<
                    <<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                        <C::Context as ContextReference<C>>::Target,
                    >>::Target,
                >>::Error,
                <<C::Context as ContextReference<C>>::Target as Read<
                    Option<<<C::Context as ContextReference<C>>::Target as Contextualize>::Handle>,
                >>::Error,
                <<<C::Context as ContextReference<C>>::Target as ReferenceContext>::JoinOutput as Future<
                    <C::Context as ContextReference<C>>::Target,
                >>::Error,
            >,
        >,
    > {
        let this = &mut *self;

        let ctx = this.context.with(ctx);

        loop {
            match &mut this.state {
                ErasedFnMutUnravelInstanceState::Read => {
                    let mut ct = Pin::new(&mut *ctx);
                    let handle = ready!(ct.as_mut().read(cx)).map_err(FnMutUnravelError::Read)?;
                    this.state = ErasedFnMutUnravelInstanceState::Join(Join::<
                        <<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                            <C::Context as ContextReference<C>>::Target,
                        >>::Target as Notify<T>>::Notification,
                    >::join(
                        &mut *ctx, handle
                    ));
                }
                ErasedFnMutUnravelInstanceState::Join(future) => {
                    let notification = ready!(Pin::new(future).poll(cx, &mut *ctx))
                        .map_err(FnMutUnravelError::Join)?;
                    this.state = ErasedFnMutUnravelInstanceState::Unwrap(
                        <<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                            <C::Context as ContextReference<C>>::Target,
                        >>::Target as Notify<T>>::unwrap(
                            &mut *ctx, notification
                        ),
                    );
                }
                ErasedFnMutUnravelInstanceState::Unwrap(future) => {
                    let item = ready!(Pin::new(future).poll(cx, &mut *ctx))
                        .map_err(FnMutUnravelError::Unwrap)?;
                    let item = (conv)(call, item);
                    this.state = ErasedFnMutUnravelInstanceState::Wrap(ctx.wrap(item));
                }
                ErasedFnMutUnravelInstanceState::Wrap(future) => {
                    let item = ready!(Pin::new(future).poll(cx, &mut *ctx))
                        .map_err(FnMutUnravelError::Notify)?;
                    this.state = ErasedFnMutUnravelInstanceState::Fork(ctx.fork(item));
                }
                ErasedFnMutUnravelInstanceState::Fork(future) => {
                    let (target, handle) = ready!(Pin::new(future).poll(cx, &mut *ctx))
                        .map_err(FnMutUnravelError::Dispatch)?;
                    this.state = ErasedFnMutUnravelInstanceState::Write(target, handle);
                }
                ErasedFnMutUnravelInstanceState::Write(_, _) => {
                    let mut ctx = Pin::new(&mut *ctx);
                    ready!(ctx.as_mut().poll_ready(cx)).map_err(FnMutUnravelError::Transport)?;
                    let data = replace(&mut this.state, ErasedFnMutUnravelInstanceState::Done);
                    if let ErasedFnMutUnravelInstanceState::Write(target, data) = data {
                        ctx.write(data).map_err(FnMutUnravelError::Transport)?;
                        this.state = ErasedFnMutUnravelInstanceState::Flush(target);
                    } else {
                        panic!("invalid state in ErasedFnMutUnravel Write")
                    }
                }
                ErasedFnMutUnravelInstanceState::Flush(_) => {
                    ready!(Pin::new(&mut *ctx).poll_ready(cx))
                        .map_err(FnMutUnravelError::Transport)?;
                    let data = replace(&mut this.state, ErasedFnMutUnravelInstanceState::Done);
                    if let ErasedFnMutUnravelInstanceState::Flush(target) = data {
                        this.state = ErasedFnMutUnravelInstanceState::Target(target);
                    } else {
                        panic!("invalid state in ErasedFnMutUnravel Write")
                    }
                }
                ErasedFnMutUnravelInstanceState::Target(target) => {
                    let finalize = ready!(Pin::new(target).poll(cx, &mut *ctx))
                        .map_err(FnMutUnravelError::Target)?;
                    this.state = ErasedFnMutUnravelInstanceState::Finalize(finalize);
                }
                ErasedFnMutUnravelInstanceState::Finalize(finalize) => {
                    ready!(Pin::new(finalize).poll(cx, &mut *ctx))
                        .map_err(FnMutUnravelError::Finalize)?;
                    this.state = ErasedFnMutUnravelInstanceState::Done;
                    return Poll::Ready(Ok(()));
                }
                ErasedFnMutUnravelInstanceState::Done => {
                    panic!("ErasedFnMutUnravel polled after completion")
                }
            }
        }
    }
}

enum FnMutUnravelState<C: ?Sized + Write<<C as Contextualize>::Handle> + ReferenceContext, T, U>
where
    <C::Context as ContextReference<C>>::Target: ReferenceContext,
    <<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
        <C::Context as ContextReference<C>>::Target,
    >>::Target: Notify<T>
        + Notify<U>
        + Read<
            <<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                <C::Context as ContextReference<C>>::Target,
            >>::Target as Dispatch<
                <<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                    <C::Context as ContextReference<C>>::Target,
                >>::Target as Notify<T>>::Notification,
            >>::Handle,
        > + Write<
            <<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                <C::Context as ContextReference<C>>::Target,
            >>::Target as Dispatch<
                <<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                    <C::Context as ContextReference<C>>::Target,
                >>::Target as Notify<U>>::Notification,
            >>::Handle,
        >,
    <<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
        <C::Context as ContextReference<C>>::Target,
    >>::Target: Unpin,
    <<<<<C as ReferenceContext>::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
        <<C as ReferenceContext>::Context as ContextReference<C>>::Target,
    >>::Target as Join<
        <<<<<C as ReferenceContext>::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
            <<C as ReferenceContext>::Context as ContextReference<C>>::Target,
        >>::Target as Notify<T>>::Notification,
    >>::Future: Unpin,
    <<<<<C as ReferenceContext>::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
        <<C as ReferenceContext>::Context as ContextReference<C>>::Target,
    >>::Target as Notify<T>>::Unwrap: Unpin,
    <<<<<C as ReferenceContext>::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
        <<C as ReferenceContext>::Context as ContextReference<C>>::Target,
    >>::Target as Fork<
        <<<<<C as ReferenceContext>::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
            <<C as ReferenceContext>::Context as ContextReference<C>>::Target,
        >>::Target as Notify<U>>::Notification,
    >>::Target: Unpin,
    <<<<<C as ReferenceContext>::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
        <<C as ReferenceContext>::Context as ContextReference<C>>::Target,
    >>::Target as Fork<
        <<<<<C as ReferenceContext>::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
            <<C as ReferenceContext>::Context as ContextReference<C>>::Target,
        >>::Target as Notify<U>>::Notification,
    >>::Finalize: Unpin,
    <<<<<C as ReferenceContext>::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
        <<C as ReferenceContext>::Context as ContextReference<C>>::Target,
    >>::Target as Notify<U>>::Wrap: Unpin,
    <<<<<C as ReferenceContext>::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
        <<C as ReferenceContext>::Context as ContextReference<C>>::Target,
    >>::Target as Fork<
        <<<<<C as ReferenceContext>::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
            <<C as ReferenceContext>::Context as ContextReference<C>>::Target,
        >>::Target as Notify<U>>::Notification,
    >>::Future: Unpin,
{
    None(PhantomData<(T, U)>),
    Context(C::ForkOutput),
    Write(C::Context, C::Handle),
    Flush(C::Context),
}

pub struct FnMutUnravel<
    C: ?Sized + Write<<C as Contextualize>::Handle> + ReferenceContext,
    T,
    U,
    F,
    M,
> where
    for<'a> M: Fn(&'a mut F, T) -> U,
    <C::Context as ContextReference<C>>::Target: ReferenceContext,
    <<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
        <C::Context as ContextReference<C>>::Target,
    >>::Target: Notify<T>
        + Notify<U>
        + Read<
            <<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                <C::Context as ContextReference<C>>::Target,
            >>::Target as Dispatch<
                <<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                    <C::Context as ContextReference<C>>::Target,
                >>::Target as Notify<T>>::Notification,
            >>::Handle,
        > + Write<
            <<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                <C::Context as ContextReference<C>>::Target,
            >>::Target as Dispatch<
                <<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                    <C::Context as ContextReference<C>>::Target,
                >>::Target as Notify<U>>::Notification,
            >>::Handle,
        >,
    <<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
        <C::Context as ContextReference<C>>::Target,
    >>::Target: Unpin,
    <<<<<C as ReferenceContext>::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
        <<C as ReferenceContext>::Context as ContextReference<C>>::Target,
    >>::Target as Join<
        <<<<<C as ReferenceContext>::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
            <<C as ReferenceContext>::Context as ContextReference<C>>::Target,
        >>::Target as Notify<T>>::Notification,
    >>::Future: Unpin,
    <<<<<C as ReferenceContext>::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
        <<C as ReferenceContext>::Context as ContextReference<C>>::Target,
    >>::Target as Notify<T>>::Unwrap: Unpin,
    <<<<<C as ReferenceContext>::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
        <<C as ReferenceContext>::Context as ContextReference<C>>::Target,
    >>::Target as Fork<
        <<<<<C as ReferenceContext>::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
            <<C as ReferenceContext>::Context as ContextReference<C>>::Target,
        >>::Target as Notify<U>>::Notification,
    >>::Target: Unpin,
    <<<<<C as ReferenceContext>::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
        <<C as ReferenceContext>::Context as ContextReference<C>>::Target,
    >>::Target as Fork<
        <<<<<C as ReferenceContext>::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
            <<C as ReferenceContext>::Context as ContextReference<C>>::Target,
        >>::Target as Notify<U>>::Notification,
    >>::Finalize: Unpin,
    <<<<<C as ReferenceContext>::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
        <<C as ReferenceContext>::Context as ContextReference<C>>::Target,
    >>::Target as Notify<U>>::Wrap: Unpin,
    <<<<<C as ReferenceContext>::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
        <<C as ReferenceContext>::Context as ContextReference<C>>::Target,
    >>::Target as Fork<
        <<<<<C as ReferenceContext>::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
            <<C as ReferenceContext>::Context as ContextReference<C>>::Target,
        >>::Target as Notify<U>>::Notification,
    >>::Future: Unpin,
{
    call: Option<F>,
    context: FnMutUnravelState<C, T, U>,
    conv: Option<M>,
}

enum ErasedFnMutUnravelState<
    C: ?Sized + Write<<C as Contextualize>::Handle> + ReferenceContext,
    T,
    U,
> where
    <C::Context as ContextReference<C>>::Target: ReferenceContext,
    <<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
        <C::Context as ContextReference<C>>::Target,
    >>::Target: Notify<T>
        + Notify<U>
        + Read<
            <<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                <C::Context as ContextReference<C>>::Target,
            >>::Target as Dispatch<
                <<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                    <C::Context as ContextReference<C>>::Target,
                >>::Target as Notify<T>>::Notification,
            >>::Handle,
        > + Write<
            <<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                <C::Context as ContextReference<C>>::Target,
            >>::Target as Dispatch<
                <<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                    <C::Context as ContextReference<C>>::Target,
                >>::Target as Notify<U>>::Notification,
            >>::Handle,
        >,
    <<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
        <C::Context as ContextReference<C>>::Target,
    >>::Target: Unpin,
    <<<<<C as ReferenceContext>::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
        <<C as ReferenceContext>::Context as ContextReference<C>>::Target,
    >>::Target as Join<
        <<<<<C as ReferenceContext>::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
            <<C as ReferenceContext>::Context as ContextReference<C>>::Target,
        >>::Target as Notify<T>>::Notification,
    >>::Future: Unpin,
    <<<<<C as ReferenceContext>::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
        <<C as ReferenceContext>::Context as ContextReference<C>>::Target,
    >>::Target as Notify<T>>::Unwrap: Unpin,
    <<<<<C as ReferenceContext>::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
        <<C as ReferenceContext>::Context as ContextReference<C>>::Target,
    >>::Target as Fork<
        <<<<<C as ReferenceContext>::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
            <<C as ReferenceContext>::Context as ContextReference<C>>::Target,
        >>::Target as Notify<U>>::Notification,
    >>::Target: Unpin,
    <<<<<C as ReferenceContext>::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
        <<C as ReferenceContext>::Context as ContextReference<C>>::Target,
    >>::Target as Fork<
        <<<<<C as ReferenceContext>::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
            <<C as ReferenceContext>::Context as ContextReference<C>>::Target,
        >>::Target as Notify<U>>::Notification,
    >>::Finalize: Unpin,
    <<<<<C as ReferenceContext>::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
        <<C as ReferenceContext>::Context as ContextReference<C>>::Target,
    >>::Target as Notify<U>>::Wrap: Unpin,
    <<<<<C as ReferenceContext>::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
        <<C as ReferenceContext>::Context as ContextReference<C>>::Target,
    >>::Target as Fork<
        <<<<<C as ReferenceContext>::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
            <<C as ReferenceContext>::Context as ContextReference<C>>::Target,
        >>::Target as Notify<U>>::Notification,
    >>::Future: Unpin,
{
    Read(PhantomData<(T, U)>),
    Contextualize(<<C::Context as ContextReference<C>>::Target as ReferenceContext>::JoinOutput),
    Done,
}

pub struct ErasedFnMutUnravel<
    C: ?Sized + Write<<C as Contextualize>::Handle> + ReferenceContext,
    T,
    U,
    F,
    M,
> where
    for<'a> M: Fn(&'a mut F, T) -> U,
    <C::Context as ContextReference<C>>::Target: ReferenceContext
        + Read<Option<<<C::Context as ContextReference<C>>::Target as Contextualize>::Handle>>,
    <<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
        <C::Context as ContextReference<C>>::Target,
    >>::Target: Notify<T>
        + Notify<U>
        + Read<
            <<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                <C::Context as ContextReference<C>>::Target,
            >>::Target as Dispatch<
                <<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                    <C::Context as ContextReference<C>>::Target,
                >>::Target as Notify<T>>::Notification,
            >>::Handle,
        > + Write<
            <<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                <C::Context as ContextReference<C>>::Target,
            >>::Target as Dispatch<
                <<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                    <C::Context as ContextReference<C>>::Target,
                >>::Target as Notify<U>>::Notification,
            >>::Handle,
        >,
    <<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
        <C::Context as ContextReference<C>>::Target,
    >>::Target: Unpin,
    <<<<<C as ReferenceContext>::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
        <<C as ReferenceContext>::Context as ContextReference<C>>::Target,
    >>::Target as Join<
        <<<<<C as ReferenceContext>::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
            <<C as ReferenceContext>::Context as ContextReference<C>>::Target,
        >>::Target as Notify<T>>::Notification,
    >>::Future: Unpin,
    <<<<<C as ReferenceContext>::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
        <<C as ReferenceContext>::Context as ContextReference<C>>::Target,
    >>::Target as Notify<T>>::Unwrap: Unpin,
    <<<<<C as ReferenceContext>::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
        <<C as ReferenceContext>::Context as ContextReference<C>>::Target,
    >>::Target as Fork<
        <<<<<C as ReferenceContext>::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
            <<C as ReferenceContext>::Context as ContextReference<C>>::Target,
        >>::Target as Notify<U>>::Notification,
    >>::Target: Unpin,
    <<<<<C as ReferenceContext>::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
        <<C as ReferenceContext>::Context as ContextReference<C>>::Target,
    >>::Target as Fork<
        <<<<<C as ReferenceContext>::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
            <<C as ReferenceContext>::Context as ContextReference<C>>::Target,
        >>::Target as Notify<U>>::Notification,
    >>::Finalize: Unpin,
    <<<<<C as ReferenceContext>::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
        <<C as ReferenceContext>::Context as ContextReference<C>>::Target,
    >>::Target as Notify<U>>::Wrap: Unpin,
    <<<<<C as ReferenceContext>::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
        <<C as ReferenceContext>::Context as ContextReference<C>>::Target,
    >>::Target as Fork<
        <<<<<C as ReferenceContext>::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
            <<C as ReferenceContext>::Context as ContextReference<C>>::Target,
        >>::Target as Notify<U>>::Notification,
    >>::Future: Unpin,
{
    call: M,
    pending: Vec<ErasedFnMutUnravelInstance<C, T, U, F, M>>,
    state: ErasedFnMutUnravelState<C, T, U>,
    context: C::Context,
    conv: F,
}

impl<C: ?Sized + Write<<C as Contextualize>::Handle> + ReferenceContext, T, U, F, M> Unpin
    for ErasedFnMutUnravel<C, T, U, F, M>
where
    for<'a> M: Fn(&'a mut F, T) -> U,
    <C::Context as ContextReference<C>>::Target: ReferenceContext
        + Read<Option<<<C::Context as ContextReference<C>>::Target as Contextualize>::Handle>>,
    <<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
        <C::Context as ContextReference<C>>::Target,
    >>::Target: Notify<T>
        + Notify<U>
        + Read<
            <<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                <C::Context as ContextReference<C>>::Target,
            >>::Target as Dispatch<
                <<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                    <C::Context as ContextReference<C>>::Target,
                >>::Target as Notify<T>>::Notification,
            >>::Handle,
        > + Write<
            <<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                <C::Context as ContextReference<C>>::Target,
            >>::Target as Dispatch<
                <<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                    <C::Context as ContextReference<C>>::Target,
                >>::Target as Notify<U>>::Notification,
            >>::Handle,
        >,
    <<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
        <C::Context as ContextReference<C>>::Target,
    >>::Target: Unpin,
    <<<<<C as ReferenceContext>::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
        <<C as ReferenceContext>::Context as ContextReference<C>>::Target,
    >>::Target as Join<
        <<<<<C as ReferenceContext>::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
            <<C as ReferenceContext>::Context as ContextReference<C>>::Target,
        >>::Target as Notify<T>>::Notification,
    >>::Future: Unpin,
    <<<<<C as ReferenceContext>::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
        <<C as ReferenceContext>::Context as ContextReference<C>>::Target,
    >>::Target as Notify<T>>::Unwrap: Unpin,
    <<<<<C as ReferenceContext>::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
        <<C as ReferenceContext>::Context as ContextReference<C>>::Target,
    >>::Target as Fork<
        <<<<<C as ReferenceContext>::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
            <<C as ReferenceContext>::Context as ContextReference<C>>::Target,
        >>::Target as Notify<U>>::Notification,
    >>::Target: Unpin,
    <<<<<C as ReferenceContext>::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
        <<C as ReferenceContext>::Context as ContextReference<C>>::Target,
    >>::Target as Fork<
        <<<<<C as ReferenceContext>::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
            <<C as ReferenceContext>::Context as ContextReference<C>>::Target,
        >>::Target as Notify<U>>::Notification,
    >>::Finalize: Unpin,
    <<<<<C as ReferenceContext>::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
        <<C as ReferenceContext>::Context as ContextReference<C>>::Target,
    >>::Target as Notify<U>>::Wrap: Unpin,
    <<<<<C as ReferenceContext>::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
        <<C as ReferenceContext>::Context as ContextReference<C>>::Target,
    >>::Target as Fork<
        <<<<<C as ReferenceContext>::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
            <<C as ReferenceContext>::Context as ContextReference<C>>::Target,
        >>::Target as Notify<U>>::Notification,
    >>::Future: Unpin,
{
}

impl<C: ?Sized + Write<<C as Contextualize>::Handle> + ReferenceContext, T, U, F, M> Future<C>
    for ErasedFnMutUnravel<C, T, U, F, M>
where
    for<'a> M: Fn(&'a mut F, T) -> U,
    <C::Context as ContextReference<C>>::Target: ReferenceContext
        + Read<Option<<<C::Context as ContextReference<C>>::Target as Contextualize>::Handle>>,
    <<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
        <C::Context as ContextReference<C>>::Target,
    >>::Target: Notify<T>
        + Notify<U>
        + Read<
            <<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                <C::Context as ContextReference<C>>::Target,
            >>::Target as Dispatch<
                <<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                    <C::Context as ContextReference<C>>::Target,
                >>::Target as Notify<T>>::Notification,
            >>::Handle,
        > + Write<
            <<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                <C::Context as ContextReference<C>>::Target,
            >>::Target as Dispatch<
                <<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                    <C::Context as ContextReference<C>>::Target,
                >>::Target as Notify<U>>::Notification,
            >>::Handle,
        >,
    <<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
        <C::Context as ContextReference<C>>::Target,
    >>::Target: Unpin,
    <<<<<C as ReferenceContext>::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
        <<C as ReferenceContext>::Context as ContextReference<C>>::Target,
    >>::Target as Join<
        <<<<<C as ReferenceContext>::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
            <<C as ReferenceContext>::Context as ContextReference<C>>::Target,
        >>::Target as Notify<T>>::Notification,
    >>::Future: Unpin,
    <<<<<C as ReferenceContext>::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
        <<C as ReferenceContext>::Context as ContextReference<C>>::Target,
    >>::Target as Notify<T>>::Unwrap: Unpin,
    <<<<<C as ReferenceContext>::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
        <<C as ReferenceContext>::Context as ContextReference<C>>::Target,
    >>::Target as Fork<
        <<<<<C as ReferenceContext>::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
            <<C as ReferenceContext>::Context as ContextReference<C>>::Target,
        >>::Target as Notify<U>>::Notification,
    >>::Target: Unpin,
    <<<<<C as ReferenceContext>::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
        <<C as ReferenceContext>::Context as ContextReference<C>>::Target,
    >>::Target as Fork<
        <<<<<C as ReferenceContext>::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
            <<C as ReferenceContext>::Context as ContextReference<C>>::Target,
        >>::Target as Notify<U>>::Notification,
    >>::Finalize: Unpin,
    <<<<<C as ReferenceContext>::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
        <<C as ReferenceContext>::Context as ContextReference<C>>::Target,
    >>::Target as Notify<U>>::Wrap: Unpin,
    <<<<<C as ReferenceContext>::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
        <<C as ReferenceContext>::Context as ContextReference<C>>::Target,
    >>::Target as Fork<
        <<<<<C as ReferenceContext>::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
            <<C as ReferenceContext>::Context as ContextReference<C>>::Target,
        >>::Target as Notify<U>>::Notification,
    >>::Future: Unpin,
    <<C as ReferenceContext>::Context as ContextReference<C>>::Target: Unpin,
    <<<C as ReferenceContext>::Context as ContextReference<C>>::Target as ReferenceContext>::JoinOutput:
        Unpin,
{
    type Ok = ();
    type Error = FnMutUnravelError<
        <<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
            <C::Context as ContextReference<C>>::Target,
        >>::Target as Read<
            <<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                <C::Context as ContextReference<C>>::Target,
            >>::Target as Dispatch<
                <<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                    <C::Context as ContextReference<C>>::Target,
                >>::Target as Notify<T>>::Notification,
            >>::Handle,
        >>::Error,
        <<<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
            <C::Context as ContextReference<C>>::Target,
        >>::Target as Notify<T>>::Unwrap as Future<
            <<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                <C::Context as ContextReference<C>>::Target,
            >>::Target,
        >>::Error,
        <<<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
            <C::Context as ContextReference<C>>::Target,
        >>::Target as Join<
            <<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                <C::Context as ContextReference<C>>::Target,
            >>::Target as Notify<T>>::Notification,
        >>::Future as Future<
            <<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                <C::Context as ContextReference<C>>::Target,
            >>::Target,
        >>::Error,
        <C::ForkOutput as Future<C>>::Error,
        C::Error,
        <<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
            <C::Context as ContextReference<C>>::Target,
        >>::Target as Write<
            <<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                <C::Context as ContextReference<C>>::Target,
            >>::Target as Dispatch<
                <<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                    <C::Context as ContextReference<C>>::Target,
                >>::Target as Notify<U>>::Notification,
            >>::Handle,
        >>::Error,
        <<<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
            <C::Context as ContextReference<C>>::Target,
        >>::Target as Notify<U>>::Wrap as Future<
            <<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                <C::Context as ContextReference<C>>::Target,
            >>::Target,
        >>::Error,
        <<<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
            <C::Context as ContextReference<C>>::Target,
        >>::Target as Fork<
            <<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                <C::Context as ContextReference<C>>::Target,
            >>::Target as Notify<U>>::Notification,
        >>::Future as Future<
            <<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                <C::Context as ContextReference<C>>::Target,
            >>::Target,
        >>::Error,
        <<<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
            <C::Context as ContextReference<C>>::Target,
        >>::Target as Fork<
            <<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                <C::Context as ContextReference<C>>::Target,
            >>::Target as Notify<U>>::Notification,
        >>::Target as Future<
            <<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                <C::Context as ContextReference<C>>::Target,
            >>::Target,
        >>::Error,
        <<<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
            <C::Context as ContextReference<C>>::Target,
        >>::Target as Fork<
            <<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                <C::Context as ContextReference<C>>::Target,
            >>::Target as Notify<U>>::Notification,
        >>::Finalize as Future<
            <<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                <C::Context as ContextReference<C>>::Target,
            >>::Target,
        >>::Error,
        <<C::Context as ContextReference<C>>::Target as Read<
            Option<<<C::Context as ContextReference<C>>::Target as Contextualize>::Handle>,
        >>::Error,
        <<<C::Context as ContextReference<C>>::Target as ReferenceContext>::JoinOutput as Future<
            <C::Context as ContextReference<C>>::Target,
        >>::Error,
    >;

    fn poll<R: BorrowMut<C>>(
        mut self: Pin<&mut Self>,
        cx: &mut Context,
        ctx: R,
    ) -> Poll<Result<Self::Ok, Self::Error>> {
        let this = &mut *self;
        let context = this.context.with(ctx);
        let mut i = 0;
        while i != this.pending.len() {
            if let Poll::Ready(()) = Pin::new(&mut this.pending[i]).poll(
                cx,
                context.borrow_mut(),
                &this.call,
                &mut this.conv,
            )? {
                this.pending.remove(i);
            } else {
                i += 1;
            }
        }

        loop {
            match &mut this.state {
                ErasedFnMutUnravelState::Read(_) => {
                    let handle = ready!(Pin::new(&mut *context).read(cx))
                        .map_err(FnMutUnravelError::ReadHandle)?;
                    this.state = if let Some(handle) = handle {
                        ErasedFnMutUnravelState::Contextualize(context.join_ref(handle))
                    } else {
                        ErasedFnMutUnravelState::Done
                    };
                }
                ErasedFnMutUnravelState::Contextualize(future) => {
                    let ct = ready!(Pin::new(future).poll(cx, &mut *context))
                        .map_err(FnMutUnravelError::SubContextualize)?;
                    let mut fut = ErasedFnMutUnravelInstance {
                        state: ErasedFnMutUnravelInstanceState::Read,
                        context: ct,
                        marker: PhantomData,
                    };
                    if let Poll::Pending = Pin::new(&mut fut).poll(
                        cx,
                        context.borrow_mut(),
                        &this.call,
                        &mut this.conv,
                    )? {
                        this.pending.push(fut);
                    }
                    this.state = ErasedFnMutUnravelState::Read(PhantomData);
                }
                ErasedFnMutUnravelState::Done => {
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

impl<C: ?Sized + Write<<C as Contextualize>::Handle> + ReferenceContext, T, U, F, M> Unpin
    for FnMutUnravel<C, T, U, F, M>
where
    for<'a> M: Fn(&'a mut F, T) -> U,
    <C::Context as ContextReference<C>>::Target: ReferenceContext,
    <<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
        <C::Context as ContextReference<C>>::Target,
    >>::Target: Notify<T>
        + Notify<U>
        + Read<
            <<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                <C::Context as ContextReference<C>>::Target,
            >>::Target as Dispatch<
                <<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                    <C::Context as ContextReference<C>>::Target,
                >>::Target as Notify<T>>::Notification,
            >>::Handle,
        > + Write<
            <<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                <C::Context as ContextReference<C>>::Target,
            >>::Target as Dispatch<
                <<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                    <C::Context as ContextReference<C>>::Target,
                >>::Target as Notify<U>>::Notification,
            >>::Handle,
        >,
    <<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
        <C::Context as ContextReference<C>>::Target,
    >>::Target: Unpin,
    <<<<<C as ReferenceContext>::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
        <<C as ReferenceContext>::Context as ContextReference<C>>::Target,
    >>::Target as Join<
        <<<<<C as ReferenceContext>::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
            <<C as ReferenceContext>::Context as ContextReference<C>>::Target,
        >>::Target as Notify<T>>::Notification,
    >>::Future: Unpin,
    <<<<<C as ReferenceContext>::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
        <<C as ReferenceContext>::Context as ContextReference<C>>::Target,
    >>::Target as Notify<T>>::Unwrap: Unpin,
    <<<<<C as ReferenceContext>::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
        <<C as ReferenceContext>::Context as ContextReference<C>>::Target,
    >>::Target as Fork<
        <<<<<C as ReferenceContext>::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
            <<C as ReferenceContext>::Context as ContextReference<C>>::Target,
        >>::Target as Notify<U>>::Notification,
    >>::Target: Unpin,
    <<<<<C as ReferenceContext>::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
        <<C as ReferenceContext>::Context as ContextReference<C>>::Target,
    >>::Target as Fork<
        <<<<<C as ReferenceContext>::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
            <<C as ReferenceContext>::Context as ContextReference<C>>::Target,
        >>::Target as Notify<U>>::Notification,
    >>::Finalize: Unpin,
    <<<<<C as ReferenceContext>::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
        <<C as ReferenceContext>::Context as ContextReference<C>>::Target,
    >>::Target as Notify<U>>::Wrap: Unpin,
    <<<<<C as ReferenceContext>::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
        <<C as ReferenceContext>::Context as ContextReference<C>>::Target,
    >>::Target as Fork<
        <<<<<C as ReferenceContext>::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
            <<C as ReferenceContext>::Context as ContextReference<C>>::Target,
        >>::Target as Notify<U>>::Notification,
    >>::Future: Unpin,
{
}

impl<C: ?Sized + Write<<C as Contextualize>::Handle> + ReferenceContext, T, U, F, M> Future<C>
    for FnMutUnravel<C, T, U, F, M>
where
    for<'a> M: Fn(&'a mut F, T) -> U,
    <C::Context as ContextReference<C>>::Target: ReferenceContext
        + Read<Option<<<C::Context as ContextReference<C>>::Target as Contextualize>::Handle>>,
    <<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
        <C::Context as ContextReference<C>>::Target,
    >>::Target: Notify<T>
        + Notify<U>
        + Read<
            <<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                <C::Context as ContextReference<C>>::Target,
            >>::Target as Dispatch<
                <<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                    <C::Context as ContextReference<C>>::Target,
                >>::Target as Notify<T>>::Notification,
            >>::Handle,
        > + Write<
            <<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                <C::Context as ContextReference<C>>::Target,
            >>::Target as Dispatch<
                <<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                    <C::Context as ContextReference<C>>::Target,
                >>::Target as Notify<U>>::Notification,
            >>::Handle,
        >,
    <<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
        <C::Context as ContextReference<C>>::Target,
    >>::Target: Unpin,
    <<<<<C as ReferenceContext>::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
        <<C as ReferenceContext>::Context as ContextReference<C>>::Target,
    >>::Target as Join<
        <<<<<C as ReferenceContext>::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
            <<C as ReferenceContext>::Context as ContextReference<C>>::Target,
        >>::Target as Notify<T>>::Notification,
    >>::Future: Unpin,
    <<<<<C as ReferenceContext>::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
        <<C as ReferenceContext>::Context as ContextReference<C>>::Target,
    >>::Target as Notify<T>>::Unwrap: Unpin,
    <<<<<C as ReferenceContext>::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
        <<C as ReferenceContext>::Context as ContextReference<C>>::Target,
    >>::Target as Fork<
        <<<<<C as ReferenceContext>::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
            <<C as ReferenceContext>::Context as ContextReference<C>>::Target,
        >>::Target as Notify<U>>::Notification,
    >>::Target: Unpin,
    <<<<<C as ReferenceContext>::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
        <<C as ReferenceContext>::Context as ContextReference<C>>::Target,
    >>::Target as Fork<
        <<<<<C as ReferenceContext>::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
            <<C as ReferenceContext>::Context as ContextReference<C>>::Target,
        >>::Target as Notify<U>>::Notification,
    >>::Finalize: Unpin,
    <<<<<C as ReferenceContext>::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
        <<C as ReferenceContext>::Context as ContextReference<C>>::Target,
    >>::Target as Notify<U>>::Wrap: Unpin,
    <<<<<C as ReferenceContext>::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
        <<C as ReferenceContext>::Context as ContextReference<C>>::Target,
    >>::Target as Fork<
        <<<<<C as ReferenceContext>::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
            <<C as ReferenceContext>::Context as ContextReference<C>>::Target,
        >>::Target as Notify<U>>::Notification,
    >>::Future: Unpin,
    C::ForkOutput: Unpin,
    C: Unpin,
{
    type Ok = ErasedFnMutUnravel<C, T, U, F, M>;
    type Error = FnMutUnravelError<
        <<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
            <C::Context as ContextReference<C>>::Target,
        >>::Target as Read<
            <<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                <C::Context as ContextReference<C>>::Target,
            >>::Target as Dispatch<
                <<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                    <C::Context as ContextReference<C>>::Target,
                >>::Target as Notify<T>>::Notification,
            >>::Handle,
        >>::Error,
        <<<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
            <C::Context as ContextReference<C>>::Target,
        >>::Target as Notify<T>>::Unwrap as Future<
            <<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                <C::Context as ContextReference<C>>::Target,
            >>::Target,
        >>::Error,
        <<<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
            <C::Context as ContextReference<C>>::Target,
        >>::Target as Join<
            <<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                <C::Context as ContextReference<C>>::Target,
            >>::Target as Notify<T>>::Notification,
        >>::Future as Future<
            <<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                <C::Context as ContextReference<C>>::Target,
            >>::Target,
        >>::Error,
        <C::ForkOutput as Future<C>>::Error,
        C::Error,
        <<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
            <C::Context as ContextReference<C>>::Target,
        >>::Target as Write<
            <<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                <C::Context as ContextReference<C>>::Target,
            >>::Target as Dispatch<
                <<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                    <C::Context as ContextReference<C>>::Target,
                >>::Target as Notify<U>>::Notification,
            >>::Handle,
        >>::Error,
        <<<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
            <C::Context as ContextReference<C>>::Target,
        >>::Target as Notify<U>>::Wrap as Future<
            <<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                <C::Context as ContextReference<C>>::Target,
            >>::Target,
        >>::Error,
        <<<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
            <C::Context as ContextReference<C>>::Target,
        >>::Target as Fork<
            <<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                <C::Context as ContextReference<C>>::Target,
            >>::Target as Notify<U>>::Notification,
        >>::Future as Future<
            <<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                <C::Context as ContextReference<C>>::Target,
            >>::Target,
        >>::Error,
        <<<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
            <C::Context as ContextReference<C>>::Target,
        >>::Target as Fork<
            <<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                <C::Context as ContextReference<C>>::Target,
            >>::Target as Notify<U>>::Notification,
        >>::Target as Future<
            <<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                <C::Context as ContextReference<C>>::Target,
            >>::Target,
        >>::Error,
        <<<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
            <C::Context as ContextReference<C>>::Target,
        >>::Target as Fork<
            <<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                <C::Context as ContextReference<C>>::Target,
            >>::Target as Notify<U>>::Notification,
        >>::Finalize as Future<
            <<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                <C::Context as ContextReference<C>>::Target,
            >>::Target,
        >>::Error,
        <<C::Context as ContextReference<C>>::Target as Read<
            Option<<<C::Context as ContextReference<C>>::Target as Contextualize>::Handle>,
        >>::Error,
        <<<C::Context as ContextReference<C>>::Target as ReferenceContext>::JoinOutput as Future<
            <C::Context as ContextReference<C>>::Target,
        >>::Error,
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
                    FnMutUnravelState::None(_) => {
                        this.context = FnMutUnravelState::Context(ctx.borrow_mut().fork_ref())
                    }
                    FnMutUnravelState::Context(future) => {
                        let (context, handle) = ready!(Pin::new(future).poll(cx, ctx.borrow_mut()))
                            .map_err(FnMutUnravelError::Contextualize)?;
                        this.context = FnMutUnravelState::Write(context, handle);
                    }
                    FnMutUnravelState::Write(_, _) => {
                        let mut ctx = Pin::new(ctx.borrow_mut());
                        ready!(ctx.as_mut().poll_ready(cx)).map_err(FnMutUnravelError::Write)?;
                        let data = replace(&mut this.context, FnMutUnravelState::None(PhantomData));
                        if let FnMutUnravelState::Write(context, handle) = data {
                            ctx.write(handle).map_err(FnMutUnravelError::Write)?;
                            this.context = FnMutUnravelState::Flush(context);
                        } else {
                            panic!("invalid state")
                        }
                    }
                    FnMutUnravelState::Flush(_) => {
                        let data = replace(&mut this.context, FnMutUnravelState::None(PhantomData));
                        if let FnMutUnravelState::Flush(context) = data {
                            return Poll::Ready(Ok(ErasedFnMutUnravel {
                                pending: vec![],
                                conv: this.call.take().unwrap(),
                                call: this.conv.take().unwrap(),
                                context,
                                state: ErasedFnMutUnravelState::Read(PhantomData),
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

pub enum ErasedFnMutCoalesceState<
    T,
    U,
    C: CloneContext + Write<Option<<C as Contextualize>::Handle>>,
> where
    C::Context: Notify<T>
        + Notify<U>
        + Write<<C::Context as Dispatch<<C::Context as Notify<T>>::Notification>>::Handle>
        + Read<<C::Context as Dispatch<<C::Context as Notify<U>>::Notification>>::Handle>
        + Finalize<<C::Context as Fork<<C::Context as Notify<T>>::Notification>>::Finalize>,
    <C::Context as Fork<<C::Context as Notify<T>>::Notification>>::Finalize: Future<
        <C::Context as Finalize<
            <C::Context as Fork<<C::Context as Notify<T>>::Notification>>::Finalize,
        >>::Target,
    >,
{
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
    Join(<C::Context as Join<<C::Context as Notify<U>>::Notification>>::Future),
    Unwrap(<C::Context as Notify<U>>::Unwrap),
    Done,
}

pub struct ErasedFnMutCoalesce<
    T,
    U,
    C: Clone + CloneContext + Write<Option<<C as Contextualize>::Handle>>,
> where
    C::Context: Notify<T>
        + Notify<U>
        + Write<<C::Context as Dispatch<<C::Context as Notify<T>>::Notification>>::Handle>
        + Read<<C::Context as Dispatch<<C::Context as Notify<U>>::Notification>>::Handle>
        + Finalize<<C::Context as Fork<<C::Context as Notify<T>>::Notification>>::Finalize>,
    <C::Context as Fork<<C::Context as Notify<T>>::Notification>>::Finalize: Future<
        <C::Context as Finalize<
            <C::Context as Fork<<C::Context as Notify<T>>::Notification>>::Finalize,
        >>::Target,
    >,
{
    state: ErasedFnMutCoalesceState<T, U, C>,
    context: C,
    data: Option<T>,
    loc_context: Option<C::Context>,
}

impl<T, U, C: Clone + CloneContext + Write<Option<<C as Contextualize>::Handle>>> Unpin
    for ErasedFnMutCoalesce<T, U, C>
where
    C::Context: Notify<T>
        + Notify<U>
        + Write<<C::Context as Dispatch<<C::Context as Notify<T>>::Notification>>::Handle>
        + Read<<C::Context as Dispatch<<C::Context as Notify<U>>::Notification>>::Handle>
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
    I: Error + 'static,
    U: Error + 'static,
    V: Error + 'static,
    W: Error + 'static,
    C: Error + 'static,
    K: Error + 'static,
    L: Error + 'static,
)]
pub enum ErasedFnMutCoalesceError<A, C, B, T, I, U, V, W, K, L> {
    #[error("failed to write argument handle for erased FnMut: {0}")]
    Write(#[source] A),
    #[error("failed to fork argument for erased FnMut: {0}")]
    Fork(#[source] C),
    #[error("failed to wrap argument notification for erased FnMut: {0}")]
    Wrap(#[source] B),
    #[error("failed to read handle for erased FnMut return: {0}")]
    Read(#[source] T),
    #[error("failed to create notification wrapper for erased FnMut return: {0}")]
    Unwrap(#[source] I),
    #[error("failed to join erased FnMut return: {0}")]
    Join(#[source] U),
    #[error("failed to target erased FnMut argument: {0}")]
    Target(#[source] V),
    #[error("failed to finalize erased FnMut argument: {0}")]
    Finalize(#[source] W),
    #[error("failed to contextualize erased FnMut call: {0}")]
    Contextualize(#[source] K),
    #[error("failed to write context handle for erased FnMut call: {0}")]
    WriteHandle(#[source] L),
}

type CoalesceError<T, U, C> = ErasedFnMutCoalesceError<
    <<C as CloneContext>::Context as Write<
        <<C as CloneContext>::Context as Dispatch<
            <<C as CloneContext>::Context as Notify<T>>::Notification,
        >>::Handle,
    >>::Error,
    <<<C as CloneContext>::Context as Fork<
        <<C as CloneContext>::Context as Notify<T>>::Notification,
    >>::Future as Future<<C as CloneContext>::Context>>::Error,
    <<<C as CloneContext>::Context as Notify<T>>::Wrap as Future<<C as CloneContext>::Context>>::Error,
    <<C as CloneContext>::Context as Read<
        <<C as CloneContext>::Context as Dispatch<
            <<C as CloneContext>::Context as Notify<U>>::Notification,
        >>::Handle,
    >>::Error,
    <<<C as CloneContext>::Context as Notify<U>>::Unwrap as Future<<C as CloneContext>::Context>>::Error,
    <<<C as CloneContext>::Context as Join<
        <<C as CloneContext>::Context as Notify<U>>::Notification,
    >>::Future as Future<<C as CloneContext>::Context>>::Error,
    <<<C as CloneContext>::Context as Fork<
        <<C as CloneContext>::Context as Notify<T>>::Notification,
    >>::Target as Future<<C as CloneContext>::Context>>::Error,
    <<<C as CloneContext>::Context as Finalize<
        <<C as CloneContext>::Context as Fork<
            <<C as CloneContext>::Context as Notify<T>>::Notification,
        >>::Finalize,
    >>::Output as Future<<C as CloneContext>::Context>>::Error,
    <<C as CloneContext>::ForkOutput as Future<C>>::Error,
    <C as Write<Option<<C as Contextualize>::Handle>>>::Error,
>;

impl<T, U, C: Clone + CloneContext + Write<Option<<C as Contextualize>::Handle>>> future::Future
    for ErasedFnMutCoalesce<T, U, C>
where
    C::Context: Notify<T>
        + Notify<U>
        + Write<<C::Context as Dispatch<<C::Context as Notify<T>>::Notification>>::Handle>
        + Read<<C::Context as Dispatch<<C::Context as Notify<U>>::Notification>>::Handle>
        + Finalize<<C::Context as Fork<<C::Context as Notify<T>>::Notification>>::Finalize>,
    <C::Context as Fork<<C::Context as Notify<T>>::Notification>>::Finalize: Future<
        <C::Context as Finalize<
            <C::Context as Fork<<C::Context as Notify<T>>::Notification>>::Finalize,
        >>::Target,
    >,
    <<C as CloneContext>::Context as Write<
        <<C as CloneContext>::Context as Dispatch<
            <<C as CloneContext>::Context as Notify<T>>::Notification,
        >>::Handle,
    >>::Error: Send + Error + 'static,
    <<<C as CloneContext>::Context as Fork<
        <<C as CloneContext>::Context as Notify<T>>::Notification,
    >>::Future as Future<<C as CloneContext>::Context>>::Error: Send + Error + 'static,
    <<<C as CloneContext>::Context as Notify<T>>::Wrap as Future<<C as CloneContext>::Context>>::Error:
        Send + Error + 'static,
    <<C as CloneContext>::Context as Read<
        <<C as CloneContext>::Context as Dispatch<
            <<C as CloneContext>::Context as Notify<U>>::Notification,
        >>::Handle,
    >>::Error: Send + Error + 'static,
    <<<C as CloneContext>::Context as Notify<U>>::Unwrap as Future<<C as CloneContext>::Context>>::Error:
        Send + Error + 'static,
    <<<C as CloneContext>::Context as Join<
        <<C as CloneContext>::Context as Notify<U>>::Notification,
    >>::Future as Future<<C as CloneContext>::Context>>::Error: Send + Error + 'static,
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
    <<C as CloneContext>::Context as Notify<U>>::Unwrap: Unpin,
    <<C as CloneContext>::Context as Fork<
        <<C as CloneContext>::Context as Notify<T>>::Notification,
    >>::Future: Unpin,
    <<C as CloneContext>::Context as Join<
        <<C as CloneContext>::Context as Notify<U>>::Notification,
    >>::Future: Unpin,
    <<C as CloneContext>::Context as Fork<
        <<C as CloneContext>::Context as Notify<T>>::Notification,
    >>::Target: Unpin,
    C::ForkOutput: Unpin,
    C: Unpin,
    <C::ForkOutput as Future<C>>::Error: Send + Error + 'static,
    <C as Write<Option<<C as Contextualize>::Handle>>>::Error: Send + Error + 'static,
{
    type Output = Result<U, ProtocolError>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        let this = &mut *self;

        loop {
            match &mut this.state {
                ErasedFnMutCoalesceState::Contextualize(future) => {
                    let (context, handle) = ready!(Pin::new(future).poll(cx, &mut this.context))
                        .map_err(|e| {
                            ProtocolError::new(CoalesceError::<T, U, C>::Contextualize(e))
                        })?;
                    this.loc_context = Some(context);
                    this.state = ErasedFnMutCoalesceState::WriteHandle(handle);
                }
                ErasedFnMutCoalesceState::WriteHandle(_) => {
                    let state = &mut this.state;
                    let mut ctx = Pin::new(&mut this.context);
                    ready!(ctx.as_mut().poll_ready(cx)).map_err(|e| {
                        ProtocolError::new(CoalesceError::<T, U, C>::WriteHandle(e))
                    })?;
                    let data = replace(state, ErasedFnMutCoalesceState::Done);
                    if let ErasedFnMutCoalesceState::WriteHandle(handle) = data {
                        ctx.write(Some(handle)).map_err(|e| {
                            ProtocolError::new(CoalesceError::<T, U, C>::WriteHandle(e))
                        })?;
                        *state = ErasedFnMutCoalesceState::FlushHandle;
                    } else {
                        panic!("invalid state in ErasedFnMutCoalesce WriteHandle")
                    }
                }
                ErasedFnMutCoalesceState::FlushHandle => {
                    ready!(Pin::new(&mut this.context).poll_flush(cx)).map_err(|e| {
                        ProtocolError::new(CoalesceError::<T, U, C>::WriteHandle(e))
                    })?;
                    this.state = ErasedFnMutCoalesceState::Wrap(
                        this.loc_context
                            .as_mut()
                            .unwrap()
                            .wrap(this.data.take().unwrap()),
                    );
                }
                ErasedFnMutCoalesceState::Wrap(future) => {
                    let ctx = this.loc_context.as_mut().unwrap();
                    let wrapped = ready!(Pin::new(future).poll(cx, &mut *ctx))
                        .map_err(|e| ProtocolError::new(CoalesceError::<T, U, C>::Wrap(e)))?;
                    this.state = ErasedFnMutCoalesceState::Fork(ctx.fork(wrapped));
                }
                ErasedFnMutCoalesceState::Fork(future) => {
                    let ctx = this.loc_context.as_mut().unwrap();
                    let (target, handle) = ready!(Pin::new(future).poll(cx, &mut *ctx))
                        .map_err(|e| ProtocolError::new(CoalesceError::<T, U, C>::Fork(e)))?;
                    this.state = ErasedFnMutCoalesceState::Write(target, handle);
                }
                ErasedFnMutCoalesceState::Write(_, _) => {
                    let ctx = this.loc_context.as_mut().unwrap();
                    let mut ctx = Pin::new(&mut *ctx);
                    ready!(ctx.as_mut().poll_ready(cx))
                        .map_err(|e| ProtocolError::new(CoalesceError::<T, U, C>::Write(e)))?;
                    let data = replace(&mut this.state, ErasedFnMutCoalesceState::Done);
                    if let ErasedFnMutCoalesceState::Write(target, data) = data {
                        ctx.write(data)
                            .map_err(|e| ProtocolError::new(CoalesceError::<T, U, C>::Write(e)))?;
                        this.state = ErasedFnMutCoalesceState::Flush(target);
                    } else {
                        panic!("invalid state in ErasedFnMutCoalesce Write")
                    }
                }
                ErasedFnMutCoalesceState::Flush(_) => {
                    let ctx = this.loc_context.as_mut().unwrap();
                    ready!(Pin::new(&mut *ctx).poll_ready(cx))
                        .map_err(|e| ProtocolError::new(CoalesceError::<T, U, C>::Write(e)))?;
                    let data = replace(&mut this.state, ErasedFnMutCoalesceState::Done);
                    if let ErasedFnMutCoalesceState::Flush(target) = data {
                        this.state = ErasedFnMutCoalesceState::Target(target);
                    } else {
                        panic!("invalid state in ErasedFnMutUnravel Write")
                    }
                }
                ErasedFnMutCoalesceState::Target(target) => {
                    let ctx = this.loc_context.as_mut().unwrap();
                    let finalize = ready!(Pin::new(target).poll(cx, &mut *ctx))
                        .map_err(|e| ProtocolError::new(CoalesceError::<T, U, C>::Target(e)))?;
                    this.state = ErasedFnMutCoalesceState::Finalize(ctx.finalize(finalize));
                }
                ErasedFnMutCoalesceState::Finalize(future) => {
                    let ctx = this.loc_context.as_mut().unwrap();
                    ready!(Pin::new(future).poll(cx, &mut *ctx))
                        .map_err(|e| ProtocolError::new(CoalesceError::<T, U, C>::Finalize(e)))?;
                    this.state = ErasedFnMutCoalesceState::Read;
                }
                ErasedFnMutCoalesceState::Read => {
                    let ctx = this.loc_context.as_mut().unwrap();
                    let handle: <C::Context as Dispatch<
                        <C::Context as Notify<U>>::Notification,
                    >>::Handle = ready!(Pin::new(&mut *ctx).read(cx))
                        .map_err(|e| ProtocolError::new(CoalesceError::<T, U, C>::Read(e)))?;
                    let join: <C::Context as Join<<C::Context as Notify<U>>::Notification>>::Future =
                        Join::<<C::Context as Notify<U>>::Notification>::join(ctx, handle);
                    this.state = ErasedFnMutCoalesceState::Join(join);
                }
                ErasedFnMutCoalesceState::Join(future) => {
                    let ctx = this.loc_context.as_mut().unwrap();
                    let wrapped = ready!(Pin::new(future).poll(cx, &mut *ctx))
                        .map_err(|e| ProtocolError::new(CoalesceError::<T, U, C>::Join(e)))?;
                    let unwrap: <C::Context as Notify<U>>::Unwrap =
                        Notify::<U>::unwrap(ctx, wrapped);
                    this.state = ErasedFnMutCoalesceState::Unwrap(unwrap);
                }
                ErasedFnMutCoalesceState::Unwrap(future) => {
                    let ctx = this.loc_context.as_mut().unwrap();
                    let data = ready!(Pin::new(future).poll(cx, &mut *ctx))
                        .map_err(|e| ProtocolError::new(CoalesceError::<T, U, C>::Unwrap(e)))?;
                    this.state = ErasedFnMutCoalesceState::Done;
                    return Poll::Ready(Ok(data));
                }
                ErasedFnMutCoalesceState::Done => {
                    panic!("erased FnMut coalesce polled after completion")
                }
            }
        }
    }
}

pub enum FnMutCoalesceState<T> {
    Read,
    Contextualize(T),
    Done,
}

pub enum ErasedFnMutComplete {
    Write,
    Flush,
    Done,
}

impl<C: Unpin + CloneContext + Write<Option<<C as Contextualize>::Handle>>> Future<C> for ErasedFnMutComplete {
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
                ErasedFnMutComplete::Write => {
                    let mut ctx = Pin::new(ctx.borrow_mut());
                    ready!(ctx.as_mut().poll_ready(cx))?;
                    ctx.write(None)?;
                    *this = ErasedFnMutComplete::Flush;
                }
                ErasedFnMutComplete::Flush => {
                    ready!(Pin::new(ctx.borrow_mut()).poll_flush(cx))?;
                    *this = ErasedFnMutComplete::Done;
                    return Poll::Ready(Ok(()));
                }
                ErasedFnMutComplete::Done => {
                    panic!("erased FnMut terminator polled after completion")
                }
            }
        }
    }
}

pub struct ErasedFnMut<T, U, C: Clone + FinalizeImmediate<ErasedFnMutComplete>>
where
    C::Target: Unpin + CloneContext + Write<Option<<C::Target as Contextualize>::Handle>>,
{
    context: C,
    data: PhantomData<(T, U, C)>,
}

impl<T, U, C: Clone + FinalizeImmediate<ErasedFnMutComplete>> Drop for ErasedFnMut<T, U, C>
where
    C::Target: Unpin + CloneContext + Write<Option<<C::Target as Contextualize>::Handle>>,
{
    fn drop(&mut self) {
        let _ = self.context.finalize(ErasedFnMutComplete::Write);
    }
}

impl<
    T,
    U: Flatten<ProtocolError, ErasedFnMutCoalesce<T, U, C>>,
    C: Clone
        + CloneContext
        + FinalizeImmediate<ErasedFnMutComplete>
        + Write<Option<<C as Contextualize>::Handle>>,
> ErasedFnMut<T, U, C>
where
    C::Target: Unpin + CloneContext + Write<Option<<C::Target as Contextualize>::Handle>>,
    C::Context: Notify<T>
        + Notify<U>
        + Write<<C::Context as Dispatch<<C::Context as Notify<T>>::Notification>>::Handle>
        + Read<<C::Context as Dispatch<<C::Context as Notify<U>>::Notification>>::Handle>
        + Finalize<<C::Context as Fork<<C::Context as Notify<T>>::Notification>>::Finalize>,
    <C::Context as Fork<<C::Context as Notify<T>>::Notification>>::Finalize: Future<
        <C::Context as Finalize<
            <C::Context as Fork<<C::Context as Notify<T>>::Notification>>::Finalize,
        >>::Target,
    >,
    <<C as CloneContext>::Context as Write<
        <<C as CloneContext>::Context as Dispatch<
            <<C as CloneContext>::Context as Notify<T>>::Notification,
        >>::Handle,
    >>::Error: Send + Error + 'static,
    <<<C as CloneContext>::Context as Fork<
        <<C as CloneContext>::Context as Notify<T>>::Notification,
    >>::Future as Future<<C as CloneContext>::Context>>::Error: Send + Error + 'static,
    <<<C as CloneContext>::Context as Notify<T>>::Wrap as Future<<C as CloneContext>::Context>>::Error:
        Send + Error + 'static,
    <<C as CloneContext>::Context as Read<
        <<C as CloneContext>::Context as Dispatch<
            <<C as CloneContext>::Context as Notify<U>>::Notification,
        >>::Handle,
    >>::Error: Send + Error + 'static,
    <<<C as CloneContext>::Context as Notify<U>>::Unwrap as Future<<C as CloneContext>::Context>>::Error:
        Send + Error + 'static,
    <<<C as CloneContext>::Context as Join<
        <<C as CloneContext>::Context as Notify<U>>::Notification,
    >>::Future as Future<<C as CloneContext>::Context>>::Error: Send + Error + 'static,
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
    <<C as CloneContext>::Context as Notify<U>>::Unwrap: Unpin,
    <<C as CloneContext>::Context as Fork<
        <<C as CloneContext>::Context as Notify<T>>::Notification,
    >>::Future: Unpin,
    <<C as CloneContext>::Context as Join<
        <<C as CloneContext>::Context as Notify<U>>::Notification,
    >>::Future: Unpin,
    <<C as CloneContext>::Context as Fork<
        <<C as CloneContext>::Context as Notify<T>>::Notification,
    >>::Target: Unpin,
    C::ForkOutput: Unpin,
    C: Unpin,
    <C::ForkOutput as Future<C>>::Error: Send + Error + 'static,
    <C as Write<Option<<C as Contextualize>::Handle>>>::Error: Send + Error + 'static,
{
    fn call(&mut self, args: T) -> U {
        U::flatten(ErasedFnMutCoalesce {
            state: ErasedFnMutCoalesceState::Contextualize(self.context.fork_owned()),
            context: self.context.clone(),
            data: Some(args),
            loc_context: None,
        })
    }
}

#[derive(Debug, Error)]
#[bounds(
where
    T: Error + 'static,
    E: Error + 'static,
)]
pub enum FnMutCoalesceError<E, T> {
    #[error("failed to read handle for erased FnMut context: {0}")]
    Read(T),
    #[error("failed to contextualize erased FnMut content: {0}")]
    Contextualize(E),
}

pub struct FnMutCoalesce<
    'a,
    O,
    P: Fn(ErasedFnMut<T, U, C::Context>) -> O,
    T: Unpin,
    U,
    C: ?Sized + ShareContext,
> where
    <C::Context as FinalizeImmediate<ErasedFnMutComplete>>::Target: Unpin
        + CloneContext
        + Write<
            Option<
                <<C::Context as FinalizeImmediate<ErasedFnMutComplete>>::Target as Contextualize>::Handle,
            >,
        >,
    C::Context: Unpin
        + FinalizeImmediate<ErasedFnMutComplete>
        + Read<<C::Context as Dispatch<<C::Context as Notify<T>>::Notification>>::Handle>
        + Notify<T>,
    <C::Context as Fork<<C::Context as Notify<T>>::Notification>>::Future: Unpin,
    <C::Context as Notify<T>>::Wrap: Unpin,
{
    conv: P,
    lifetime: PhantomData<&'a (O, T, U)>,
    state: FnMutCoalesceState<C::JoinOutput>,
}

impl<
    'a,
    O,
    P: Fn(ErasedFnMut<T, U, C::Context>) -> O,
    T: Unpin,
    U,
    C: ?Sized + Read<<C as Contextualize>::Handle> + ShareContext,
> Future<C> for FnMutCoalesce<'a, O, P, T, U, C>
where
    C::JoinOutput: Unpin,
    C: Unpin,
    <C::Context as FinalizeImmediate<ErasedFnMutComplete>>::Target: Unpin
        + CloneContext
        + Write<
            Option<
                <<C::Context as FinalizeImmediate<ErasedFnMutComplete>>::Target as Contextualize>::Handle,
            >,
        >,
    C::Context: Unpin,
    P: Unpin,
    <C::Context as Notify<T>>::Wrap: Unpin,
    C::Context: FinalizeImmediate<ErasedFnMutComplete>
        + Unpin
        + Read<<C::Context as Dispatch<<C::Context as Notify<T>>::Notification>>::Handle>
        + Notify<T>,
    <C::Context as Fork<<C::Context as Notify<T>>::Notification>>::Future: Unpin,
{
    type Ok = O;
    type Error = FnMutCoalesceError<<C::JoinOutput as Future<C>>::Error, C::Error>;

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
                FnMutCoalesceState::Read => {
                    let ct = Pin::new(&mut *ctx);
                    let handle = ready!(ct.read(cx)).map_err(FnMutCoalesceError::Read)?;
                    this.state = FnMutCoalesceState::Contextualize(ctx.join_shared(handle));
                }
                FnMutCoalesceState::Contextualize(future) => {
                    let context = ready!(Pin::new(future).poll(cx, &mut *ctx))
                        .map_err(FnMutCoalesceError::Contextualize)?;
                    replace(&mut this.state, FnMutCoalesceState::Done);
                    return Poll::Ready(Ok((this.conv)(ErasedFnMut {
                        context,
                        data: PhantomData,
                    })));
                }
                FnMutCoalesceState::Done => panic!("FnMutCoalesce polled after completion"),
            }
        }
    }
}

macro_rules! tuple_impls {
(
    $(($($marker:ident)*) => (
        $($n:tt $name:ident)*
    ))+
) => {
    $(
        impl<'a, $($name: 'a,)* U: 'a, C: ?Sized + Write<<C as Contextualize>::Handle> + ReferenceContext> Unravel<C>
            for Box<dyn FnMut($($name,)*) -> U + 'a $(+ $marker)*>
        where
            <C::Context as ContextReference<C>>::Target: ReferenceContext
                + Read<Option<<<C::Context as ContextReference<C>>::Target as Contextualize>::Handle>>,
            <<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                <C::Context as ContextReference<C>>::Target,
            >>::Target: Notify<($($name,)*)>
                + Notify<U>
                + Read<
                    <<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                        <C::Context as ContextReference<C>>::Target,
                    >>::Target as Dispatch<
                        <<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                            <C::Context as ContextReference<C>>::Target,
                        >>::Target as Notify<($($name,)*)>>::Notification,
                    >>::Handle,
                > + Write<
                    <<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                        <C::Context as ContextReference<C>>::Target,
                    >>::Target as Dispatch<
                        <<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                            <C::Context as ContextReference<C>>::Target,
                        >>::Target as Notify<U>>::Notification,
                    >>::Handle,
                >,
            <<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                <C::Context as ContextReference<C>>::Target,
            >>::Target: Unpin,
            <<<<<C as ReferenceContext>::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                <<C as ReferenceContext>::Context as ContextReference<C>>::Target,
            >>::Target as Join<
                <<<<<C as ReferenceContext>::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                    <<C as ReferenceContext>::Context as ContextReference<C>>::Target,
                >>::Target as Notify<($($name,)*)>>::Notification,
            >>::Future: Unpin,
            <<<<<C as ReferenceContext>::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                <<C as ReferenceContext>::Context as ContextReference<C>>::Target,
            >>::Target as Notify<($($name,)*)>>::Unwrap: Unpin,
            <<<<<C as ReferenceContext>::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                <<C as ReferenceContext>::Context as ContextReference<C>>::Target,
            >>::Target as Fork<
                <<<<<C as ReferenceContext>::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                    <<C as ReferenceContext>::Context as ContextReference<C>>::Target,
                >>::Target as Notify<U>>::Notification,
            >>::Target: Unpin,
            <<<<<C as ReferenceContext>::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                <<C as ReferenceContext>::Context as ContextReference<C>>::Target,
            >>::Target as Fork<
                <<<<<C as ReferenceContext>::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                    <<C as ReferenceContext>::Context as ContextReference<C>>::Target,
                >>::Target as Notify<U>>::Notification,
            >>::Finalize: Unpin,
            <<<<<C as ReferenceContext>::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                <<C as ReferenceContext>::Context as ContextReference<C>>::Target,
            >>::Target as Notify<U>>::Wrap: Unpin,
            <<<<<C as ReferenceContext>::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                <<C as ReferenceContext>::Context as ContextReference<C>>::Target,
            >>::Target as Fork<
                <<<<<C as ReferenceContext>::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                    <<C as ReferenceContext>::Context as ContextReference<C>>::Target,
                >>::Target as Notify<U>>::Notification,
            >>::Future: Unpin,
            C::ForkOutput: Unpin,
            C: Unpin,
            <<C as ReferenceContext>::Context as ContextReference<C>>::Target: Unpin,
            <<<C as ReferenceContext>::Context as ContextReference<C>>::Target as ReferenceContext>::JoinOutput:
                Unpin,
        {
            type Finalize = ErasedFnMutUnravel<
                C,
                ($($name,)*),
                U,
                Box<dyn FnMut($($name,)*) -> U + 'a>,
                fn(&mut Box<dyn FnMut($($name,)*) -> U + 'a>, ($($name,)*)) -> U,
            >;
            type Target = FnMutUnravel<
                C,
                ($($name,)*),
                U,
                Box<dyn FnMut($($name,)*) -> U + 'a>,
                fn(&mut Box<dyn FnMut($($name,)*) -> U + 'a>, ($($name,)*)) -> U,
            >;

            #[allow(unused_variables)]
            fn unravel(self) -> Self::Target {
                FnMutUnravel {
                    call: Some(self),
                    conv: Some(|call, data| (call)($(data.$n,)*)),
                    context: FnMutUnravelState::None(PhantomData),
                }
            }
        }

        impl<
            'a,
            $($name: Unpin + 'a,)*
            U: Flatten<ProtocolError, ErasedFnMutCoalesce<($($name,)*), U, C::Context>> + 'a $(+ $marker)*,
            C: ?Sized + Read<<C as Contextualize>::Handle> + ShareContext,
        > Coalesce<C> for Box<dyn FnMut($($name,)*) -> U + 'a $(+ $marker)*>
        where
            <C::Context as FinalizeImmediate<ErasedFnMutComplete>>::Target: Unpin
                + CloneContext
                + Write<
                    Option<
                        <<C::Context as FinalizeImmediate<ErasedFnMutComplete>>::Target as Contextualize>::Handle,
                    >,
                >,
            ($($name,)*): 'a $(+ $marker)*,
            <C::Context as CloneContext>::Context: Notify<($($name,)*)>
                + Notify<U>
                + Write<
                    <<C::Context as CloneContext>::Context as Dispatch<
                        <<C::Context as CloneContext>::Context as Notify<($($name,)*)>>::Notification,
                    >>::Handle,
                > + Read<
                    <<C::Context as CloneContext>::Context as Dispatch<
                        <<C::Context as CloneContext>::Context as Notify<U>>::Notification,
                    >>::Handle,
                > + Finalize<
                    <<C::Context as CloneContext>::Context as Fork<
                        <<C::Context as CloneContext>::Context as Notify<($($name,)*)>>::Notification,
                    >>::Finalize,
                >,
            <<C::Context as CloneContext>::Context as Fork<
                <<C::Context as CloneContext>::Context as Notify<($($name,)*)>>::Notification,
            >>::Finalize: Future<
                <<C::Context as CloneContext>::Context as Finalize<
                    <<C::Context as CloneContext>::Context as Fork<
                        <<C::Context as CloneContext>::Context as Notify<($($name,)*)>>::Notification,
                    >>::Finalize,
                >>::Target,
            >,
            <<C::Context as CloneContext>::Context as Write<
                <<C::Context as CloneContext>::Context as Dispatch<
                    <<C::Context as CloneContext>::Context as Notify<($($name,)*)>>::Notification,
                >>::Handle,
            >>::Error: Send + Error + 'static,
            <<<C::Context as CloneContext>::Context as Fork<
                <<C::Context as CloneContext>::Context as Notify<($($name,)*)>>::Notification,
            >>::Future as Future<<C::Context as CloneContext>::Context>>::Error: Send + Error + 'static,
            <<<C::Context as CloneContext>::Context as Notify<($($name,)*)>>::Wrap as Future<
                <C::Context as CloneContext>::Context,
            >>::Error: Send + Error + 'static,
            <<C::Context as CloneContext>::Context as Read<
                <<C::Context as CloneContext>::Context as Dispatch<
                    <<C::Context as CloneContext>::Context as Notify<U>>::Notification,
                >>::Handle,
            >>::Error: Send + Error + 'static,
            <<<C::Context as CloneContext>::Context as Notify<U>>::Unwrap as Future<
                <C::Context as CloneContext>::Context,
            >>::Error: Send + Error + 'static,
            <<<C::Context as CloneContext>::Context as Join<
                <<C::Context as CloneContext>::Context as Notify<U>>::Notification,
            >>::Future as Future<<C::Context as CloneContext>::Context>>::Error: Send + Error + 'static,
            <<<C::Context as CloneContext>::Context as Fork<
                <<C::Context as CloneContext>::Context as Notify<($($name,)*)>>::Notification,
            >>::Target as Future<<C::Context as CloneContext>::Context>>::Error: Send + Error + 'static,
            <<<C::Context as CloneContext>::Context as Finalize<
                <<C::Context as CloneContext>::Context as Fork<
                    <<C::Context as CloneContext>::Context as Notify<($($name,)*)>>::Notification,
                >>::Finalize,
            >>::Output as Future<<C::Context as CloneContext>::Context>>::Error: Send + Error + 'static,
            <C::Context as CloneContext>::Context: Unpin,
            <<C::Context as CloneContext>::Context as Finalize<
                <<C::Context as CloneContext>::Context as Fork<
                    <<C::Context as CloneContext>::Context as Notify<($($name,)*)>>::Notification,
                >>::Finalize,
            >>::Output: Unpin,
            <<C::Context as CloneContext>::Context as Notify<($($name,)*)>>::Wrap: Unpin,
            <<C::Context as CloneContext>::Context as Notify<U>>::Unwrap: Unpin,
            <<C::Context as CloneContext>::Context as Fork<
                <<C::Context as CloneContext>::Context as Notify<($($name,)*)>>::Notification,
            >>::Future: Unpin,
            <<C::Context as CloneContext>::Context as Join<
                <<C::Context as CloneContext>::Context as Notify<U>>::Notification,
            >>::Future: Unpin,
            <<C::Context as CloneContext>::Context as Fork<
                <<C::Context as CloneContext>::Context as Notify<($($name,)*)>>::Notification,
            >>::Target: Unpin,
            <C::Context as CloneContext>::ForkOutput: Unpin,
            C: Unpin,
            <<C::Context as CloneContext>::ForkOutput as Future<C::Context>>::Error: Send + Error + 'static,
            <C::Context as Write<Option<<C::Context as Contextualize>::Handle>>>::Error:
                Send + Error + 'static,
            C::JoinOutput: Unpin,
            C: Unpin,
            C::Context: Unpin,
            <C::Context as Notify<($($name,)*)>>::Wrap: Unpin,
            C::Context: Unpin
                + Read<<C::Context as Dispatch<<C::Context as Notify<($($name,)*)>>::Notification>>::Handle>
                + Notify<($($name,)*)>
                + FinalizeImmediate<ErasedFnMutComplete>
                + CloneContext
                + Write<Option<<C::Context as Contextualize>::Handle>>
                $(+ $marker)*
                + 'a,
            <C::Context as Fork<<C::Context as Notify<($($name,)*)>>::Notification>>::Future: Unpin,
        {
            type Future = FnMutCoalesce<
                'a,
                Self,
                fn(ErasedFnMut<($($name,)*), U, C::Context>) -> Self,
                ($($name,)*),
                U,
                C,
            >;

            #[allow(non_snake_case)]
            fn coalesce() -> Self::Future {
                FnMutCoalesce {
                    lifetime: PhantomData,
                    conv: |mut erased| Box::new(move |$($name,)*| erased.call(($($name,)*))),
                    state: FnMutCoalesceState::Read,
                }
            }
        }
    )+
}}

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
        )+
    };
}

pub trait Null {}

impl<T> Null for T {}

marker_variants! {
    ,
    Unpin,
    Sync,
    Send, Sync Send,
    Sync Unpin, Send Unpin, Sync Send Unpin
}
