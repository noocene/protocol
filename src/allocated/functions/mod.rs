use crate::{
    allocated::{Flatten, ProtocolError},
    future::MapErr,
    CloneContext, Coalesce, ContextReference, Contextualize, Dispatch, Finalize, FinalizeImmediate,
    Fork, ForkContextRef, ForkContextRefError, Future, FutureExt, Join, JoinContextShared as Jcs,
    Notify, Read, RefContextTarget, ReferenceContext, ShareContext, Unravel, Write,
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

type JoinContextShared<O, C> = Jcs<C, O, fn(<C as ShareContext>::Context) -> O>;

#[derive(Debug, Error)]
#[bounds(
    where
        A: Error + 'static,
        T: Error + 'static,
        B: Error + 'static,
        U: Error + 'static,
        V: Error + 'static,
        W: Error + 'static,
        Z: Error + 'static,
        C: Error + 'static,
        F: Error + 'static,
        P: Error + 'static,
)]
pub enum FnUnravelError<A, B, C, Z, T, U, V, W, F, P> {
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
    #[error("failed to write handle for erased Fn: {0}")]
    Transport(#[source] T),
    #[error("failed to read handle for erased Fn: {0}")]
    ReadHandle(#[source] F),
    #[error("failed to fork erased Fn return: {0}")]
    Dispatch(#[source] U),
    #[error("failed to target erased Fn return: {0}")]
    Target(#[source] V),
    #[error("failed to finalize erased Fn return: {0}")]
    Finalize(#[source] W),
}

pub type UnravelError<T, U, C> = FnUnravelError<
    <RefContextTarget<RefContextTarget<C>> as Read<
        <RefContextTarget<RefContextTarget<C>> as Dispatch<
            <RefContextTarget<RefContextTarget<C>> as Notify<T>>::Notification,
        >>::Handle,
    >>::Error,
    <<RefContextTarget<RefContextTarget<C>> as Notify<T>>::Unwrap as Future<
        RefContextTarget<RefContextTarget<C>>,
    >>::Error,
    <<RefContextTarget<RefContextTarget<C>> as Join<
        <RefContextTarget<RefContextTarget<C>> as Notify<T>>::Notification,
    >>::Future as Future<RefContextTarget<RefContextTarget<C>>>>::Error,
    ForkContextRefError<
        <<C as ReferenceContext>::ForkOutput as Future<C>>::Error,
        <C as Write<<C as Contextualize>::Handle>>::Error,
    >,
    <RefContextTarget<RefContextTarget<C>> as Write<
        <RefContextTarget<RefContextTarget<C>> as Dispatch<U>>::Handle,
    >>::Error,
    <<RefContextTarget<RefContextTarget<C>> as Fork<U>>::Future as Future<
        RefContextTarget<RefContextTarget<C>>,
    >>::Error,
    <<RefContextTarget<RefContextTarget<C>> as Fork<U>>::Target as Future<
        RefContextTarget<RefContextTarget<C>>,
    >>::Error,
    <<RefContextTarget<RefContextTarget<C>> as Fork<U>>::Finalize as Future<
        RefContextTarget<RefContextTarget<C>>,
    >>::Error,
    <RefContextTarget<C> as Read<Option<<RefContextTarget<C> as Contextualize>::Handle>>>::Error,
    <<RefContextTarget<C> as ReferenceContext>::JoinOutput as Future<RefContextTarget<C>>>::Error,
>;

pub enum ErasedFnUnravelInstanceState<C: ?Sized + ReferenceContext, T, U>
where
    RefContextTarget<C>: ReferenceContext,
    RefContextTarget<RefContextTarget<C>>: Notify<T>
        + Fork<U>
        + Read<
            <RefContextTarget<RefContextTarget<C>> as Dispatch<
                <RefContextTarget<RefContextTarget<C>> as Notify<T>>::Notification,
            >>::Handle,
        > + Write<<RefContextTarget<RefContextTarget<C>> as Dispatch<U>>::Handle>,
{
    Read,
    Join(
        <RefContextTarget<RefContextTarget<C>> as Join<
            <RefContextTarget<RefContextTarget<C>> as Notify<T>>::Notification,
        >>::Future,
    ),
    Unwrap(<RefContextTarget<RefContextTarget<C>> as Notify<T>>::Unwrap),
    Fork(<RefContextTarget<RefContextTarget<C>> as Fork<U>>::Future),
    Write(
        <RefContextTarget<RefContextTarget<C>> as Fork<U>>::Target,
        <RefContextTarget<RefContextTarget<C>> as Dispatch<U>>::Handle,
    ),
    Flush(<RefContextTarget<RefContextTarget<C>> as Fork<U>>::Target),
    Target(<RefContextTarget<RefContextTarget<C>> as Fork<U>>::Target),
    Finalize(<RefContextTarget<RefContextTarget<C>> as Fork<U>>::Finalize),
    Done,
}

pub struct ErasedFnUnravelInstance<C: ?Sized + ReferenceContext, T, U, F, M>
where
    RefContextTarget<C>: ReferenceContext,
    RefContextTarget<RefContextTarget<C>>: Notify<T>
        + Fork<U>
        + Read<
            <RefContextTarget<RefContextTarget<C>> as Dispatch<
                <RefContextTarget<RefContextTarget<C>> as Notify<T>>::Notification,
            >>::Handle,
        > + Write<<RefContextTarget<RefContextTarget<C>> as Dispatch<U>>::Handle>,
{
    state: ErasedFnUnravelInstanceState<C, T, U>,
    marker: PhantomData<(F, M)>,
    context: <RefContextTarget<C> as ReferenceContext>::Context,
}

impl<C: ?Sized + ReferenceContext, T, U, F, M> ErasedFnUnravelInstance<C, T, U, F, M>
where
    RefContextTarget<C>: ReferenceContext,
    RefContextTarget<RefContextTarget<C>>: Notify<T>
        + Fork<U>
        + Read<
            <RefContextTarget<RefContextTarget<C>> as Dispatch<
                <RefContextTarget<RefContextTarget<C>> as Notify<T>>::Notification,
            >>::Handle,
        > + Write<<RefContextTarget<RefContextTarget<C>> as Dispatch<U>>::Handle>,
{
    pub fn new(context: <RefContextTarget<C> as ReferenceContext>::Context) -> Self {
        ErasedFnUnravelInstance {
            state: ErasedFnUnravelInstanceState::Read,
            marker: PhantomData,
            context,
        }
    }
}

impl<C: ?Sized + ReferenceContext, T, U, F, M> Unpin for ErasedFnUnravelInstance<C, T, U, F, M>
where
    RefContextTarget<C>: ReferenceContext,
    RefContextTarget<RefContextTarget<C>>: Notify<T>
        + Fork<U>
        + Read<
            <RefContextTarget<RefContextTarget<C>> as Dispatch<
                <RefContextTarget<RefContextTarget<C>> as Notify<T>>::Notification,
            >>::Handle,
        > + Write<<RefContextTarget<RefContextTarget<C>> as Dispatch<U>>::Handle>,
{
}

impl<C: ?Sized + ReferenceContext + Write<<C as Contextualize>::Handle>, T, U, F, M>
    ErasedFnUnravelInstance<C, T, U, F, M>
where
    for<'a> M: Fn(&'a mut F, T) -> U,
    RefContextTarget<C>:
        ReferenceContext + Read<Option<<RefContextTarget<C> as Contextualize>::Handle>>,
    RefContextTarget<RefContextTarget<C>>: Notify<T>
        + Fork<U>
        + Read<
            <RefContextTarget<RefContextTarget<C>> as Dispatch<
                <RefContextTarget<RefContextTarget<C>> as Notify<T>>::Notification,
            >>::Handle,
        > + Write<<RefContextTarget<RefContextTarget<C>> as Dispatch<U>>::Handle>,
    RefContextTarget<RefContextTarget<C>>: Unpin,
    <RefContextTarget<RefContextTarget<C>> as Join<
        <RefContextTarget<RefContextTarget<C>> as Notify<T>>::Notification,
    >>::Future: Unpin,
    <RefContextTarget<RefContextTarget<C>> as Notify<T>>::Unwrap: Unpin,
    <RefContextTarget<RefContextTarget<C>> as Fork<U>>::Target: Unpin,
    <RefContextTarget<RefContextTarget<C>> as Fork<U>>::Finalize: Unpin,
    <RefContextTarget<RefContextTarget<C>> as Fork<U>>::Future: Unpin,
{
    fn poll<R: BorrowMut<RefContextTarget<C>>>(
        mut self: Pin<&mut Self>,
        cx: &mut Context,
        ctx: R,
        conv: &M,
        call: &mut F,
    ) -> Poll<Result<(), UnravelError<T, U, C>>> {
        use ErasedFnUnravelInstanceState::*;

        let this = &mut *self;

        let ctx = this.context.with(ctx);

        loop {
            match &mut this.state {
                Read => {
                    let mut ct = Pin::new(&mut *ctx);
                    let handle = ready!(ct.as_mut().read(cx)).map_err(FnUnravelError::Read)?;
                    this.state = Join(crate::Join::<
                        <RefContextTarget<RefContextTarget<C>> as Notify<T>>::Notification,
                    >::join(&mut *ctx, handle));
                }
                Join(future) => {
                    let notification = ready!(Pin::new(future).poll(cx, &mut *ctx))
                        .map_err(FnUnravelError::Join)?;
                    this.state = Unwrap(
                        <RefContextTarget<RefContextTarget<C>> as Notify<T>>::unwrap(
                            &mut *ctx,
                            notification,
                        ),
                    );
                }
                Unwrap(future) => {
                    let item = ready!(Pin::new(future).poll(cx, &mut *ctx))
                        .map_err(FnUnravelError::Unwrap)?;
                    let item = (conv)(call, item);
                    this.state = Fork(ctx.fork(item));
                }
                Fork(future) => {
                    let (target, handle) = ready!(Pin::new(future).poll(cx, &mut *ctx))
                        .map_err(FnUnravelError::Dispatch)?;
                    this.state = Write(target, handle);
                }
                Write(_, _) => {
                    let mut ctx = Pin::new(&mut *ctx);
                    ready!(ctx.as_mut().poll_ready(cx)).map_err(FnUnravelError::Transport)?;
                    let data = replace(&mut this.state, Done);
                    if let Write(target, data) = data {
                        ctx.write(data).map_err(FnUnravelError::Transport)?;
                        this.state = Flush(target);
                    } else {
                        panic!("invalid state in ErasedFnUnravel Write")
                    }
                }
                Flush(_) => {
                    ready!(Pin::new(&mut *ctx).poll_flush(cx))
                        .map_err(FnUnravelError::Transport)?;
                    let data = replace(&mut this.state, Done);
                    if let Flush(target) = data {
                        this.state = Target(target);
                    } else {
                        panic!("invalid state in ErasedFnUnravel Write")
                    }
                }
                Target(target) => {
                    let finalize = ready!(Pin::new(target).poll(cx, &mut *ctx))
                        .map_err(FnUnravelError::Target)?;
                    this.state = Finalize(finalize);
                }
                Finalize(finalize) => {
                    ready!(Pin::new(finalize).poll(cx, &mut *ctx))
                        .map_err(FnUnravelError::Finalize)?;
                    this.state = Done;
                    return Poll::Ready(Ok(()));
                }
                Done => panic!("ErasedFnUnravel polled after completion"),
            }
        }
    }
}

enum ErasedFnUnravelState<C: ?Sized + Write<<C as Contextualize>::Handle> + ReferenceContext, T, U>
where
    RefContextTarget<C>: ReferenceContext,
    RefContextTarget<RefContextTarget<C>>: Notify<T>
        + Fork<U>
        + Read<
            <RefContextTarget<RefContextTarget<C>> as Dispatch<
                <RefContextTarget<RefContextTarget<C>> as Notify<T>>::Notification,
            >>::Handle,
        > + Write<<RefContextTarget<RefContextTarget<C>> as Dispatch<U>>::Handle>,
    RefContextTarget<RefContextTarget<C>>: Unpin,
    <RefContextTarget<RefContextTarget<C>> as Join<
        <RefContextTarget<RefContextTarget<C>> as Notify<T>>::Notification,
    >>::Future: Unpin,
    <RefContextTarget<RefContextTarget<C>> as Notify<T>>::Unwrap: Unpin,
    <RefContextTarget<RefContextTarget<C>> as Fork<U>>::Target: Unpin,
    <RefContextTarget<RefContextTarget<C>> as Fork<U>>::Finalize: Unpin,
    <RefContextTarget<RefContextTarget<C>> as Fork<U>>::Future: Unpin,
{
    Read(PhantomData<(T, U)>),
    Contextualize(<RefContextTarget<C> as ReferenceContext>::JoinOutput),
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
    RefContextTarget<C>:
        ReferenceContext + Read<Option<<RefContextTarget<C> as Contextualize>::Handle>>,
    RefContextTarget<RefContextTarget<C>>: Notify<T>
        + Fork<U>
        + Read<
            <RefContextTarget<RefContextTarget<C>> as Dispatch<
                <RefContextTarget<RefContextTarget<C>> as Notify<T>>::Notification,
            >>::Handle,
        > + Write<<RefContextTarget<RefContextTarget<C>> as Dispatch<U>>::Handle>,
    RefContextTarget<RefContextTarget<C>>: Unpin,
    <RefContextTarget<RefContextTarget<C>> as Join<
        <RefContextTarget<RefContextTarget<C>> as Notify<T>>::Notification,
    >>::Future: Unpin,
    <RefContextTarget<RefContextTarget<C>> as Notify<T>>::Unwrap: Unpin,
    <RefContextTarget<RefContextTarget<C>> as Fork<U>>::Target: Unpin,
    <RefContextTarget<RefContextTarget<C>> as Fork<U>>::Finalize: Unpin,
    <RefContextTarget<RefContextTarget<C>> as Fork<U>>::Future: Unpin,
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
    RefContextTarget<C>:
        ReferenceContext + Read<Option<<RefContextTarget<C> as Contextualize>::Handle>>,
    RefContextTarget<RefContextTarget<C>>: Notify<T>
        + Fork<U>
        + Read<
            <RefContextTarget<RefContextTarget<C>> as Dispatch<
                <RefContextTarget<RefContextTarget<C>> as Notify<T>>::Notification,
            >>::Handle,
        > + Write<<RefContextTarget<RefContextTarget<C>> as Dispatch<U>>::Handle>,
    RefContextTarget<RefContextTarget<C>>: Unpin,
    <RefContextTarget<RefContextTarget<C>> as Join<
        <RefContextTarget<RefContextTarget<C>> as Notify<T>>::Notification,
    >>::Future: Unpin,
    <RefContextTarget<RefContextTarget<C>> as Notify<T>>::Unwrap: Unpin,
    <RefContextTarget<RefContextTarget<C>> as Fork<U>>::Target: Unpin,
    <RefContextTarget<RefContextTarget<C>> as Fork<U>>::Finalize: Unpin,
    <RefContextTarget<RefContextTarget<C>> as Fork<U>>::Future: Unpin,
{
}

impl<C: ?Sized + Write<<C as Contextualize>::Handle> + ReferenceContext, T, U, F, M> Future<C>
    for ErasedFnUnravel<C, T, U, F, M>
where
    for<'a> M: Fn(&'a mut F, T) -> U,
    RefContextTarget<C>:
        ReferenceContext + Read<Option<<RefContextTarget<C> as Contextualize>::Handle>>,
    RefContextTarget<RefContextTarget<C>>: Notify<T>
        + Fork<U>
        + Read<
            <RefContextTarget<RefContextTarget<C>> as Dispatch<
                <RefContextTarget<RefContextTarget<C>> as Notify<T>>::Notification,
            >>::Handle,
        > + Write<<RefContextTarget<RefContextTarget<C>> as Dispatch<U>>::Handle>,
    RefContextTarget<RefContextTarget<C>>: Unpin,
    <RefContextTarget<RefContextTarget<C>> as Join<
        <RefContextTarget<RefContextTarget<C>> as Notify<T>>::Notification,
    >>::Future: Unpin,
    <RefContextTarget<RefContextTarget<C>> as Notify<T>>::Unwrap: Unpin,
    <RefContextTarget<RefContextTarget<C>> as Fork<U>>::Target: Unpin,
    <RefContextTarget<RefContextTarget<C>> as Fork<U>>::Finalize: Unpin,
    <RefContextTarget<RefContextTarget<C>> as Fork<U>>::Future: Unpin,
    RefContextTarget<C>: Unpin,
    <RefContextTarget<C> as ReferenceContext>::JoinOutput: Unpin,
{
    type Ok = ();
    type Error = UnravelError<T, U, C>;

    fn poll<R: BorrowMut<C>>(
        mut self: Pin<&mut Self>,
        cx: &mut Context,
        ctx: R,
    ) -> Poll<Result<Self::Ok, Self::Error>> {
        use ErasedFnUnravelState::*;

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
                Read(_) => {
                    let handle = ready!(Pin::new(&mut *context).read(cx))
                        .map_err(FnUnravelError::ReadHandle)?;
                    this.state = if let Some(handle) = handle {
                        Contextualize(context.join_ref(handle))
                    } else {
                        Done
                    };
                }
                Contextualize(future) => {
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
                    this.state = Read(PhantomData);
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

pub enum ErasedFnCoalesceState<T, U, C: CloneContext + Write<Option<<C as Contextualize>::Handle>>>
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

pub struct ErasedFnCoalesce<
    T,
    U,
    C: Clone + CloneContext + Write<Option<<C as Contextualize>::Handle>>,
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
    state: ErasedFnCoalesceState<T, U, C>,
    context: C,
    data: Option<T>,
    loc_context: Option<C::Context>,
}

impl<T, U, C: Clone + CloneContext + Write<Option<<C as Contextualize>::Handle>>> Unpin
    for ErasedFnCoalesce<T, U, C>
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
pub enum ErasedFnCoalesceError<A, C, B, T, U, V, W, K, L> {
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
    <C as Write<Option<<C as Contextualize>::Handle>>>::Error,
>;

impl<T, U, C: Clone + CloneContext + Write<Option<<C as Contextualize>::Handle>>> future::Future
    for ErasedFnCoalesce<T, U, C>
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
        <<C as CloneContext>::Context as Dispatch<
            <<C as CloneContext>::Context as Notify<T>>::Notification,
        >>::Handle,
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
    <C as Write<Option<<C as Contextualize>::Handle>>>::Error: Send + Error + 'static,
{
    type Output = Result<U, ProtocolError>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        use ErasedFnCoalesceState::*;

        let this = &mut *self;

        loop {
            match &mut this.state {
                Begin => {
                    this.state = Contextualize(this.context.fork_owned());
                }
                Contextualize(future) => {
                    let (context, handle) = ready!(Pin::new(future).poll(cx, &mut this.context))
                        .map_err(|e| {
                            ProtocolError::new(CoalesceError::<T, U, C>::Contextualize(e))
                        })?;
                    this.loc_context = Some(context);
                    this.state = WriteHandle(handle);
                }
                WriteHandle(_) => {
                    let state = &mut this.state;
                    let mut ctx = Pin::new(&mut this.context);
                    ready!(ctx.as_mut().poll_ready(cx)).map_err(|e| {
                        ProtocolError::new(CoalesceError::<T, U, C>::WriteHandle(e))
                    })?;
                    let data = replace(state, Done);
                    if let WriteHandle(handle) = data {
                        ctx.write(Some(handle)).map_err(|e| {
                            ProtocolError::new(CoalesceError::<T, U, C>::WriteHandle(e))
                        })?;
                        *state = FlushHandle;
                    } else {
                        panic!("invalid state in ErasedFnCoalesce WriteHandle")
                    }
                }
                FlushHandle => {
                    ready!(Pin::new(&mut this.context).poll_flush(cx)).map_err(|e| {
                        ProtocolError::new(CoalesceError::<T, U, C>::WriteHandle(e))
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
                        .map_err(|e| ProtocolError::new(CoalesceError::<T, U, C>::Wrap(e)))?;
                    this.state = Fork(ctx.fork(wrapped));
                }
                Fork(future) => {
                    let ctx = this.loc_context.as_mut().unwrap();
                    let (target, handle) = ready!(Pin::new(future).poll(cx, &mut *ctx))
                        .map_err(|e| ProtocolError::new(CoalesceError::<T, U, C>::Fork(e)))?;
                    this.state = Write(target, handle);
                }
                Write(_, _) => {
                    let ctx = this.loc_context.as_mut().unwrap();
                    let mut ctx = Pin::new(&mut *ctx);
                    ready!(ctx.as_mut().poll_ready(cx))
                        .map_err(|e| ProtocolError::new(CoalesceError::<T, U, C>::Write(e)))?;
                    let data = replace(&mut this.state, Done);
                    if let Write(target, data) = data {
                        ctx.write(data)
                            .map_err(|e| ProtocolError::new(CoalesceError::<T, U, C>::Write(e)))?;
                        this.state = Flush(target);
                    } else {
                        panic!("invalid state in ErasedFnCoalesce Write")
                    }
                }
                Flush(_) => {
                    let ctx = this.loc_context.as_mut().unwrap();
                    ready!(Pin::new(&mut *ctx).poll_ready(cx))
                        .map_err(|e| ProtocolError::new(CoalesceError::<T, U, C>::Write(e)))?;
                    let data = replace(&mut this.state, Done);
                    if let Flush(target) = data {
                        this.state = Target(target);
                    } else {
                        panic!("invalid state in ErasedFnUnravel Write")
                    }
                }
                Target(target) => {
                    let ctx = this.loc_context.as_mut().unwrap();
                    let finalize = ready!(Pin::new(target).poll(cx, &mut *ctx))
                        .map_err(|e| ProtocolError::new(CoalesceError::<T, U, C>::Target(e)))?;
                    this.state = Finalize(ctx.finalize(finalize));
                }
                Finalize(future) => {
                    let ctx = this.loc_context.as_mut().unwrap();
                    ready!(Pin::new(future).poll(cx, &mut *ctx))
                        .map_err(|e| ProtocolError::new(CoalesceError::<T, U, C>::Finalize(e)))?;
                    this.state = Read;
                }
                Read => {
                    let ctx = this.loc_context.as_mut().unwrap();
                    let handle: <C::Context as Dispatch<U>>::Handle =
                        ready!(Pin::new(&mut *ctx).read(cx))
                            .map_err(|e| ProtocolError::new(CoalesceError::<T, U, C>::Read(e)))?;
                    let join: <C::Context as crate::Join<U>>::Future =
                        crate::Join::<U>::join(ctx, handle);
                    this.state = Join(join);
                }
                Join(future) => {
                    let ctx = this.loc_context.as_mut().unwrap();
                    let data = ready!(Pin::new(future).poll(cx, &mut *ctx))
                        .map_err(|e| ProtocolError::new(CoalesceError::<T, U, C>::Join(e)))?;
                    this.state = Done;
                    return Poll::Ready(Ok(data));
                }
                Done => panic!("erased Fn coalesce polled after completion"),
            }
        }
    }
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
        <<C as CloneContext>::Context as Dispatch<
            <<C as CloneContext>::Context as Notify<T>>::Notification,
        >>::Handle,
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
                    + Join<U>
                    + Write<
                        <<C::Context as CloneContext>::Context as Dispatch<
                            <<C::Context as CloneContext>::Context as Notify<($($name,)*)>>::Notification,
                        >>::Handle,
                    > + Read<
                        <<C::Context as CloneContext>::Context as Dispatch<
                            U,
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
                        U,
                    >>::Handle,
                >>::Error: Send + Error + 'static,
                <<<C::Context as CloneContext>::Context as Join<
                    U,
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
                <<C::Context as CloneContext>::Context as Fork<
                    <<C::Context as CloneContext>::Context as Notify<($($name,)*)>>::Notification,
                >>::Future: Unpin,
                <<C::Context as CloneContext>::Context as Join<
                    U,
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
                type Future = JoinContextShared<Self, C>;

                #[allow(non_snake_case, unused_mut)]
                fn coalesce() -> Self::Future {
                    JoinContextShared::new(|context| {
                        let erased = ErasedFn {
                            context,
                            data: PhantomData
                        };
                        Box::new(move |$($name,)*| erased.call(($($name,)*)))
                    })
                }
            }

            impl<'a, $($name: 'a,)* U: 'a, C: ?Sized + Write<<C as Contextualize>::Handle> + ReferenceContext> Unravel<C>
                for Box<dyn $trait($($name,)*) -> U + 'a $(+ $marker)*>
            where
                RefContextTarget<C>: ReferenceContext
                    + Read<Option<<RefContextTarget<C> as Contextualize>::Handle>>,
                RefContextTarget<RefContextTarget<C>>: Notify<($($name,)*)>
                    + Fork<U>
                    + Read<
                        <RefContextTarget<RefContextTarget<C>> as Dispatch<
                            <RefContextTarget<RefContextTarget<C>> as Notify<($($name,)*)>>::Notification,
                        >>::Handle,
                    > + Write<
                        <RefContextTarget<RefContextTarget<C>> as Dispatch<
                            U,
                        >>::Handle,
                    >,
                RefContextTarget<RefContextTarget<C>>: Unpin,
                <RefContextTarget<RefContextTarget<C>> as Join<
                    <RefContextTarget<RefContextTarget<C>> as Notify<($($name,)*)>>::Notification,
                >>::Future: Unpin,
                <RefContextTarget<RefContextTarget<C>> as Notify<($($name,)*)>>::Unwrap: Unpin,
                <RefContextTarget<RefContextTarget<C>> as Fork<
                    U,
                >>::Target: Unpin,
                <RefContextTarget<RefContextTarget<C>> as Fork<
                    U,
                >>::Finalize: Unpin,
                <RefContextTarget<RefContextTarget<C>> as Fork<
                    U,
                >>::Future: Unpin,
                C::ForkOutput: Unpin,
                C: Unpin,
                RefContextTarget<C>: Unpin,
                <RefContextTarget<C> as ReferenceContext>::JoinOutput:
                    Unpin,
            {
                type Finalize = ErasedFnUnravel<
                    C,
                    ($($name,)*),
                    U,
                    Self,
                    fn(&mut Self, ($($name,)*)) -> U,
                >;
                type Target = MapErr<ForkContextRef<
                    C,
                    Self::Finalize,
                    Self,
                    fn(Self, <C as ReferenceContext>::Context) -> Self::Finalize,
                >, fn(ForkContextRefError<<<C as ReferenceContext>::ForkOutput as Future<C>>::Error, <C as Write<<C as Contextualize>::Handle>>::Error>) -> UnravelError<($($name,)*), U, C>>;

                #[allow(unused_variables)]
                fn unravel(self) -> Self::Target {
                    ForkContextRef::new(self, |conv, context| -> Self::Finalize {
                        ErasedFnUnravel {
                            pending: vec![],
                            call: |method, data: ($($name,)*)| (method)($(data.$n,)*),
                            conv,
                            context,
                            state: ErasedFnUnravelState::Read(PhantomData),
                        }
                    } as fn(Self, <C as ReferenceContext>::Context) -> Self::Finalize).map_err(FnUnravelError::Contextualize)
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
