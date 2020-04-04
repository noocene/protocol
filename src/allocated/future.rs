use super::{Flatten, FromError, ProtocolError};
use crate::{
    future::MapErr, CloneContext, Coalesce, ContextReference, Contextualize, Dispatch, Fork,
    ForkContextRef as Fcr, Future, FutureExt, Join, JoinContextOwned as Jco, Notification, Notify,
    Read, RefContextTarget, ReferenceContext, Unravel, Write,
};
use alloc::boxed::Box;
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
    ready, FutureExt as _, TryFutureExt,
};
use thiserror::Error;

type ForkContextRef<T, C> = Fcr<
    C,
    ErasedFutureUnravel<T, C>,
    T,
    fn(T, <C as ReferenceContext>::Context) -> ErasedFutureUnravel<T, C>,
>;

type JoinContextOwned<O, C> = Jco<C, O, fn(<C as CloneContext>::Context) -> O>;

type OutputNotification<C, T> = Notification<RefContextTarget<C>, <T as future::Future>::Output>;

pub enum ErasedFutureUnravelState<T: future::Future, C: ?Sized + ReferenceContext + Unpin>
where
    RefContextTarget<C>: Write<<RefContextTarget<C> as Dispatch<OutputNotification<C, T>>>::Handle>
        + Notify<T::Output>,
{
    Future(T),
    Wrap(<RefContextTarget<C> as Notify<T::Output>>::Wrap),
    Fork(<RefContextTarget<C> as Fork<OutputNotification<C, T>>>::Future),
    Write(
        <RefContextTarget<C> as Dispatch<OutputNotification<C, T>>>::Handle,
        <RefContextTarget<C> as Fork<OutputNotification<C, T>>>::Target,
    ),
    Flush(<RefContextTarget<C> as Fork<OutputNotification<C, T>>>::Target),
    Target(<RefContextTarget<C> as Fork<OutputNotification<C, T>>>::Target),
    Finalize(<RefContextTarget<C> as Fork<OutputNotification<C, T>>>::Finalize),
    Done,
}

pub struct ErasedFutureUnravel<T: future::Future, C: ?Sized + ReferenceContext + Unpin>
where
    RefContextTarget<C>: Write<<RefContextTarget<C> as Dispatch<OutputNotification<C, T>>>::Handle>
        + Notify<T::Output>,
{
    state: ErasedFutureUnravelState<T, C>,
    context: C::Context,
}

#[derive(Debug, Error)]
#[bounds(
    where
        T: Error + 'static,
        I: Error + 'static,
        U: Error + 'static,
        V: Error + 'static,
        W: Error + 'static,
        Z: Error + 'static,
)]
pub enum FutureUnravelError<Z, T, I, U, V, W> {
    #[error("failed to fork context ref for erased future: {0}")]
    Contextualize(#[source] Z),
    #[error("failed to write handle for erased future: {0}")]
    Transport(#[source] T),
    #[error("failed to create notification wrapper for erased future content: {0}")]
    Notify(#[source] I),
    #[error("failed to fork erased future content: {0}")]
    Dispatch(#[source] U),
    #[error("failed to target erased future content: {0}")]
    Target(#[source] V),
    #[error("failed to finalize erased future content: {0}")]
    Finalize(#[source] W),
}

type UnravelError<T, C> = FutureUnravelError<
    <ForkContextRef<T, C> as Future<C>>::Error,
    <RefContextTarget<C> as Write<
        <RefContextTarget<C> as Dispatch<
            <RefContextTarget<C> as Notify<<T as future::Future>::Output>>::Notification,
        >>::Handle,
    >>::Error,
    <<RefContextTarget<C> as Notify<<T as future::Future>::Output>>::Wrap as Future<
        RefContextTarget<C>,
    >>::Error,
    <<RefContextTarget<C> as Fork<
        <RefContextTarget<C> as Notify<<T as future::Future>::Output>>::Notification,
    >>::Future as Future<RefContextTarget<C>>>::Error,
    <<RefContextTarget<C> as Fork<
        <RefContextTarget<C> as Notify<<T as future::Future>::Output>>::Notification,
    >>::Target as Future<RefContextTarget<C>>>::Error,
    <<RefContextTarget<C> as Fork<
        <RefContextTarget<C> as Notify<<T as future::Future>::Output>>::Notification,
    >>::Finalize as Future<RefContextTarget<C>>>::Error,
>;

impl<T: future::Future, C: ?Sized + ReferenceContext + Unpin> Unpin for ErasedFutureUnravel<T, C> where
    RefContextTarget<C>: Write<<RefContextTarget<C> as Dispatch<OutputNotification<C, T>>>::Handle>
        + Notify<T::Output>
{
}

impl<T: future::Future, C: ?Sized + ReferenceContext + Write<<C as Contextualize>::Handle> + Unpin>
    Future<C> for ErasedFutureUnravel<T, C>
where
    RefContextTarget<C>: Write<<RefContextTarget<C> as Dispatch<OutputNotification<C, T>>>::Handle>
        + Notify<T::Output>,
    C::ForkOutput: Unpin,
    <RefContextTarget<C> as Fork<OutputNotification<C, T>>>::Finalize: Unpin,
    <RefContextTarget<C> as Fork<OutputNotification<C, T>>>::Future: Unpin,
    RefContextTarget<C>: Unpin,
    <RefContextTarget<C> as Fork<OutputNotification<C, T>>>::Target: Unpin,
    T: Unpin,
    <RefContextTarget<C> as Notify<T::Output>>::Wrap: Unpin,
{
    type Ok = ();
    type Error = UnravelError<T, C>;

    fn poll<R: BorrowMut<C>>(
        mut self: Pin<&mut Self>,
        cx: &mut Context,
        ctx: R,
    ) -> Poll<Result<Self::Ok, Self::Error>> {
        use ErasedFutureUnravelState::*;

        let this = &mut *self;

        let ctx = this.context.with(ctx);

        loop {
            match &mut this.state {
                Future(future) => {
                    let item = ready!(Pin::new(future).poll(cx));
                    this.state = Wrap(ctx.wrap(item));
                }
                Wrap(future) => {
                    let item = ready!(Pin::new(future).poll(cx, &mut *ctx))
                        .map_err(FutureUnravelError::Notify)?;
                    this.state = Fork(ctx.fork(item));
                }
                Fork(future) => {
                    let (target, handle) = ready!(Pin::new(&mut *future).poll(cx, &mut *ctx))
                        .map_err(FutureUnravelError::Dispatch)?;
                    this.state = Write(handle, target);
                }
                Write(_, _) => {
                    let mut ctx = Pin::new(&mut *ctx);
                    ready!(ctx.as_mut().poll_ready(cx)).map_err(FutureUnravelError::Transport)?;
                    let data = replace(&mut this.state, Done);
                    if let Write(data, target) = data {
                        ctx.write(data).map_err(FutureUnravelError::Transport)?;
                        this.state = Flush(target);
                    } else {
                        panic!("invalid state in FutureUnravel Write")
                    }
                }
                Flush(_) => {
                    ready!(Pin::new(&mut *ctx).poll_ready(cx))
                        .map_err(FutureUnravelError::Transport)?;
                    let data = replace(&mut this.state, Done);
                    if let Flush(target) = data {
                        this.state = Target(target);
                    } else {
                        panic!("invalid state in FutureUnravel Write")
                    }
                }
                Target(target) => {
                    let finalize = ready!(Pin::new(target).poll(cx, &mut *ctx))
                        .map_err(FutureUnravelError::Target)?;
                    this.state = Finalize(finalize);
                }
                Finalize(finalize) => {
                    ready!(Pin::new(finalize).poll(cx, &mut *ctx))
                        .map_err(FutureUnravelError::Finalize)?;
                    this.state = Done;
                    return Poll::Ready(Ok(()));
                }
                Done => panic!("FutureUnravel polled after completion"),
            }
        }
    }
}

pub enum ErasedFutureCoalesceState<T, C: Notify<T>> {
    Read,
    Join(<C as Join<Notification<C, T>>>::Future),
    Unwrap(<C as Notify<T>>::Unwrap),
    Done,
}

pub struct ErasedFutureCoalesce<T, C: Notify<T>> {
    state: ErasedFutureCoalesceState<T, C>,
    context: C,
}

#[derive(Debug, Error)]
#[bounds(
    where
        T: Error + 'static,
        E: Error + 'static,
        S: Error + 'static,
)]
pub enum ErasedFutureCoalesceError<T, E, S> {
    #[error("failed to read handle for erased future: {0}")]
    Transport(T),
    #[error("failed to join erased future content: {0}")]
    Dispatch(E),
    #[error("failed to unwrap erased future content: {0}")]
    Notify(S),
}

impl<C: Unpin + Notify<T> + Read<<C as Dispatch<Notification<C, T>>>::Handle>, T> future::Future
    for ErasedFutureCoalesce<T, C>
where
    <C as Join<Notification<C, T>>>::Future: Unpin,
    C::Unwrap: Unpin,
{
    type Output = Result<
        T,
        ErasedFutureCoalesceError<
            C::Error,
            <<C as Join<Notification<C, T>>>::Future as Future<C>>::Error,
            <<C as Notify<T>>::Unwrap as Future<C>>::Error,
        >,
    >;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        use ErasedFutureCoalesceState::*;

        let this = &mut *self;

        let ctx = this.context.borrow_mut();

        loop {
            match &mut this.state {
                Read => {
                    let mut ctx = Pin::new(&mut *ctx);
                    let handle = ready!(ctx.as_mut().read(cx))
                        .map_err(ErasedFutureCoalesceError::Transport)?;
                    this.state = Join(ctx.join(handle));
                }
                Unwrap(future) => {
                    let item = ready!(Pin::new(future).poll(cx, &mut *ctx))
                        .map_err(ErasedFutureCoalesceError::Notify)?;
                    this.state = Done;
                    return Poll::Ready(Ok(item));
                }
                Join(future) => {
                    let notification = ready!(Pin::new(future).poll(cx, &mut *ctx))
                        .map_err(ErasedFutureCoalesceError::Dispatch)?;
                    this.state = Unwrap(ctx.unwrap(notification));
                }
                Done => panic!("FutureUnravel polled after completion"),
            }
        }
    }
}

macro_rules! marker_variants {
    ($(
        $($marker:ident)*
    ),+) => {
        $(
            impl<'a, T: Unpin + FromError<ProtocolError> + 'a, C: ?Sized + Read<<C as Contextualize>::Handle> + CloneContext + 'a $(+ $marker)*> Coalesce<C> for Pin<Box<dyn future::Future<Output = T> + 'a $(+ $marker)*>>
            where
                C::JoinOutput: Unpin,
                C: Unpin,
                C::Context: Unpin + Read<<C::Context as Dispatch<Notification<C::Context, T>>>::Handle> + Notify<T> + 'a $(+ $marker)*,
                <C::Context as Join<Notification<C::Context, T>>>::Future: Unpin + 'a $(+ $marker)*,
                <C::Context as Notify<T>>::Unwrap: Unpin + 'a $(+ $marker)*,
                <C::Context as Read<<C::Context as Dispatch<Notification<C::Context, T>>>::Handle>>::Error: Error + 'static + Send,
                <<C::Context as Join<Notification<C::Context, T>>>::Future as Future<C::Context>>::Error: Error + 'static + Send,
                <<C::Context as Notify<T>>::Unwrap as Future<C::Context>>::Error: Error + 'static + Send
            {
                type Future = JoinContextOwned<Self, C>;

                fn coalesce() -> Self::Future {
                    JoinContextOwned::new(|context| {
                        Box::pin(ErasedFutureCoalesce {
                            context,
                            state: ErasedFutureCoalesceState::Read
                        }.unwrap_or_else(|e| T::from_error(ProtocolError(Box::new(e)))))
                    })
                }
            }

            impl<'a, T, C: ?Sized + Write<<C as Contextualize>::Handle> + ReferenceContext + Unpin> Unravel<C>
                for Pin<Box<dyn future::Future<Output = T> + 'a $(+ $marker)*>>
            where
                RefContextTarget<C>: Write<<RefContextTarget<C> as Dispatch<Notification<RefContextTarget<C>, T>>>::Handle>
                    + Notify<T>,
                T: Unpin,
                C::ForkOutput: Unpin,
                <RefContextTarget<C> as Fork<
                    Notification<RefContextTarget<C>, T>,
                >>::Finalize: Unpin,
                <RefContextTarget<C> as Fork<
                    Notification<RefContextTarget<C>, T>,
                >>::Future: Unpin,
                RefContextTarget<C>: Unpin,
                <RefContextTarget<C> as Fork<
                    Notification<RefContextTarget<C>, T>,
                >>::Target: Unpin,
                T: Unpin,
                <RefContextTarget<C> as Notify<T>>::Wrap: Unpin,
            {
                type Finalize = ErasedFutureUnravel<Self, C>;
                type Target = MapErr<ForkContextRef<Self, C>, fn(<ForkContextRef<Self, C> as Future<C>>::Error) -> UnravelError<Self, C>>;

                fn unravel(self) -> Self::Target {
                    ForkContextRef::new(self, |state, context| ErasedFutureUnravel {
                        context,
                        state: ErasedFutureUnravelState::Future(state)
                    }).map_err(FutureUnravelError::Contextualize)
                }
            }

            impl<
                    'a,
                    E,
                    T: future::Future<Output = Result<Self, E>> + 'a $(+ $marker)*,
                    U: FromError<E> $(+ $marker)*,
                > Flatten<E, T> for Pin<Box<dyn future::Future<Output = U> + 'a $(+ $marker)*>>
            {
                fn flatten(future: T) -> Self {
                    Box::pin(
                        future
                            .map(|out| match out {
                                Err(e) => Either::Left(ready(U::from_error(e))),
                                Ok(item) => Either::Right(item),
                            })
                            .flatten(),
                    )
                }
            }

            impl<'a, E, U: FromError<E> + 'a $(+ $marker)*> FromError<E> for Pin<Box<dyn future::Future<Output = U> + 'a $(+ $marker)*>> {
                fn from_error(error: E) -> Self {
                    Box::pin(ready(U::from_error(error)))
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
