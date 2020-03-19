use super::{Flatten, FromError, ProtocolError};
use crate::{
    Coalesce, CoalesceContextualizer, ContextualizeCoalesce, ContextualizeUnravel, Contextualizer,
    Dispatch, Fork, Future, Join, Notify, Read, Unravel, UnravelContext, Write,
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
use futures::{
    future::{ready, Either},
    ready, FutureExt, TryFutureExt,
};
use thiserror::Error;

pub enum FutureCoalesceState<T> {
    Read,
    Contextualize(T),
    Done,
}

pub struct FutureCoalesce<
    'a,
    O,
    P: Fn(
        <C as ContextualizeCoalesce<
            ErasedFutureCoalesce<T, <C as CoalesceContextualizer>::Target>,
        >>::Future,
    ) -> O,
    T: Unpin,
    C: ?Sized + ContextualizeCoalesce<ErasedFutureCoalesce<T, <C as CoalesceContextualizer>::Target>>,
> where
    C::Target: Unpin
        + Read<<C::Target as Dispatch<<C::Target as Notify<T>>::Notification>>::Handle>
        + Notify<T>,
    <C::Target as Join<<C::Target as Notify<T>>::Notification>>::Future: Unpin,
    <C::Target as Notify<T>>::Unwrap: Unpin,
{
    conv: P,
    lifetime: PhantomData<&'a ()>,
    state: FutureCoalesceState<C::Output>,
}

enum FutureUnravelState<
    T,
    C: ?Sized + Write<<C as Contextualizer>::Handle> + ContextualizeUnravel + Unpin,
> where
    <C::Context as UnravelContext<C>>::Target: Write<
            <<C::Context as UnravelContext<C>>::Target as Dispatch<
                <<C::Context as UnravelContext<C>>::Target as Notify<T>>::Notification,
            >>::Handle,
        > + Notify<T>,
{
    None(PhantomData<T>),
    Context(C::Output),
    Write(C::Context, C::Handle),
    Flush(C::Context),
}

pub struct FutureUnravel<
    T: future::Future,
    C: ?Sized + Write<<C as Contextualizer>::Handle> + ContextualizeUnravel + Unpin,
> where
    <C::Context as UnravelContext<C>>::Target: Write<
            <<C::Context as UnravelContext<C>>::Target as Dispatch<
                <<C::Context as UnravelContext<C>>::Target as Notify<T::Output>>::Notification,
            >>::Handle,
        > + Notify<T::Output>,
{
    future: Option<T>,
    context: FutureUnravelState<T::Output, C>,
}

pub enum ErasedFutureUnravelState<T: future::Future, C: ?Sized + ContextualizeUnravel + Unpin>
where
    <C::Context as UnravelContext<C>>::Target: Write<
            <<C::Context as UnravelContext<C>>::Target as Dispatch<
                <<C::Context as UnravelContext<C>>::Target as Notify<T::Output>>::Notification,
            >>::Handle,
        > + Notify<T::Output>,
{
    Future(T),
    Wrap(<<C::Context as UnravelContext<C>>::Target as Notify<T::Output>>::Wrap),
    Fork(
        <<C::Context as UnravelContext<C>>::Target as Fork<
            <<C::Context as UnravelContext<C>>::Target as Notify<T::Output>>::Notification,
        >>::Future,
    ),
    Write(
        <<C::Context as UnravelContext<C>>::Target as Dispatch<
            <<C::Context as UnravelContext<C>>::Target as Notify<T::Output>>::Notification,
        >>::Handle,
        <<C::Context as UnravelContext<C>>::Target as Fork<
            <<C::Context as UnravelContext<C>>::Target as Notify<T::Output>>::Notification,
        >>::Target,
    ),
    Flush(
        <<C::Context as UnravelContext<C>>::Target as Fork<
            <<C::Context as UnravelContext<C>>::Target as Notify<T::Output>>::Notification,
        >>::Target,
    ),
    Target(
        <<C::Context as UnravelContext<C>>::Target as Fork<
            <<C::Context as UnravelContext<C>>::Target as Notify<T::Output>>::Notification,
        >>::Target,
    ),
    Finalize(
        <<C::Context as UnravelContext<C>>::Target as Fork<
            <<C::Context as UnravelContext<C>>::Target as Notify<T::Output>>::Notification,
        >>::Finalize,
    ),
    Done,
}

pub struct ErasedFutureUnravel<T: future::Future, C: ?Sized + ContextualizeUnravel + Unpin>
where
    <C::Context as UnravelContext<C>>::Target: Write<
            <<C::Context as UnravelContext<C>>::Target as Dispatch<
                <<C::Context as UnravelContext<C>>::Target as Notify<T::Output>>::Notification,
            >>::Handle,
        > + Notify<T::Output>,
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
        K: Error + 'static,
)]
pub enum FutureUnravelError<Z, K, T, I, U, V, W> {
    #[error("failed to contextualize erased future: {0}")]
    Contextualize(#[source] Z),
    #[error("failed to write context handle for erased future: {0}")]
    Write(#[source] K),
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

impl<
        T: future::Future,
        C: ?Sized + Write<<C as Contextualizer>::Handle> + ContextualizeUnravel + Unpin,
    > Unpin for FutureUnravel<T, C>
where
    <C::Context as UnravelContext<C>>::Target: Write<
            <<C::Context as UnravelContext<C>>::Target as Dispatch<
                <<C::Context as UnravelContext<C>>::Target as Notify<T::Output>>::Notification,
            >>::Handle,
        > + Notify<T::Output>,
{
}

impl<T: future::Future, C: ?Sized + ContextualizeUnravel + Unpin> Unpin
    for ErasedFutureUnravel<T, C>
where
    <C::Context as UnravelContext<C>>::Target: Write<
            <<C::Context as UnravelContext<C>>::Target as Dispatch<
                <<C::Context as UnravelContext<C>>::Target as Notify<T::Output>>::Notification,
            >>::Handle,
        > + Notify<T::Output>,
{
}

impl<
        T: future::Future,
        C: ?Sized + Write<<C as Contextualizer>::Handle> + ContextualizeUnravel + Unpin,
    > Future<C> for FutureUnravel<T, C>
where
    <C::Context as UnravelContext<C>>::Target: Write<
            <<C::Context as UnravelContext<C>>::Target as Dispatch<
                <<C::Context as UnravelContext<C>>::Target as Notify<T::Output>>::Notification,
            >>::Handle,
        > + Notify<T::Output>,
    C::Output: Unpin,
{
    type Ok = ErasedFutureUnravel<T, C>;
    type Error = FutureUnravelError<
        <C::Output as Future<C>>::Error,
        C::Error,
        <<C::Context as UnravelContext<C>>::Target as Write<
            <<C::Context as UnravelContext<C>>::Target as Dispatch<
                <<C::Context as UnravelContext<C>>::Target as Notify<T::Output>>::Notification,
            >>::Handle,
        >>::Error,
        <<<C::Context as UnravelContext<C>>::Target as Notify<T::Output>>::Wrap as Future<
            <C::Context as UnravelContext<C>>::Target,
        >>::Error,
        <<<C::Context as UnravelContext<C>>::Target as Fork<
            <<C::Context as UnravelContext<C>>::Target as Notify<T::Output>>::Notification,
        >>::Future as Future<<C::Context as UnravelContext<C>>::Target>>::Error,
        <<<C::Context as UnravelContext<C>>::Target as Fork<
            <<C::Context as UnravelContext<C>>::Target as Notify<T::Output>>::Notification,
        >>::Target as Future<<C::Context as UnravelContext<C>>::Target>>::Error,
        <<<C::Context as UnravelContext<C>>::Target as Fork<
            <<C::Context as UnravelContext<C>>::Target as Notify<T::Output>>::Notification,
        >>::Finalize as Future<<C::Context as UnravelContext<C>>::Target>>::Error,
    >;
    fn poll<R: BorrowMut<C>>(
        mut self: Pin<&mut Self>,
        cx: &mut Context,
        mut ctx: R,
    ) -> Poll<Result<Self::Ok, Self::Error>> {
        let this = &mut *self;

        loop {
            if this.future.is_some() {
                match &mut this.context {
                    FutureUnravelState::None(_) => {
                        this.context = FutureUnravelState::Context(ctx.borrow_mut().contextualize())
                    }
                    FutureUnravelState::Context(future) => {
                        let (context, handle) = ready!(Pin::new(future).poll(cx, ctx.borrow_mut()))
                            .map_err(FutureUnravelError::Contextualize)?;
                        this.context = FutureUnravelState::Write(context, handle);
                    }
                    FutureUnravelState::Write(_, _) => {
                        let mut ctx = Pin::new(ctx.borrow_mut());
                        ready!(ctx.as_mut().poll_ready(cx)).map_err(FutureUnravelError::Write)?;
                        let data =
                            replace(&mut this.context, FutureUnravelState::None(PhantomData));
                        if let FutureUnravelState::Write(context, handle) = data {
                            ctx.write(handle).map_err(FutureUnravelError::Write)?;
                            this.context = FutureUnravelState::Flush(context);
                        } else {
                            panic!("invalid state")
                        }
                    }
                    FutureUnravelState::Flush(_) => {
                        let data =
                            replace(&mut this.context, FutureUnravelState::None(PhantomData));
                        if let FutureUnravelState::Flush(context) = data {
                            return Poll::Ready(Ok(ErasedFutureUnravel {
                                context,
                                state: ErasedFutureUnravelState::Future(
                                    this.future.take().unwrap(),
                                ),
                            }));
                        } else {
                            panic!("invalid state")
                        }
                    }
                }
            } else {
                panic!("FutureUnravel polled after completion")
            }
        }
    }
}

impl<
        T: future::Future,
        C: ?Sized + ContextualizeUnravel + Write<<C as Contextualizer>::Handle> + Unpin,
    > Future<C> for ErasedFutureUnravel<T, C>
where
    <C::Context as UnravelContext<C>>::Target: Write<
            <<C::Context as UnravelContext<C>>::Target as Dispatch<
                <<C::Context as UnravelContext<C>>::Target as Notify<T::Output>>::Notification,
            >>::Handle,
        > + Notify<T::Output>,
    C::Output: Unpin,
    <<C::Context as UnravelContext<C>>::Target as Fork<
        <<C::Context as UnravelContext<C>>::Target as Notify<T::Output>>::Notification,
    >>::Finalize: Unpin,
    <<C::Context as UnravelContext<C>>::Target as Fork<
        <<C::Context as UnravelContext<C>>::Target as Notify<T::Output>>::Notification,
    >>::Future: Unpin,
    <C::Context as UnravelContext<C>>::Target: Unpin,
    <<C::Context as UnravelContext<C>>::Target as Fork<
        <<C::Context as UnravelContext<C>>::Target as Notify<T::Output>>::Notification,
    >>::Target: Unpin,
    T: Unpin,
    <<C::Context as UnravelContext<C>>::Target as Notify<T::Output>>::Wrap: Unpin,
{
    type Ok = ();
    type Error = FutureUnravelError<
        <C::Output as Future<C>>::Error,
        C::Error,
        <<C::Context as UnravelContext<C>>::Target as Write<
            <<C::Context as UnravelContext<C>>::Target as Dispatch<
                <<C::Context as UnravelContext<C>>::Target as Notify<T::Output>>::Notification,
            >>::Handle,
        >>::Error,
        <<<C::Context as UnravelContext<C>>::Target as Notify<T::Output>>::Wrap as Future<
            <C::Context as UnravelContext<C>>::Target,
        >>::Error,
        <<<C::Context as UnravelContext<C>>::Target as Fork<
            <<C::Context as UnravelContext<C>>::Target as Notify<T::Output>>::Notification,
        >>::Future as Future<<C::Context as UnravelContext<C>>::Target>>::Error,
        <<<C::Context as UnravelContext<C>>::Target as Fork<
            <<C::Context as UnravelContext<C>>::Target as Notify<T::Output>>::Notification,
        >>::Target as Future<<C::Context as UnravelContext<C>>::Target>>::Error,
        <<<C::Context as UnravelContext<C>>::Target as Fork<
            <<C::Context as UnravelContext<C>>::Target as Notify<T::Output>>::Notification,
        >>::Finalize as Future<<C::Context as UnravelContext<C>>::Target>>::Error,
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
                ErasedFutureUnravelState::Future(future) => {
                    let item = ready!(Pin::new(future).poll(cx));
                    this.state = ErasedFutureUnravelState::Wrap(ctx.wrap(item));
                }
                ErasedFutureUnravelState::Wrap(future) => {
                    let item = ready!(Pin::new(future).poll(cx, &mut *ctx))
                        .map_err(FutureUnravelError::Notify)?;
                    this.state = ErasedFutureUnravelState::Fork(ctx.fork(item));
                }
                ErasedFutureUnravelState::Fork(future) => {
                    let (target, handle) = ready!(Pin::new(&mut *future).poll(cx, &mut *ctx))
                        .map_err(FutureUnravelError::Dispatch)?;
                    this.state = ErasedFutureUnravelState::Write(handle, target);
                }
                ErasedFutureUnravelState::Write(_, _) => {
                    let mut ctx = Pin::new(&mut *ctx);
                    ready!(ctx.as_mut().poll_ready(cx)).map_err(FutureUnravelError::Transport)?;
                    let data = replace(&mut this.state, ErasedFutureUnravelState::Done);
                    if let ErasedFutureUnravelState::Write(data, target) = data {
                        ctx.write(data).map_err(FutureUnravelError::Transport)?;
                        this.state = ErasedFutureUnravelState::Flush(target);
                    } else {
                        panic!("invalid state in FutureUnravel Write")
                    }
                }
                ErasedFutureUnravelState::Flush(_) => {
                    ready!(Pin::new(&mut *ctx).poll_ready(cx))
                        .map_err(FutureUnravelError::Transport)?;
                    let data = replace(&mut this.state, ErasedFutureUnravelState::Done);
                    if let ErasedFutureUnravelState::Flush(target) = data {
                        this.state = ErasedFutureUnravelState::Target(target);
                    } else {
                        panic!("invalid state in FutureUnravel Write")
                    }
                }
                ErasedFutureUnravelState::Target(target) => {
                    let finalize = ready!(Pin::new(target).poll(cx, &mut *ctx))
                        .map_err(FutureUnravelError::Target)?;
                    this.state = ErasedFutureUnravelState::Finalize(finalize);
                }
                ErasedFutureUnravelState::Finalize(finalize) => {
                    ready!(Pin::new(finalize).poll(cx, &mut *ctx))
                        .map_err(FutureUnravelError::Finalize)?;
                    this.state = ErasedFutureUnravelState::Done;
                    return Poll::Ready(Ok(()));
                }
                ErasedFutureUnravelState::Done => panic!("FutureUnravel polled after completion"),
            }
        }
    }
}

pub enum ErasedFutureCoalesce<T, C: ?Sized + Notify<T>> {
    Read,
    Join(<C as Join<<C as Notify<T>>::Notification>>::Future),
    Unwrap(<C as Notify<T>>::Unwrap),
    Done,
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

#[derive(Debug, Error)]
#[bounds(
    where
        T: Error + 'static,
        E: Error + 'static,
)]
pub enum FutureCoalesceError<E, T> {
    #[error("failed to read handle for erased future context: {0}")]
    Read(T),
    #[error("failed to contextualize erased future content: {0}")]
    Contextualize(E),
}

impl<
        C: Unpin + ?Sized + Notify<T> + Read<<C as Dispatch<<C as Notify<T>>::Notification>>::Handle>,
        T,
    > Future<C> for ErasedFutureCoalesce<T, C>
where
    <C as Join<<C as Notify<T>>::Notification>>::Future: Unpin,
    C::Unwrap: Unpin,
{
    type Ok = T;
    type Error = ErasedFutureCoalesceError<
        C::Error,
        <<C as Join<<C as Notify<T>>::Notification>>::Future as Future<C>>::Error,
        <<C as Notify<T>>::Unwrap as Future<C>>::Error,
    >;

    fn poll<R: BorrowMut<C>>(
        mut self: Pin<&mut Self>,
        cx: &mut Context,
        mut ctx: R,
    ) -> Poll<Result<Self::Ok, Self::Error>> {
        let ctx = ctx.borrow_mut();

        loop {
            match &mut *self {
                ErasedFutureCoalesce::Read => {
                    let mut ctx = Pin::new(&mut *ctx);
                    let handle = ready!(ctx.as_mut().read(cx))
                        .map_err(ErasedFutureCoalesceError::Transport)?;
                    replace(&mut *self, ErasedFutureCoalesce::Join(ctx.join(handle)));
                }
                ErasedFutureCoalesce::Unwrap(future) => {
                    let item = ready!(Pin::new(future).poll(cx, &mut *ctx))
                        .map_err(ErasedFutureCoalesceError::Notify)?;
                    replace(&mut *self, ErasedFutureCoalesce::Done);
                    return Poll::Ready(Ok(item));
                }
                ErasedFutureCoalesce::Join(future) => {
                    let notification = ready!(Pin::new(future).poll(cx, &mut *ctx))
                        .map_err(ErasedFutureCoalesceError::Dispatch)?;
                    replace(
                        &mut *self,
                        ErasedFutureCoalesce::Unwrap(ctx.unwrap(notification)),
                    );
                }
                ErasedFutureCoalesce::Done => panic!("FutureUnravel polled after completion"),
            }
        }
    }
}

impl<
        'a,
        O,
        P: Fn(C::Future) -> O,
        T: Unpin,
        C: ?Sized
            + Read<<C as Contextualizer>::Handle>
            + ContextualizeCoalesce<ErasedFutureCoalesce<T, <C as CoalesceContextualizer>::Target>>,
    > Future<C> for FutureCoalesce<'a, O, P, T, C>
where
    C::Output: Unpin,
    C: Unpin,
    P: Unpin,
    <C::Target as Notify<T>>::Unwrap: Unpin,
    C::Future: 'a,
    C::Target: Unpin
        + Read<<C::Target as Dispatch<<C::Target as Notify<T>>::Notification>>::Handle>
        + Notify<T>,
    <C::Target as Join<<C::Target as Notify<T>>::Notification>>::Future: Unpin,
{
    type Ok = O;
    type Error = FutureCoalesceError<<C::Output as Future<C>>::Error, C::Error>;

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
                FutureCoalesceState::Read => {
                    let ct = Pin::new(&mut *ctx);
                    let handle = ready!(ct.read(cx)).map_err(FutureCoalesceError::Read)?;
                    this.state = FutureCoalesceState::Contextualize(
                        ctx.contextualize(handle, ErasedFutureCoalesce::Read),
                    );
                }
                FutureCoalesceState::Contextualize(future) => {
                    let item = ready!(Pin::new(future).poll(cx, &mut *ctx))
                        .map_err(FutureCoalesceError::Contextualize)?;
                    replace(&mut this.state, FutureCoalesceState::Done);
                    return Poll::Ready(Ok((this.conv)(item)));
                }
                FutureCoalesceState::Done => panic!("FutureCoalesce polled after completion"),
            }
        }
    }
}

macro_rules! marker_variants {
    ($(
        $($marker:ident)*
    ),+) => {
        $(
            impl<'a, T: Unpin + FromError<ProtocolError>, C: Read<<C as Contextualizer>::Handle> + ContextualizeCoalesce<ErasedFutureCoalesce<T, <C as CoalesceContextualizer>::Target>>> Coalesce<C> for Pin<Box<dyn future::Future<Output = T> + 'a $(+ $marker)*>>
            where
                C::Output: Unpin,
                C: Unpin,
                C::Future: 'a $(+ $marker)*,
                C::Target: Unpin + Read<<C::Target as Dispatch<<C::Target as Notify<T>>::Notification>>::Handle> + Notify<T>,
                <C::Target as Join<<C::Target as Notify<T>>::Notification>>::Future: Unpin,
                <C::Target as Notify<T>>::Unwrap: Unpin,
                <C::Target as Read<<C::Target as Dispatch<<C::Target as Notify<T>>::Notification>>::Handle>>::Error: Error + 'static,
                <<C::Target as Join<<C::Target as Notify<T>>::Notification>>::Future as Future<C::Target>>::Error: Error + 'static,
                <<C::Target as Notify<T>>::Unwrap as Future<C::Target>>::Error: Error + 'static
            {
                type Future = FutureCoalesce<'a, Self, fn(C::Future) -> Self, T, C>;

                fn coalesce() -> Self::Future {
                    fn conv<'a, T: Unpin + FromError<ProtocolError>, C: ContextualizeCoalesce<ErasedFutureCoalesce<T, <C as CoalesceContextualizer>::Target>>>(
                        fut: C::Future,
                    ) -> Pin<Box<dyn future::Future<Output = T> + 'a $(+ $marker)*>>
                    where
                        C::Future: 'a $(+ $marker)*,
                        C::Target: Unpin + Read<<C::Target as Dispatch<<C::Target as Notify<T>>::Notification>>::Handle> + Notify<T>,
                        <C::Target as Join<<C::Target as Notify<T>>::Notification>>::Future: Unpin,
                        <C::Target as Notify<T>>::Unwrap: Unpin,
                        <C::Target as Read<<C::Target as Dispatch<<C::Target as Notify<T>>::Notification>>::Handle>>::Error: Error + 'static,
                        <<C::Target as Join<<C::Target as Notify<T>>::Notification>>::Future as Future<C::Target>>::Error: Error + 'static,
                        <<C::Target as Notify<T>>::Unwrap as Future<C::Target>>::Error: Error + 'static
                    {
                        Box::pin(fut.unwrap_or_else(|e| T::from_error(ProtocolError(Box::new(e)))))
                    }

                    FutureCoalesce {
                        lifetime: PhantomData,
                        state: FutureCoalesceState::Read,
                        conv: conv::<'a, T, C>,
                    }
                }
            }

            impl<'a, T, C: ?Sized + Write<<C as Contextualizer>::Handle> + ContextualizeUnravel + Unpin> Unravel<C>
                for Pin<Box<dyn future::Future<Output = T> + 'a $(+ $marker)*>>
            where
                <C::Context as UnravelContext<C>>::Target: Write<<<C::Context as UnravelContext<C>>::Target as Dispatch<<<C::Context as UnravelContext<C>>::Target as Notify<T>>::Notification>>::Handle>
                    + Notify<T>,
                T: Unpin,
                C::Output: Unpin,
                <<C::Context as UnravelContext<C>>::Target as Fork<
                    <<C::Context as UnravelContext<C>>::Target as Notify<T>>::Notification,
                >>::Finalize: Unpin,
                <<C::Context as UnravelContext<C>>::Target as Fork<
                    <<C::Context as UnravelContext<C>>::Target as Notify<T>>::Notification,
                >>::Future: Unpin,
                <C::Context as UnravelContext<C>>::Target: Unpin,
                <<C::Context as UnravelContext<C>>::Target as Fork<
                    <<C::Context as UnravelContext<C>>::Target as Notify<T>>::Notification,
                >>::Target: Unpin,
                T: Unpin,
                <<C::Context as UnravelContext<C>>::Target as Notify<T>>::Wrap: Unpin,
            {
                type Finalize = ErasedFutureUnravel<Self, C>;
                type Target = FutureUnravel<Self, C>;

                fn unravel(self) -> Self::Target {
                    FutureUnravel {
                        future: Some(self),
                        context: FutureUnravelState::None(PhantomData)
                    }
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
        )*
    };
}

marker_variants! {
    ,
    Sync,
    Send, Sync Send,
    Unpin, Sync Unpin, Send Unpin, Sync Send Unpin
}

#[cfg(feature = "std")]
mod standard {
    use super::*;
    use std::panic::{RefUnwindSafe, UnwindSafe};

    marker_variants! {
        UnwindSafe, Sync UnwindSafe, Send UnwindSafe, Sync Send UnwindSafe, Unpin UnwindSafe, Sync Unpin UnwindSafe, Send Unpin UnwindSafe, Sync Send Unpin UnwindSafe,
        RefUnwindSafe, Sync RefUnwindSafe, Send RefUnwindSafe, Sync Send RefUnwindSafe, Unpin RefUnwindSafe, Sync Unpin RefUnwindSafe, Send Unpin RefUnwindSafe, Sync Send Unpin RefUnwindSafe, UnwindSafe RefUnwindSafe, Sync UnwindSafe RefUnwindSafe, Send UnwindSafe RefUnwindSafe, Sync Send UnwindSafe RefUnwindSafe, Unpin UnwindSafe RefUnwindSafe, Sync Unpin UnwindSafe RefUnwindSafe, Send Unpin UnwindSafe RefUnwindSafe, Sync Send Unpin UnwindSafe RefUnwindSafe
    }
}
