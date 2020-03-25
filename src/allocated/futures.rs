use super::{Flatten, FromError, ProtocolError};
use crate::{
    CloneContext, Coalesce, ContextReference, Contextualize, Dispatch, Fork, Future, Join, Notify,
    Read, ReferenceContext, Unravel, Write,
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
    P: Fn(ErasedFutureCoalesce<T, C::Context>) -> O,
    T: Unpin,
    C: ?Sized + CloneContext,
> where
    C::Context: Unpin
        + Read<<C::Context as Dispatch<<C::Context as Notify<T>>::Notification>>::Handle>
        + Notify<T>,
    <C::Context as Join<<C::Context as Notify<T>>::Notification>>::Future: Unpin,
    <C::Context as Notify<T>>::Unwrap: Unpin,
{
    conv: P,
    lifetime: PhantomData<&'a (O, T)>,
    state: FutureCoalesceState<C::JoinOutput>,
}

enum FutureUnravelState<
    T,
    C: ?Sized + Write<<C as Contextualize>::Handle> + ReferenceContext + Unpin,
> where
    <C::Context as ContextReference<C>>::Target: Write<
            <<C::Context as ContextReference<C>>::Target as Dispatch<
                <<C::Context as ContextReference<C>>::Target as Notify<T>>::Notification,
            >>::Handle,
        > + Notify<T>,
{
    None(PhantomData<T>),
    Context(C::ForkOutput),
    Write(C::Context, C::Handle),
    Flush(C::Context),
}

pub struct FutureUnravel<
    T: future::Future,
    C: ?Sized + Write<<C as Contextualize>::Handle> + ReferenceContext + Unpin,
> where
    <C::Context as ContextReference<C>>::Target: Write<
            <<C::Context as ContextReference<C>>::Target as Dispatch<
                <<C::Context as ContextReference<C>>::Target as Notify<T::Output>>::Notification,
            >>::Handle,
        > + Notify<T::Output>,
{
    future: Option<T>,
    context: FutureUnravelState<T::Output, C>,
}

pub enum ErasedFutureUnravelState<T: future::Future, C: ?Sized + ReferenceContext + Unpin>
where
    <C::Context as ContextReference<C>>::Target: Write<
            <<C::Context as ContextReference<C>>::Target as Dispatch<
                <<C::Context as ContextReference<C>>::Target as Notify<T::Output>>::Notification,
            >>::Handle,
        > + Notify<T::Output>,
{
    Future(T),
    Wrap(<<C::Context as ContextReference<C>>::Target as Notify<T::Output>>::Wrap),
    Fork(
        <<C::Context as ContextReference<C>>::Target as Fork<
            <<C::Context as ContextReference<C>>::Target as Notify<T::Output>>::Notification,
        >>::Future,
    ),
    Write(
        <<C::Context as ContextReference<C>>::Target as Dispatch<
            <<C::Context as ContextReference<C>>::Target as Notify<T::Output>>::Notification,
        >>::Handle,
        <<C::Context as ContextReference<C>>::Target as Fork<
            <<C::Context as ContextReference<C>>::Target as Notify<T::Output>>::Notification,
        >>::Target,
    ),
    Flush(
        <<C::Context as ContextReference<C>>::Target as Fork<
            <<C::Context as ContextReference<C>>::Target as Notify<T::Output>>::Notification,
        >>::Target,
    ),
    Target(
        <<C::Context as ContextReference<C>>::Target as Fork<
            <<C::Context as ContextReference<C>>::Target as Notify<T::Output>>::Notification,
        >>::Target,
    ),
    Finalize(
        <<C::Context as ContextReference<C>>::Target as Fork<
            <<C::Context as ContextReference<C>>::Target as Notify<T::Output>>::Notification,
        >>::Finalize,
    ),
    Done,
}

pub struct ErasedFutureUnravel<T: future::Future, C: ?Sized + ReferenceContext + Unpin>
where
    <C::Context as ContextReference<C>>::Target: Write<
            <<C::Context as ContextReference<C>>::Target as Dispatch<
                <<C::Context as ContextReference<C>>::Target as Notify<T::Output>>::Notification,
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

impl<T: future::Future, C: ?Sized + Write<<C as Contextualize>::Handle> + ReferenceContext + Unpin> Unpin
    for FutureUnravel<T, C>
where
    <C::Context as ContextReference<C>>::Target: Write<
            <<C::Context as ContextReference<C>>::Target as Dispatch<
                <<C::Context as ContextReference<C>>::Target as Notify<T::Output>>::Notification,
            >>::Handle,
        > + Notify<T::Output>,
{
}

impl<T: future::Future, C: ?Sized + ReferenceContext + Unpin> Unpin for ErasedFutureUnravel<T, C> where
    <C::Context as ContextReference<C>>::Target: Write<
            <<C::Context as ContextReference<C>>::Target as Dispatch<
                <<C::Context as ContextReference<C>>::Target as Notify<T::Output>>::Notification,
            >>::Handle,
        > + Notify<T::Output>
{
}

impl<T: future::Future, C: ?Sized + Write<<C as Contextualize>::Handle> + ReferenceContext + Unpin>
    Future<C> for FutureUnravel<T, C>
where
    <C::Context as ContextReference<C>>::Target: Write<
            <<C::Context as ContextReference<C>>::Target as Dispatch<
                <<C::Context as ContextReference<C>>::Target as Notify<T::Output>>::Notification,
            >>::Handle,
        > + Notify<T::Output>,
    C::ForkOutput: Unpin,
{
    type Ok = ErasedFutureUnravel<T, C>;
    type Error = FutureUnravelError<
        <C::ForkOutput as Future<C>>::Error,
        C::Error,
        <<C::Context as ContextReference<C>>::Target as Write<
            <<C::Context as ContextReference<C>>::Target as Dispatch<
                <<C::Context as ContextReference<C>>::Target as Notify<T::Output>>::Notification,
            >>::Handle,
        >>::Error,
        <<<C::Context as ContextReference<C>>::Target as Notify<T::Output>>::Wrap as Future<
            <C::Context as ContextReference<C>>::Target,
        >>::Error,
        <<<C::Context as ContextReference<C>>::Target as Fork<
            <<C::Context as ContextReference<C>>::Target as Notify<T::Output>>::Notification,
        >>::Future as Future<<C::Context as ContextReference<C>>::Target>>::Error,
        <<<C::Context as ContextReference<C>>::Target as Fork<
            <<C::Context as ContextReference<C>>::Target as Notify<T::Output>>::Notification,
        >>::Target as Future<<C::Context as ContextReference<C>>::Target>>::Error,
        <<<C::Context as ContextReference<C>>::Target as Fork<
            <<C::Context as ContextReference<C>>::Target as Notify<T::Output>>::Notification,
        >>::Finalize as Future<<C::Context as ContextReference<C>>::Target>>::Error,
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
                        this.context = FutureUnravelState::Context(ctx.borrow_mut().fork_ref())
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
                        let mut ctx = Pin::new(ctx.borrow_mut());
                        ready!(ctx.as_mut().poll_flush(cx)).map_err(FutureUnravelError::Write)?;
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

impl<T: future::Future, C: ?Sized + ReferenceContext + Write<<C as Contextualize>::Handle> + Unpin>
    Future<C> for ErasedFutureUnravel<T, C>
where
    <C::Context as ContextReference<C>>::Target: Write<
            <<C::Context as ContextReference<C>>::Target as Dispatch<
                <<C::Context as ContextReference<C>>::Target as Notify<T::Output>>::Notification,
            >>::Handle,
        > + Notify<T::Output>,
    C::ForkOutput: Unpin,
    <<C::Context as ContextReference<C>>::Target as Fork<
        <<C::Context as ContextReference<C>>::Target as Notify<T::Output>>::Notification,
    >>::Finalize: Unpin,
    <<C::Context as ContextReference<C>>::Target as Fork<
        <<C::Context as ContextReference<C>>::Target as Notify<T::Output>>::Notification,
    >>::Future: Unpin,
    <C::Context as ContextReference<C>>::Target: Unpin,
    <<C::Context as ContextReference<C>>::Target as Fork<
        <<C::Context as ContextReference<C>>::Target as Notify<T::Output>>::Notification,
    >>::Target: Unpin,
    T: Unpin,
    <<C::Context as ContextReference<C>>::Target as Notify<T::Output>>::Wrap: Unpin,
{
    type Ok = ();
    type Error = FutureUnravelError<
        <C::ForkOutput as Future<C>>::Error,
        C::Error,
        <<C::Context as ContextReference<C>>::Target as Write<
            <<C::Context as ContextReference<C>>::Target as Dispatch<
                <<C::Context as ContextReference<C>>::Target as Notify<T::Output>>::Notification,
            >>::Handle,
        >>::Error,
        <<<C::Context as ContextReference<C>>::Target as Notify<T::Output>>::Wrap as Future<
            <C::Context as ContextReference<C>>::Target,
        >>::Error,
        <<<C::Context as ContextReference<C>>::Target as Fork<
            <<C::Context as ContextReference<C>>::Target as Notify<T::Output>>::Notification,
        >>::Future as Future<<C::Context as ContextReference<C>>::Target>>::Error,
        <<<C::Context as ContextReference<C>>::Target as Fork<
            <<C::Context as ContextReference<C>>::Target as Notify<T::Output>>::Notification,
        >>::Target as Future<<C::Context as ContextReference<C>>::Target>>::Error,
        <<<C::Context as ContextReference<C>>::Target as Fork<
            <<C::Context as ContextReference<C>>::Target as Notify<T::Output>>::Notification,
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

pub enum ErasedFutureCoalesceState<T, C: Notify<T>> {
    Read,
    Join(<C as Join<<C as Notify<T>>::Notification>>::Future),
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

impl<C: Unpin + Notify<T> + Read<<C as Dispatch<<C as Notify<T>>::Notification>>::Handle>, T> future::Future
    for ErasedFutureCoalesce<T, C>
where
    <C as Join<<C as Notify<T>>::Notification>>::Future: Unpin,
    C::Unwrap: Unpin,
{
    type Output = Result<
        T,
        ErasedFutureCoalesceError<
            C::Error,
            <<C as Join<<C as Notify<T>>::Notification>>::Future as Future<C>>::Error,
            <<C as Notify<T>>::Unwrap as Future<C>>::Error,
        >,
    >;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        let this = &mut *self;

        let ctx = this.context.borrow_mut();

        loop {
            match &mut this.state {
                ErasedFutureCoalesceState::Read => {
                    let mut ctx = Pin::new(&mut *ctx);
                    let handle = ready!(ctx.as_mut().read(cx))
                        .map_err(ErasedFutureCoalesceError::Transport)?;
                    this.state = ErasedFutureCoalesceState::Join(ctx.join(handle));
                }
                ErasedFutureCoalesceState::Unwrap(future) => {
                    let item = ready!(Pin::new(future).poll(cx, &mut *ctx))
                        .map_err(ErasedFutureCoalesceError::Notify)?;
                    this.state = ErasedFutureCoalesceState::Done;
                    return Poll::Ready(Ok(item));
                }
                ErasedFutureCoalesceState::Join(future) => {
                    let notification = ready!(Pin::new(future).poll(cx, &mut *ctx))
                        .map_err(ErasedFutureCoalesceError::Dispatch)?;
                    this.state = ErasedFutureCoalesceState::Unwrap(ctx.unwrap(notification));
                }
                ErasedFutureCoalesceState::Done => panic!("FutureUnravel polled after completion"),
            }
        }
    }
}

impl<
    'a,
    O,
    P: Fn(ErasedFutureCoalesce<T, C::Context>) -> O,
    T: Unpin,
    C: ?Sized + Read<<C as Contextualize>::Handle> + CloneContext,
> Future<C> for FutureCoalesce<'a, O, P, T, C>
where
    C::JoinOutput: Unpin,
    C: Unpin,
    C::Context: Unpin,
    P: Unpin,
    <C::Context as Notify<T>>::Unwrap: Unpin,
    C::Context: Unpin
        + Read<<C::Context as Dispatch<<C::Context as Notify<T>>::Notification>>::Handle>
        + Notify<T>,
    <C::Context as Join<<C::Context as Notify<T>>::Notification>>::Future: Unpin,
{
    type Ok = O;
    type Error = FutureCoalesceError<<C::JoinOutput as Future<C>>::Error, C::Error>;

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
                    this.state = FutureCoalesceState::Contextualize(ctx.join_owned(handle));
                }
                FutureCoalesceState::Contextualize(future) => {
                    let context = ready!(Pin::new(future).poll(cx, &mut *ctx))
                        .map_err(FutureCoalesceError::Contextualize)?;
                    replace(&mut this.state, FutureCoalesceState::Done);
                    return Poll::Ready(Ok((this.conv)(ErasedFutureCoalesce {
                        context,
                        state: ErasedFutureCoalesceState::Read,
                    })));
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
            impl<'a, T: Unpin + FromError<ProtocolError> + 'a, C: Read<<C as Contextualize>::Handle> + CloneContext + 'a $(+ $marker)*> Coalesce<C> for Pin<Box<dyn future::Future<Output = T> + 'a $(+ $marker)*>>
            where
                C::JoinOutput: Unpin,
                C: Unpin,
                C::Context: Unpin + Read<<C::Context as Dispatch<<C::Context as Notify<T>>::Notification>>::Handle> + Notify<T> + 'a $(+ $marker)*,
                <C::Context as Join<<C::Context as Notify<T>>::Notification>>::Future: Unpin + 'a $(+ $marker)*,
                <C::Context as Notify<T>>::Unwrap: Unpin + 'a $(+ $marker)*,
                <C::Context as Read<<C::Context as Dispatch<<C::Context as Notify<T>>::Notification>>::Handle>>::Error: Error + 'static + Send,
                <<C::Context as Join<<C::Context as Notify<T>>::Notification>>::Future as Future<C::Context>>::Error: Error + 'static + Send,
                <<C::Context as Notify<T>>::Unwrap as Future<C::Context>>::Error: Error + 'static + Send
            {
                type Future = FutureCoalesce<'a, Self, fn(ErasedFutureCoalesce<T, C::Context>) -> Self, T, C>;

                fn coalesce() -> Self::Future {
                    fn conv<'a, T: Unpin + FromError<ProtocolError> + 'a, C: CloneContext + 'a $(+ $marker)*>(
                        fut: ErasedFutureCoalesce<T, C::Context>,
                    ) -> Pin<Box<dyn future::Future<Output = T> + 'a $(+ $marker)*>>
                    where
                        C::Context: Unpin + Read<<C::Context as Dispatch<<C::Context as Notify<T>>::Notification>>::Handle> + Notify<T> + 'a $(+ $marker)*,
                        <C::Context as Join<<C::Context as Notify<T>>::Notification>>::Future: Unpin + 'a $(+ $marker)*,
                        <C::Context as Notify<T>>::Unwrap: Unpin + 'a $(+ $marker)*,
                        <C::Context as Read<<C::Context as Dispatch<<C::Context as Notify<T>>::Notification>>::Handle>>::Error: Error + 'static + Send,
                        <<C::Context as Join<<C::Context as Notify<T>>::Notification>>::Future as Future<C::Context>>::Error: Error + 'static + Send,
                        <<C::Context as Notify<T>>::Unwrap as Future<C::Context>>::Error: Error + 'static + Send
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

            impl<'a, T, C: ?Sized + Write<<C as Contextualize>::Handle> + ReferenceContext + Unpin> Unravel<C>
                for Pin<Box<dyn future::Future<Output = T> + 'a $(+ $marker)*>>
            where
                <C::Context as ContextReference<C>>::Target: Write<<<C::Context as ContextReference<C>>::Target as Dispatch<<<C::Context as ContextReference<C>>::Target as Notify<T>>::Notification>>::Handle>
                    + Notify<T>,
                T: Unpin,
                C::ForkOutput: Unpin,
                <<C::Context as ContextReference<C>>::Target as Fork<
                    <<C::Context as ContextReference<C>>::Target as Notify<T>>::Notification,
                >>::Finalize: Unpin,
                <<C::Context as ContextReference<C>>::Target as Fork<
                    <<C::Context as ContextReference<C>>::Target as Notify<T>>::Notification,
                >>::Future: Unpin,
                <C::Context as ContextReference<C>>::Target: Unpin,
                <<C::Context as ContextReference<C>>::Target as Fork<
                    <<C::Context as ContextReference<C>>::Target as Notify<T>>::Notification,
                >>::Target: Unpin,
                T: Unpin,
                <<C::Context as ContextReference<C>>::Target as Notify<T>>::Wrap: Unpin,
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
    Send, Sync Send
}
