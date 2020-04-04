use crate::{
    allocated::{Flatten, ProtocolError},
    CloneContext, Coalesce, ContextReference, Contextualize, Dispatch, Finalize, FinalizeImmediate,
    Fork, Future, Join, Notify, Read, ReferenceContext, ShareContext, Unravel, Write,
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
use futures::ready;
use thiserror::Error;

mod fn_once;

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
pub enum FnUnravelError<A, B, C, Z, K, T, I, U, V, W, F, P> {
    #[error("failed to read argument handle for erased Fn: {0}")]
    Read(#[source] A),
    #[error("failed to join argument for erased Fn: {0}")]
    Join(#[source] C),
    #[error("failed to unwrap argument notification for erased Fn: {0}")]
    Unwrap(#[source] B),
    #[error("failed to contextualize erased Fn: {0}")]
    Contextualize(#[source] Z),
    #[error("failed to contextualize erased Fn item: {0}")]
    SubContextualize(#[source] P),
    #[error("failed to write context handle for erased Fn: {0}")]
    Write(#[source] K),
    #[error("failed to write handle for erased Fn: {0}")]
    Transport(#[source] T),
    #[error("failed to read handle for erased Fn: {0}")]
    ReadHandle(#[source] F),
    #[error("failed to create notification wrapper for erased Fn return: {0}")]
    Notify(#[source] I),
    #[error("failed to fork erased Fn return: {0}")]
    Dispatch(#[source] U),
    #[error("failed to target erased Fn return: {0}")]
    Target(#[source] V),
    #[error("failed to finalize erased Fn return: {0}")]
    Finalize(#[source] W),
}

pub enum ErasedFnUnravelInstanceState<C: ?Sized + ReferenceContext, T, U>
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

pub struct ErasedFnUnravelInstance<C: ?Sized + ReferenceContext, T, U, F, M>
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
    state: ErasedFnUnravelInstanceState<C, T, U>,
    marker: PhantomData<(F, M)>,
    context: <<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context,
}

impl<C: ?Sized + ReferenceContext, T, U, F, M> ErasedFnUnravelInstance<C, T, U, F, M>
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
    pub fn new(
        context: <<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context,
    ) -> Self {
        ErasedFnUnravelInstance {
            state: ErasedFnUnravelInstanceState::Read,
            marker: PhantomData,
            context,
        }
    }
}

impl<C: ?Sized + ReferenceContext, T, U, F, M> Unpin for ErasedFnUnravelInstance<C, T, U, F, M>
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
    ErasedFnUnravelInstance<C, T, U, F, M>
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
            FnUnravelError<
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
                ErasedFnUnravelInstanceState::Read => {
                    let mut ct = Pin::new(&mut *ctx);
                    let handle = ready!(ct.as_mut().read(cx)).map_err(FnUnravelError::Read)?;
                    this.state = ErasedFnUnravelInstanceState::Join(Join::<
                        <<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                            <C::Context as ContextReference<C>>::Target,
                        >>::Target as Notify<T>>::Notification,
                    >::join(
                        &mut *ctx, handle
                    ));
                }
                ErasedFnUnravelInstanceState::Join(future) => {
                    let notification = ready!(Pin::new(future).poll(cx, &mut *ctx))
                        .map_err(FnUnravelError::Join)?;
                    this.state = ErasedFnUnravelInstanceState::Unwrap(
                        <<<<C::Context as ContextReference<C>>::Target as ReferenceContext>::Context as ContextReference<
                            <C::Context as ContextReference<C>>::Target,
                        >>::Target as Notify<T>>::unwrap(
                            &mut *ctx, notification
                        ),
                    );
                }
                ErasedFnUnravelInstanceState::Unwrap(future) => {
                    let item = ready!(Pin::new(future).poll(cx, &mut *ctx))
                        .map_err(FnUnravelError::Unwrap)?;
                    let item = (conv)(call, item);
                    this.state = ErasedFnUnravelInstanceState::Wrap(ctx.wrap(item));
                }
                ErasedFnUnravelInstanceState::Wrap(future) => {
                    let item = ready!(Pin::new(future).poll(cx, &mut *ctx))
                        .map_err(FnUnravelError::Notify)?;
                    this.state = ErasedFnUnravelInstanceState::Fork(ctx.fork(item));
                }
                ErasedFnUnravelInstanceState::Fork(future) => {
                    let (target, handle) = ready!(Pin::new(future).poll(cx, &mut *ctx))
                        .map_err(FnUnravelError::Dispatch)?;
                    this.state = ErasedFnUnravelInstanceState::Write(target, handle);
                }
                ErasedFnUnravelInstanceState::Write(_, _) => {
                    let mut ctx = Pin::new(&mut *ctx);
                    ready!(ctx.as_mut().poll_ready(cx)).map_err(FnUnravelError::Transport)?;
                    let data = replace(&mut this.state, ErasedFnUnravelInstanceState::Done);
                    if let ErasedFnUnravelInstanceState::Write(target, data) = data {
                        ctx.write(data).map_err(FnUnravelError::Transport)?;
                        this.state = ErasedFnUnravelInstanceState::Flush(target);
                    } else {
                        panic!("invalid state in ErasedFnUnravel Write")
                    }
                }
                ErasedFnUnravelInstanceState::Flush(_) => {
                    ready!(Pin::new(&mut *ctx).poll_flush(cx))
                        .map_err(FnUnravelError::Transport)?;
                    let data = replace(&mut this.state, ErasedFnUnravelInstanceState::Done);
                    if let ErasedFnUnravelInstanceState::Flush(target) = data {
                        this.state = ErasedFnUnravelInstanceState::Target(target);
                    } else {
                        panic!("invalid state in ErasedFnUnravel Write")
                    }
                }
                ErasedFnUnravelInstanceState::Target(target) => {
                    let finalize = ready!(Pin::new(target).poll(cx, &mut *ctx))
                        .map_err(FnUnravelError::Target)?;
                    this.state = ErasedFnUnravelInstanceState::Finalize(finalize);
                }
                ErasedFnUnravelInstanceState::Finalize(finalize) => {
                    ready!(Pin::new(finalize).poll(cx, &mut *ctx))
                        .map_err(FnUnravelError::Finalize)?;
                    this.state = ErasedFnUnravelInstanceState::Done;
                    return Poll::Ready(Ok(()));
                }
                ErasedFnUnravelInstanceState::Done => {
                    panic!("ErasedFnUnravel polled after completion")
                }
            }
        }
    }
}

enum FnUnravelState<C: ?Sized + Write<<C as Contextualize>::Handle> + ReferenceContext, T, U>
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

pub struct FnUnravel<C: ?Sized + Write<<C as Contextualize>::Handle> + ReferenceContext, T, U, F, M>
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
    call: Option<F>,
    context: FnUnravelState<C, T, U>,
    conv: Option<M>,
}

enum ErasedFnUnravelState<C: ?Sized + Write<<C as Contextualize>::Handle> + ReferenceContext, T, U>
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
    Read(PhantomData<(T, U)>),
    Contextualize(<<C::Context as ContextReference<C>>::Target as ReferenceContext>::JoinOutput),
    Done,
}

pub struct ErasedFnUnravel<
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
    pending: Vec<ErasedFnUnravelInstance<C, T, U, F, M>>,
    state: ErasedFnUnravelState<C, T, U>,
    context: C::Context,
    conv: F,
}

impl<C: ?Sized + Write<<C as Contextualize>::Handle> + ReferenceContext, T, U, F, M> Unpin
    for ErasedFnUnravel<C, T, U, F, M>
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
    for ErasedFnUnravel<C, T, U, F, M>
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
    type Error = FnUnravelError<
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
                ErasedFnUnravelState::Read(_) => {
                    let handle = ready!(Pin::new(&mut *context).read(cx))
                        .map_err(FnUnravelError::ReadHandle)?;
                    this.state = if let Some(handle) = handle {
                        ErasedFnUnravelState::Contextualize(context.join_ref(handle))
                    } else {
                        ErasedFnUnravelState::Done
                    };
                }
                ErasedFnUnravelState::Contextualize(future) => {
                    let ct = ready!(Pin::new(future).poll(cx, &mut *context))
                        .map_err(FnUnravelError::SubContextualize)?;
                    let mut fut = ErasedFnUnravelInstance::new(ct);
                    if let Poll::Pending = Pin::new(&mut fut).poll(
                        cx,
                        context.borrow_mut(),
                        &this.call,
                        &mut this.conv,
                    )? {
                        this.pending.push(fut);
                    }
                    this.state = ErasedFnUnravelState::Read(PhantomData);
                }
                ErasedFnUnravelState::Done => {
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
    for FnUnravel<C, T, U, F, M>
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
    for FnUnravel<C, T, U, F, M>
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
    type Ok = ErasedFnUnravel<C, T, U, F, M>;
    type Error = FnUnravelError<
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
                    FnUnravelState::None(_) => {
                        this.context = FnUnravelState::Context(ctx.borrow_mut().fork_ref())
                    }
                    FnUnravelState::Context(future) => {
                        let (context, handle) = ready!(Pin::new(future).poll(cx, ctx.borrow_mut()))
                            .map_err(FnUnravelError::Contextualize)?;
                        this.context = FnUnravelState::Write(context, handle);
                    }
                    FnUnravelState::Write(_, _) => {
                        let mut ctx = Pin::new(ctx.borrow_mut());
                        ready!(ctx.as_mut().poll_ready(cx)).map_err(FnUnravelError::Write)?;
                        let data = replace(&mut this.context, FnUnravelState::None(PhantomData));
                        if let FnUnravelState::Write(context, handle) = data {
                            ctx.write(handle).map_err(FnUnravelError::Write)?;
                            this.context = FnUnravelState::Flush(context);
                        } else {
                            panic!("invalid state")
                        }
                    }
                    FnUnravelState::Flush(_) => {
                        let mut ctx = Pin::new(ctx.borrow_mut());
                        ready!(ctx.as_mut().poll_flush(cx)).map_err(FnUnravelError::Write)?;
                        let data = replace(&mut this.context, FnUnravelState::None(PhantomData));
                        if let FnUnravelState::Flush(context) = data {
                            return Poll::Ready(Ok(ErasedFnUnravel {
                                pending: vec![],
                                conv: this.call.take().unwrap(),
                                call: this.conv.take().unwrap(),
                                context,
                                state: ErasedFnUnravelState::Read(PhantomData),
                            }));
                        } else {
                            panic!("invalid state")
                        }
                    }
                }
            } else {
                panic!("FnUnravel polled after completion")
            }
        }
    }
}

pub enum ErasedFnCoalesceState<T, U, C: CloneContext + Write<Option<<C as Contextualize>::Handle>>>
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
    Join(<C::Context as Join<<C::Context as Notify<U>>::Notification>>::Future),
    Unwrap(<C::Context as Notify<U>>::Unwrap),
    Done,
}

pub struct ErasedFnCoalesce<
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
    state: ErasedFnCoalesceState<T, U, C>,
    context: C,
    data: Option<T>,
    loc_context: Option<C::Context>,
}

impl<T, U, C: Clone + CloneContext + Write<Option<<C as Contextualize>::Handle>>> Unpin
    for ErasedFnCoalesce<T, U, C>
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
pub enum ErasedFnCoalesceError<A, C, B, T, I, U, V, W, K, L> {
    #[error("failed to write argument handle for erased Fn: {0}")]
    Write(#[source] A),
    #[error("failed to fork argument for erased Fn: {0}")]
    Fork(#[source] C),
    #[error("failed to wrap argument notification for erased Fn: {0}")]
    Wrap(#[source] B),
    #[error("failed to read handle for erased Fn return: {0}")]
    Read(#[source] T),
    #[error("failed to create notification wrapper for erased Fn return: {0}")]
    Unwrap(#[source] I),
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

type CoalesceError<T, U, C> = ErasedFnCoalesceError<
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
    for ErasedFnCoalesce<T, U, C>
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
                ErasedFnCoalesceState::Begin => {
                    this.state = ErasedFnCoalesceState::Contextualize(this.context.fork_owned());
                }
                ErasedFnCoalesceState::Contextualize(future) => {
                    let (context, handle) = ready!(Pin::new(future).poll(cx, &mut this.context))
                        .map_err(|e| {
                            ProtocolError::new(CoalesceError::<T, U, C>::Contextualize(e))
                        })?;
                    this.loc_context = Some(context);
                    this.state = ErasedFnCoalesceState::WriteHandle(handle);
                }
                ErasedFnCoalesceState::WriteHandle(_) => {
                    let state = &mut this.state;
                    let mut ctx = Pin::new(&mut this.context);
                    ready!(ctx.as_mut().poll_ready(cx)).map_err(|e| {
                        ProtocolError::new(CoalesceError::<T, U, C>::WriteHandle(e))
                    })?;
                    let data = replace(state, ErasedFnCoalesceState::Done);
                    if let ErasedFnCoalesceState::WriteHandle(handle) = data {
                        ctx.write(Some(handle)).map_err(|e| {
                            ProtocolError::new(CoalesceError::<T, U, C>::WriteHandle(e))
                        })?;
                        *state = ErasedFnCoalesceState::FlushHandle;
                    } else {
                        panic!("invalid state in ErasedFnCoalesce WriteHandle")
                    }
                }
                ErasedFnCoalesceState::FlushHandle => {
                    ready!(Pin::new(&mut this.context).poll_flush(cx)).map_err(|e| {
                        ProtocolError::new(CoalesceError::<T, U, C>::WriteHandle(e))
                    })?;
                    this.state = ErasedFnCoalesceState::Wrap(
                        this.loc_context
                            .as_mut()
                            .unwrap()
                            .wrap(this.data.take().unwrap()),
                    );
                }
                ErasedFnCoalesceState::Wrap(future) => {
                    let ctx = this.loc_context.as_mut().unwrap();
                    let wrapped = ready!(Pin::new(future).poll(cx, &mut *ctx))
                        .map_err(|e| ProtocolError::new(CoalesceError::<T, U, C>::Wrap(e)))?;
                    this.state = ErasedFnCoalesceState::Fork(ctx.fork(wrapped));
                }
                ErasedFnCoalesceState::Fork(future) => {
                    let ctx = this.loc_context.as_mut().unwrap();
                    let (target, handle) = ready!(Pin::new(future).poll(cx, &mut *ctx))
                        .map_err(|e| ProtocolError::new(CoalesceError::<T, U, C>::Fork(e)))?;
                    this.state = ErasedFnCoalesceState::Write(target, handle);
                }
                ErasedFnCoalesceState::Write(_, _) => {
                    let ctx = this.loc_context.as_mut().unwrap();
                    let mut ctx = Pin::new(&mut *ctx);
                    ready!(ctx.as_mut().poll_ready(cx))
                        .map_err(|e| ProtocolError::new(CoalesceError::<T, U, C>::Write(e)))?;
                    let data = replace(&mut this.state, ErasedFnCoalesceState::Done);
                    if let ErasedFnCoalesceState::Write(target, data) = data {
                        ctx.write(data)
                            .map_err(|e| ProtocolError::new(CoalesceError::<T, U, C>::Write(e)))?;
                        this.state = ErasedFnCoalesceState::Flush(target);
                    } else {
                        panic!("invalid state in ErasedFnCoalesce Write")
                    }
                }
                ErasedFnCoalesceState::Flush(_) => {
                    let ctx = this.loc_context.as_mut().unwrap();
                    ready!(Pin::new(&mut *ctx).poll_ready(cx))
                        .map_err(|e| ProtocolError::new(CoalesceError::<T, U, C>::Write(e)))?;
                    let data = replace(&mut this.state, ErasedFnCoalesceState::Done);
                    if let ErasedFnCoalesceState::Flush(target) = data {
                        this.state = ErasedFnCoalesceState::Target(target);
                    } else {
                        panic!("invalid state in ErasedFnUnravel Write")
                    }
                }
                ErasedFnCoalesceState::Target(target) => {
                    let ctx = this.loc_context.as_mut().unwrap();
                    let finalize = ready!(Pin::new(target).poll(cx, &mut *ctx))
                        .map_err(|e| ProtocolError::new(CoalesceError::<T, U, C>::Target(e)))?;
                    this.state = ErasedFnCoalesceState::Finalize(ctx.finalize(finalize));
                }
                ErasedFnCoalesceState::Finalize(future) => {
                    let ctx = this.loc_context.as_mut().unwrap();
                    ready!(Pin::new(future).poll(cx, &mut *ctx))
                        .map_err(|e| ProtocolError::new(CoalesceError::<T, U, C>::Finalize(e)))?;
                    this.state = ErasedFnCoalesceState::Read;
                }
                ErasedFnCoalesceState::Read => {
                    let ctx = this.loc_context.as_mut().unwrap();
                    let handle: <C::Context as Dispatch<
                        <C::Context as Notify<U>>::Notification,
                    >>::Handle = ready!(Pin::new(&mut *ctx).read(cx))
                        .map_err(|e| ProtocolError::new(CoalesceError::<T, U, C>::Read(e)))?;
                    let join: <C::Context as Join<<C::Context as Notify<U>>::Notification>>::Future =
                        Join::<<C::Context as Notify<U>>::Notification>::join(ctx, handle);
                    this.state = ErasedFnCoalesceState::Join(join);
                }
                ErasedFnCoalesceState::Join(future) => {
                    let ctx = this.loc_context.as_mut().unwrap();
                    let wrapped = ready!(Pin::new(future).poll(cx, &mut *ctx))
                        .map_err(|e| ProtocolError::new(CoalesceError::<T, U, C>::Join(e)))?;
                    let unwrap: <C::Context as Notify<U>>::Unwrap =
                        Notify::<U>::unwrap(ctx, wrapped);
                    this.state = ErasedFnCoalesceState::Unwrap(unwrap);
                }
                ErasedFnCoalesceState::Unwrap(future) => {
                    let ctx = this.loc_context.as_mut().unwrap();
                    let data = ready!(Pin::new(future).poll(cx, &mut *ctx))
                        .map_err(|e| ProtocolError::new(CoalesceError::<T, U, C>::Unwrap(e)))?;
                    this.state = ErasedFnCoalesceState::Done;
                    return Poll::Ready(Ok(data));
                }
                ErasedFnCoalesceState::Done => panic!("erased Fn coalesce polled after completion"),
            }
        }
    }
}

pub enum FnCoalesceState<T> {
    Read,
    Contextualize(T),
    Done,
}

pub enum ErasedFnComplete {
    Write,
    Flush,
    Done,
}

impl<C: Unpin + CloneContext + Write<Option<<C as Contextualize>::Handle>>> Future<C> for ErasedFnComplete {
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
                ErasedFnComplete::Write => {
                    let mut ctx = Pin::new(ctx.borrow_mut());
                    ready!(ctx.as_mut().poll_ready(cx))?;
                    ctx.write(None)?;
                    *this = ErasedFnComplete::Flush;
                }
                ErasedFnComplete::Flush => {
                    ready!(Pin::new(ctx.borrow_mut()).poll_flush(cx))?;
                    *this = ErasedFnComplete::Done;
                    return Poll::Ready(Ok(()));
                }
                ErasedFnComplete::Done => panic!("erased Fn terminator polled after completion"),
            }
        }
    }
}

pub struct ErasedFn<T, U, C: Clone + FinalizeImmediate<ErasedFnComplete>>
where
    C::Target: Unpin + CloneContext + Write<Option<<C::Target as Contextualize>::Handle>>,
{
    context: C,
    data: PhantomData<(T, U, C)>,
}

impl<T, U, C: Clone + FinalizeImmediate<ErasedFnComplete>> Drop for ErasedFn<T, U, C>
where
    C::Target: Unpin + CloneContext + Write<Option<<C::Target as Contextualize>::Handle>>,
{
    fn drop(&mut self) {
        let _ = self.context.finalize_immediate(ErasedFnComplete::Write);
    }
}

impl<
    T,
    U: Flatten<ProtocolError, ErasedFnCoalesce<T, U, C>>,
    C: Clone
        + CloneContext
        + FinalizeImmediate<ErasedFnComplete>
        + Write<Option<<C as Contextualize>::Handle>>,
> ErasedFn<T, U, C>
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
    fn call(&self, args: T) -> U {
        U::flatten(ErasedFnCoalesce {
            state: ErasedFnCoalesceState::Begin,
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
pub enum FnCoalesceError<E, T> {
    #[error("failed to read handle for erased Fn context: {0}")]
    Read(T),
    #[error("failed to contextualize erased Fn content: {0}")]
    Contextualize(E),
}

pub struct FnCoalesce<
    'a,
    O,
    P: Fn(ErasedFn<T, U, C::Context>) -> O,
    T: Unpin,
    U,
    C: ?Sized + ShareContext,
> where
    <C::Context as FinalizeImmediate<ErasedFnComplete>>::Target: Unpin
        + CloneContext
        + Write<
            Option<
                <<C::Context as FinalizeImmediate<ErasedFnComplete>>::Target as Contextualize>::Handle,
            >,
        >,
    C::Context: Unpin
        + FinalizeImmediate<ErasedFnComplete>
        + Read<<C::Context as Dispatch<<C::Context as Notify<T>>::Notification>>::Handle>
        + Notify<T>,
    <C::Context as Fork<<C::Context as Notify<T>>::Notification>>::Future: Unpin,
    <C::Context as Notify<T>>::Wrap: Unpin,
{
    conv: P,
    lifetime: PhantomData<&'a (O, T, U)>,
    state: FnCoalesceState<C::JoinOutput>,
}

impl<
    'a,
    O,
    P: Fn(ErasedFn<T, U, C::Context>) -> O,
    T: Unpin,
    U,
    C: ?Sized + Read<<C as Contextualize>::Handle> + ShareContext,
> Future<C> for FnCoalesce<'a, O, P, T, U, C>
where
    C::JoinOutput: Unpin,
    C: Unpin,
    <C::Context as FinalizeImmediate<ErasedFnComplete>>::Target: Unpin
        + CloneContext
        + Write<
            Option<
                <<C::Context as FinalizeImmediate<ErasedFnComplete>>::Target as Contextualize>::Handle,
            >,
        >,
    C::Context: Unpin,
    P: Unpin,
    <C::Context as Notify<T>>::Wrap: Unpin,
    C::Context: FinalizeImmediate<ErasedFnComplete>
        + Unpin
        + Read<<C::Context as Dispatch<<C::Context as Notify<T>>::Notification>>::Handle>
        + Notify<T>,
    <C::Context as Fork<<C::Context as Notify<T>>::Notification>>::Future: Unpin,
{
    type Ok = O;
    type Error = FnCoalesceError<<C::JoinOutput as Future<C>>::Error, C::Error>;

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
                FnCoalesceState::Read => {
                    let ct = Pin::new(&mut *ctx);
                    let handle = ready!(ct.read(cx)).map_err(FnCoalesceError::Read)?;
                    this.state = FnCoalesceState::Contextualize(ctx.join_shared(handle));
                }
                FnCoalesceState::Contextualize(future) => {
                    let context = ready!(Pin::new(future).poll(cx, &mut *ctx))
                        .map_err(FnCoalesceError::Contextualize)?;
                    replace(&mut this.state, FnCoalesceState::Done);
                    return Poll::Ready(Ok((this.conv)(ErasedFn {
                        context,
                        data: PhantomData,
                    })));
                }
                FnCoalesceState::Done => panic!("FnCoalesce polled after completion"),
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
        tuple_impls! {FnMut $(($($marker)*) => (
            $($n $name)*
        ))+}
        tuple_impls! {Fn $(($($marker)*) => (
            $($n $name)*
        ))+}
    };
    (
        $trait:ident $(($($marker:ident)*) => (
            $($n:tt $name:ident)*
        ))+
    ) => {
        $(
            impl<
                'a,
                $($name: Unpin + 'a,)*
                U: Flatten<ProtocolError, ErasedFnCoalesce<($($name,)*), U, C::Context>> + 'a $(+ $marker)*,
                C: ?Sized + Read<<C as Contextualize>::Handle> + ShareContext,
            > Coalesce<C> for Box<dyn $trait($($name,)*) -> U + 'a $(+ $marker)*>
            where
                <C::Context as FinalizeImmediate<ErasedFnComplete>>::Target: Unpin
                    + CloneContext
                    + Write<
                        Option<
                            <<C::Context as FinalizeImmediate<ErasedFnComplete>>::Target as Contextualize>::Handle,
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
                    + FinalizeImmediate<ErasedFnComplete>
                    + CloneContext
                    + Write<Option<<C::Context as Contextualize>::Handle>>
                    $(+ $marker)*
                    + 'a,
                <C::Context as Fork<<C::Context as Notify<($($name,)*)>>::Notification>>::Future: Unpin,
            {
                type Future = FnCoalesce<
                    'a,
                    Self,
                    fn(ErasedFn<($($name,)*), U, C::Context>) -> Self,
                    ($($name,)*),
                    U,
                    C,
                >;

                #[allow(non_snake_case, unused_mut)]
                fn coalesce() -> Self::Future {
                    FnCoalesce {
                        lifetime: PhantomData,
                        conv: |mut erased| Box::new(move |$($name,)*| erased.call(($($name,)*))),
                        state: FnCoalesceState::Read,
                    }
                }
            }

            impl<'a, $($name: 'a,)* U: 'a, C: ?Sized + Write<<C as Contextualize>::Handle> + ReferenceContext> Unravel<C>
                for Box<dyn $trait($($name,)*) -> U + 'a $(+ $marker)*>
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
                type Finalize = ErasedFnUnravel<
                    C,
                    ($($name,)*),
                    U,
                    Self,
                    fn(&mut Self, ($($name,)*)) -> U,
                >;
                type Target = FnUnravel<
                    C,
                    ($($name,)*),
                    U,
                    Self,
                    fn(&mut Self, ($($name,)*)) -> U,
                >;

                #[allow(unused_variables)]
                fn unravel(self) -> Self::Target {
                    FnUnravel {
                        call: Some(self),
                        conv: Some(|call, data| (call)($(data.$n,)*)),
                        context: FnUnravelState::None(PhantomData),
                    }
                }
            }
        )+
    }
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
        )+
    };
}

marker_variants! {
    ,
    Sync,
    Send, Sync Send
}
