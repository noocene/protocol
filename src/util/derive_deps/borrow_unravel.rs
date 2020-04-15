use crate::{
    ContextReference, Contextualize, Dispatch, Fork, Future, Join, Notify, Read, RefContextTarget,
    ReferenceContext, Write,
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
pub enum BorrowUnravelError<A, B, C, T, U, V, W> {
    #[error("failed to read argument handle for erased object function: {0}")]
    Read(#[source] A),
    #[error("failed to join argument for erased object function: {0}")]
    Join(#[source] C),
    #[error("failed to unwrap argument notification for erased object function: {0}")]
    Unwrap(#[source] B),
    #[error("failed to write handle for erased object function: {0}")]
    Transport(#[source] T),
    #[error("failed to fork erased object function return: {0}")]
    Dispatch(#[source] U),
    #[error("failed to target erased object function return: {0}")]
    Target(#[source] V),
    #[error("failed to finalize erased object function return: {0}")]
    Finalize(#[source] W),
}

pub type UnravelError<T, U, C> = BorrowUnravelError<
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
>;

pub enum BorrowUnravelState<C: ?Sized + ReferenceContext, T, U>
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

pub struct BorrowUnravel<C: ?Sized + ReferenceContext, T, U>
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
    state: BorrowUnravelState<C, T, U>,
    context: <RefContextTarget<C> as ReferenceContext>::Context,
}

impl<C: ?Sized + ReferenceContext, T, U> Unpin for BorrowUnravel<C, T, U>
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

impl<C: ?Sized + ReferenceContext + Write<<C as Contextualize>::Handle>, T, U>
    BorrowUnravel<C, T, U>
where
    RefContextTarget<C>: ReferenceContext + Read<<RefContextTarget<C> as Contextualize>::Handle>,
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
    pub fn new(context: <RefContextTarget<C> as ReferenceContext>::Context) -> Self {
        BorrowUnravel {
            context,
            state: BorrowUnravelState::Read,
        }
    }

    pub fn poll<R: BorrowMut<RefContextTarget<C>>>(
        mut self: Pin<&mut Self>,
        cx: &mut Context,
        ctx: R,
        mut call: impl FnMut(T) -> U,
    ) -> Poll<Result<(), UnravelError<T, U, C>>> {
        use BorrowUnravelState::*;

        let this = &mut *self;

        let ctx = this.context.with(ctx);

        loop {
            match &mut this.state {
                Read => {
                    let mut ct = Pin::new(&mut *ctx);
                    let handle = ready!(ct.as_mut().read(cx)).map_err(BorrowUnravelError::Read)?;
                    this.state = Join(crate::Join::<
                        <RefContextTarget<RefContextTarget<C>> as Notify<T>>::Notification,
                    >::join(&mut *ctx, handle));
                }
                Join(future) => {
                    let notification = ready!(Pin::new(future).poll(cx, &mut *ctx))
                        .map_err(BorrowUnravelError::Join)?;
                    this.state = Unwrap(
                        <RefContextTarget<RefContextTarget<C>> as Notify<T>>::unwrap(
                            &mut *ctx,
                            notification,
                        ),
                    );
                }
                Unwrap(future) => {
                    let item = ready!(Pin::new(future).poll(cx, &mut *ctx))
                        .map_err(BorrowUnravelError::Unwrap)?;
                    let item = (call)(item);
                    this.state = Fork(ctx.fork(item));
                }
                Fork(future) => {
                    let (target, handle) = ready!(Pin::new(future).poll(cx, &mut *ctx))
                        .map_err(BorrowUnravelError::Dispatch)?;
                    this.state = Write(target, handle);
                }
                Write(_, _) => {
                    let mut ctx = Pin::new(&mut *ctx);
                    ready!(ctx.as_mut().poll_ready(cx)).map_err(BorrowUnravelError::Transport)?;
                    let data = replace(&mut this.state, Done);
                    if let Write(target, data) = data {
                        ctx.write(data).map_err(BorrowUnravelError::Transport)?;
                        this.state = Flush(target);
                    } else {
                        panic!("invalid state in BorrowUnravel Write")
                    }
                }
                Flush(_) => {
                    ready!(Pin::new(&mut *ctx).poll_flush(cx))
                        .map_err(BorrowUnravelError::Transport)?;
                    let data = replace(&mut this.state, Done);
                    if let Flush(target) = data {
                        this.state = Target(target);
                    } else {
                        panic!("invalid state in BorrowUnravel Write")
                    }
                }
                Target(target) => {
                    let finalize = ready!(Pin::new(target).poll(cx, &mut *ctx))
                        .map_err(BorrowUnravelError::Target)?;
                    this.state = Finalize(finalize);
                }
                Finalize(finalize) => {
                    ready!(Pin::new(finalize).poll(cx, &mut *ctx))
                        .map_err(BorrowUnravelError::Finalize)?;
                    this.state = Done;
                    return Poll::Ready(Ok(()));
                }
                Done => panic!("BorrowUnravel polled after completion"),
            }
        }
    }
}
