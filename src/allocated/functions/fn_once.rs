use crate::{
    allocated::{Flatten, ProtocolError},
    future::MapErr,
    CloneContext, Coalesce, ContextReference, Contextualize, Dispatch, Finalize, Fork,
    ForkContextRef, ForkContextRefError, Future, FutureExt, Join, JoinContextOwned, Notify, Read,
    RefContextTarget, ReferenceContext, Unravel, Write,
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
        U: Error + 'static,
        V: Error + 'static,
        W: Error + 'static,
        Z: Error + 'static,
        C: Error + 'static,
)]
pub enum FnOnceUnravelError<A, B, C, Z, T, U, V, W> {
    #[error("failed to read argument handle for erased FnOnce: {0}")]
    Read(#[source] A),
    #[error("failed to join argument for erased FnOnce: {0}")]
    Join(#[source] C),
    #[error("failed to unwrap argument notification for erased FnOnce: {0}")]
    Unwrap(#[source] B),
    #[error("failed to contextualize erased FnOnce: {0}")]
    Contextualize(#[source] Z),
    #[error("failed to write handle for erased FnOnce: {0}")]
    Transport(#[source] T),
    #[error("failed to fork erased FnOnce return: {0}")]
    Dispatch(#[source] U),
    #[error("failed to target erased FnOnce return: {0}")]
    Target(#[source] V),
    #[error("failed to finalize erased FnOnce return: {0}")]
    Finalize(#[source] W),
}

type UnravelError<T, U, C> = FnOnceUnravelError<
    <RefContextTarget<C> as Read<
        <RefContextTarget<C> as Dispatch<<RefContextTarget<C> as Notify<T>>::Notification>>::Handle,
    >>::Error,
    <<RefContextTarget<C> as Notify<T>>::Unwrap as Future<RefContextTarget<C>>>::Error,
    <<RefContextTarget<C> as Join<<RefContextTarget<C> as Notify<T>>::Notification>>::Future as Future<
        RefContextTarget<C>,
    >>::Error,
    ForkContextRefError<
        <<C as ReferenceContext>::ForkOutput as Future<C>>::Error,
        <C as Write<<C as Contextualize>::Handle>>::Error,
    >,
    <RefContextTarget<C> as Write<<RefContextTarget<C> as Dispatch<U>>::Handle>>::Error,
    <<RefContextTarget<C> as Fork<U>>::Future as Future<RefContextTarget<C>>>::Error,
    <<RefContextTarget<C> as Fork<U>>::Target as Future<RefContextTarget<C>>>::Error,
    <<RefContextTarget<C> as Fork<U>>::Finalize as Future<RefContextTarget<C>>>::Error,
>;

pub enum ErasedFnOnceUnravelState<C: ?Sized + ReferenceContext, T, U>
where
    RefContextTarget<C>: Notify<T>
        + Fork<U>
        + Read<
            <RefContextTarget<C> as Dispatch<<RefContextTarget<C> as Notify<T>>::Notification>>::Handle,
        > + Write<<RefContextTarget<C> as Dispatch<U>>::Handle>,
{
    Read,
    Join(<RefContextTarget<C> as Join<<RefContextTarget<C> as Notify<T>>::Notification>>::Future),
    Unwrap(<RefContextTarget<C> as Notify<T>>::Unwrap),
    Fork(<RefContextTarget<C> as Fork<U>>::Future),
    Write(
        <RefContextTarget<C> as Fork<U>>::Target,
        <RefContextTarget<C> as Dispatch<U>>::Handle,
    ),
    Flush(<RefContextTarget<C> as Fork<U>>::Target),
    Target(<RefContextTarget<C> as Fork<U>>::Target),
    Finalize(<RefContextTarget<C> as Fork<U>>::Finalize),
    Done,
}

pub struct ErasedFnOnceUnravel<C: ?Sized + ReferenceContext, T, U, F, M: Fn(F, T) -> U>
where
    RefContextTarget<C>: Notify<T>
        + Fork<U>
        + Read<
            <RefContextTarget<C> as Dispatch<<RefContextTarget<C> as Notify<T>>::Notification>>::Handle,
        > + Write<<RefContextTarget<C> as Dispatch<U>>::Handle>,
{
    state: ErasedFnOnceUnravelState<C, T, U>,
    call: Option<F>,
    conv: M,
    context: C::Context,
}

impl<C: ?Sized + ReferenceContext, T, U, F, M: Fn(F, T) -> U> Unpin
    for ErasedFnOnceUnravel<C, T, U, F, M>
where
    RefContextTarget<C>: Notify<T>
        + Fork<U>
        + Read<
            <RefContextTarget<C> as Dispatch<<RefContextTarget<C> as Notify<T>>::Notification>>::Handle,
        > + Write<<RefContextTarget<C> as Dispatch<U>>::Handle>,
{
}

impl<C: ?Sized + Write<<C as Contextualize>::Handle> + ReferenceContext, T, U, F, M: Fn(F, T) -> U>
    Future<C> for ErasedFnOnceUnravel<C, T, U, F, M>
where
    RefContextTarget<C>: Notify<T>
        + Fork<U>
        + Read<
            <RefContextTarget<C> as Dispatch<<RefContextTarget<C> as Notify<T>>::Notification>>::Handle,
        > + Write<<RefContextTarget<C> as Dispatch<U>>::Handle>,
    U: Unpin,
    <RefContextTarget<C> as Fork<U>>::Target: Unpin,
    <RefContextTarget<C> as Fork<U>>::Future: Unpin,
    <RefContextTarget<C> as Fork<U>>::Finalize: Unpin,
    RefContextTarget<C>: Unpin,
    <RefContextTarget<C> as Notify<T>>::Unwrap: Unpin,
    <RefContextTarget<C> as Join<<RefContextTarget<C> as Notify<T>>::Notification>>::Future: Unpin,
{
    type Ok = ();
    type Error = UnravelError<T, U, C>;

    fn poll<R: BorrowMut<C>>(
        mut self: Pin<&mut Self>,
        cx: &mut Context,
        ctx: R,
    ) -> Poll<Result<Self::Ok, Self::Error>> {
        use ErasedFnOnceUnravelState::*;

        let this = &mut *self;

        let ctx = this.context.with(ctx);

        loop {
            match &mut this.state {
                Read => {
                    let mut ct = Pin::new(&mut *ctx);
                    let handle = ready!(ct.as_mut().read(cx)).map_err(FnOnceUnravelError::Read)?;
                    this.state = Join(crate::Join::<
                        <RefContextTarget<C> as Notify<T>>::Notification,
                    >::join(&mut *ctx, handle));
                }
                Join(future) => {
                    let notification = ready!(Pin::new(future).poll(cx, &mut *ctx))
                        .map_err(FnOnceUnravelError::Join)?;
                    this.state = Unwrap(<RefContextTarget<C> as Notify<T>>::unwrap(
                        &mut *ctx,
                        notification,
                    ));
                }
                Unwrap(future) => {
                    let item = ready!(Pin::new(future).poll(cx, &mut *ctx))
                        .map_err(FnOnceUnravelError::Unwrap)?;
                    this.state = Fork(ctx.fork((this.conv)(this.call.take().unwrap(), item)));
                }
                Fork(future) => {
                    let (target, handle) = ready!(Pin::new(&mut *future).poll(cx, &mut *ctx))
                        .map_err(FnOnceUnravelError::Dispatch)?;
                    this.state = Write(target, handle);
                }
                Write(_, _) => {
                    let mut ctx = Pin::new(&mut *ctx);
                    ready!(ctx.as_mut().poll_ready(cx)).map_err(FnOnceUnravelError::Transport)?;
                    let data = replace(&mut this.state, Done);
                    if let Write(target, data) = data {
                        ctx.write(data).map_err(FnOnceUnravelError::Transport)?;
                        this.state = Flush(target);
                    } else {
                        panic!("invalid state in ErasedFnOnceUnravel Write")
                    }
                }
                Flush(_) => {
                    ready!(Pin::new(&mut *ctx).poll_flush(cx))
                        .map_err(FnOnceUnravelError::Transport)?;
                    let data = replace(&mut this.state, Done);
                    if let Flush(target) = data {
                        this.state = Target(target);
                    } else {
                        panic!("invalid state in ErasedFnOnceUnravel Write")
                    }
                }
                Target(target) => {
                    let finalize = ready!(Pin::new(target).poll(cx, &mut *ctx))
                        .map_err(FnOnceUnravelError::Target)?;
                    this.state = Finalize(finalize);
                }
                Finalize(finalize) => {
                    ready!(Pin::new(finalize).poll(cx, &mut *ctx))
                        .map_err(FnOnceUnravelError::Finalize)?;
                    this.state = Done;
                    return Poll::Ready(Ok(()));
                }
                Done => panic!("ErasedFnOnceUnravel polled after completion"),
            }
        }
    }
}

pub enum ErasedFnOnceCoalesceState<
    T,
    U,
    C: Notify<T>
        + Join<U>
        + Write<<C as Dispatch<<C as Notify<T>>::Notification>>::Handle>
        + Read<<C as Dispatch<U>>::Handle>
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
    Join(<C as Join<U>>::Future),
    Done,
}

pub struct ErasedFnOnceCoalesce<
    T,
    U,
    C: Notify<T>
        + Join<U>
        + Write<<C as Dispatch<<C as Notify<T>>::Notification>>::Handle>
        + Read<<C as Dispatch<U>>::Handle>
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
        + Join<U>
        + Write<<C as Dispatch<<C as Notify<T>>::Notification>>::Handle>
        + Read<<C as Dispatch<U>>::Handle>
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
        U: Error + 'static,
        V: Error + 'static,
        W: Error + 'static,
        C: Error + 'static,
)]
pub enum ErasedFnOnceCoalesceError<A, C, B, T, U, V, W> {
    #[error("failed to write argument handle for erased FnOnce: {0}")]
    Write(#[source] A),
    #[error("failed to fork argument for erased FnOnce: {0}")]
    Fork(#[source] C),
    #[error("failed to wrap argument notification for erased FnOnce: {0}")]
    Wrap(#[source] B),
    #[error("failed to read handle for erased FnOnce return: {0}")]
    Read(#[source] T),
    #[error("failed to create notification wrapper for erased FnOnce return: {0}")]
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
    <C as Read<<C as Dispatch<U>>::Handle>>::Error,
    <<C as Join<U>>::Future as Future<C>>::Error,
    <<C as Fork<<C as Notify<T>>::Notification>>::Target as Future<C>>::Error,
    <<C as Finalize<<C as Fork<<C as Notify<T>>::Notification>>::Finalize>>::Output as Future<C>>::Error,
>;

impl<
    T,
    U,
    C: Notify<T>
        + Join<U>
        + Write<<C as Dispatch<<C as Notify<T>>::Notification>>::Handle>
        + Read<<C as Dispatch<U>>::Handle>
        + Finalize<<C as Fork<<C as Notify<T>>::Notification>>::Finalize>,
> future::Future for ErasedFnOnceCoalesce<T, U, C>
where
    <C as Notify<T>>::Wrap: Unpin,
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
    <C as Read<<C as Dispatch<U>>::Handle>>::Error: Error + Send + 'static,
    <C as Join<U>>::Future: Unpin,
    <<C as Join<U>>::Future as Future<C>>::Error: Error + Send + 'static,
    <<C as Fork<<C as Notify<T>>::Notification>>::Target as Future<C>>::Error:
        Error + Send + 'static,
    <<C as Finalize<<C as Fork<<C as Notify<T>>::Notification>>::Finalize>>::Output as Future<C>>::Error:
        Error + Send + 'static,
{
    type Output = Result<U, ProtocolError>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        use ErasedFnOnceCoalesceState::*;

        let this = &mut *self;
        let ctx = this.context.borrow_mut();

        loop {
            match &mut this.state {
                Wrap(future) => {
                    let wrapped = ready!(Pin::new(future).poll(cx, &mut *ctx))
                        .map_err(|e| ProtocolError::new(CoalesceError::<T, U, C>::Wrap(e)))?;
                    this.state = Fork(ctx.fork(wrapped));
                }
                Fork(future) => {
                    let (target, handle) = ready!(Pin::new(future).poll(cx, &mut *ctx))
                        .map_err(|e| ProtocolError::new(CoalesceError::<T, U, C>::Fork(e)))?;
                    this.state = Write(target, handle);
                }
                Write(_, _) => {
                    let mut ctx = Pin::new(&mut *ctx);
                    ready!(ctx.as_mut().poll_ready(cx))
                        .map_err(|e| ProtocolError::new(CoalesceError::<T, U, C>::Write(e)))?;
                    let data = replace(&mut this.state, Done);
                    if let Write(target, data) = data {
                        ctx.write(data)
                            .map_err(|e| ProtocolError::new(CoalesceError::<T, U, C>::Write(e)))?;
                        this.state = Flush(target);
                    } else {
                        panic!("invalid state in ErasedFnOnceCoalesce Write")
                    }
                }
                Flush(_) => {
                    ready!(Pin::new(&mut *ctx).poll_ready(cx))
                        .map_err(|e| ProtocolError::new(CoalesceError::<T, U, C>::Write(e)))?;
                    let data = replace(&mut this.state, Done);
                    if let Flush(target) = data {
                        this.state = Target(target);
                    } else {
                        panic!("invalid state in ErasedFnOnceUnravel Write")
                    }
                }
                Target(target) => {
                    let finalize = ready!(Pin::new(target).poll(cx, &mut *ctx))
                        .map_err(|e| ProtocolError::new(CoalesceError::<T, U, C>::Target(e)))?;
                    this.state = Finalize(ctx.finalize(finalize));
                }
                Finalize(future) => {
                    ready!(Pin::new(future).poll(cx, &mut *ctx))
                        .map_err(|e| ProtocolError::new(CoalesceError::<T, U, C>::Finalize(e)))?;
                    this.state = Read;
                }
                Read => {
                    let handle: <C as Dispatch<U>>::Handle = ready!(Pin::new(&mut *ctx).read(cx))
                        .map_err(|e| ProtocolError::new(CoalesceError::<T, U, C>::Read(e)))?;
                    let join: <C as crate::Join<U>>::Future = crate::Join::<U>::join(ctx, handle);
                    this.state = Join(join);
                }
                Join(future) => {
                    let data = ready!(Pin::new(future).poll(cx, &mut *ctx))
                        .map_err(|e| ProtocolError::new(CoalesceError::<T, U, C>::Join(e)))?;
                    this.state = Done;
                    return Poll::Ready(Ok(data));
                }

                Done => panic!("erased FnOnce coalesce polled after completion"),
            }
        }
    }
}

pub struct ErasedFnOnce<T, U, C> {
    context: C,
    data: PhantomData<(T, U)>,
}

impl<
    T,
    U: Flatten<ProtocolError, ErasedFnOnceCoalesce<T, U, C>>,
    C: Notify<T>
        + Join<U>
        + Write<<C as Dispatch<<C as Notify<T>>::Notification>>::Handle>
        + Read<<C as Dispatch<U>>::Handle>
        + Finalize<<C as Fork<<C as Notify<T>>::Notification>>::Finalize>,
> ErasedFnOnce<T, U, C>
where
    <C as Notify<T>>::Wrap: Unpin,
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
    <C as Read<<C as Dispatch<U>>::Handle>>::Error: Error + Send + 'static,
    <<C as Join<U>>::Future as Future<C>>::Error: Error + Send + 'static,
    <<C as Fork<<C as Notify<T>>::Notification>>::Target as Future<C>>::Error:
        Error + Send + 'static,
    <C as Join<U>>::Future: Unpin,
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
                RefContextTarget<C>: Notify<($($name,)*)>
                    + Fork<U>
                    + Read<
                        <RefContextTarget<C> as Dispatch<
                            <RefContextTarget<C> as Notify<($($name,)*)>>::Notification,
                        >>::Handle,
                    > + Write<
                        <RefContextTarget<C> as Dispatch<
                            U,
                        >>::Handle,
                    >,
                U: Unpin,
                <RefContextTarget<C> as Fork<
                    U,
                >>::Target: Unpin,
                <RefContextTarget<C> as Fork<
                    U,
                >>::Future: Unpin,
                <RefContextTarget<C> as Fork<
                    U,
                >>::Finalize: Unpin,
                RefContextTarget<C>: Unpin,
                <RefContextTarget<C> as Notify<($($name,)*)>>::Unwrap: Unpin,
                <RefContextTarget<C> as Join<
                    <RefContextTarget<C> as Notify<($($name,)*)>>::Notification,
                >>::Future: Unpin,
                C::ForkOutput: Unpin,
                C: Unpin,
            {
                type Finalize = ErasedFnOnceUnravel<
                    C,
                    ($($name,)*),
                    U,
                    Box<dyn FnOnce($($name,)*) -> U + 'a $(+ $marker)*>,
                    fn(Box<dyn FnOnce($($name,)*) -> U + 'a $(+ $marker)*>, ($($name,)*)) -> U,
                >;
                type Target = MapErr<ForkContextRef<
                    C,
                    Self::Finalize,
                    Option<Self>,
                    fn(Option<Self>, <C as ReferenceContext>::Context) -> Self::Finalize,
                >, fn(ForkContextRefError<<<C as ReferenceContext>::ForkOutput as Future<C>>::Error, <C as Write<<C as Contextualize>::Handle>>::Error>) -> UnravelError<($($name,)*), U, C>>;

                #[allow(unused_variables)]
                fn unravel(self) -> Self::Target {
                    ForkContextRef::new(Some(self), |call, context| -> Self::Finalize {
                        ErasedFnOnceUnravel {
                            conv: |method, data: ($($name,)*)| (method)($(data.$n,)*),
                            call,
                            context,
                            state: ErasedFnOnceUnravelState::Read,
                        }
                    } as fn(Option<Self>, <C as ReferenceContext>::Context) -> Self::Finalize).map_err(FnOnceUnravelError::Contextualize)
                }
            }

            impl<
                    'a,
                    $($name: Unpin + 'a,)*
                    U: Flatten<ProtocolError, ErasedFnOnceCoalesce<($($name,)*), U, C::Context>> + 'a $(+ $marker)*,
                    C: ?Sized + Read<<C as Contextualize>::Handle> + CloneContext,
                > Coalesce<C> for Box<dyn FnOnce($($name,)*) -> U + 'a $(+ $marker)*>
            where
                ($($name,)*): 'a $(+ $marker)*,
                C::JoinOutput: Unpin,
                C: Unpin,
                C::Context: 'a $(+ $marker)*,
                <C::Context as Notify<($($name,)*)>>::Wrap: Unpin,
                C::Context: Unpin
                    + Read<<C::Context as Dispatch<<C::Context as Notify<($($name,)*)>>::Notification>>::Handle>
                    + Notify<($($name,)*)>
                    + Join<U>
                    + Write<<C::Context as Dispatch<<C::Context as Notify<($($name,)*)>>::Notification>>::Handle>
                    + Read<<C::Context as Dispatch<U>>::Handle>
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
                <C::Context as Read<<C::Context as Dispatch<U>>::Handle>>::Error: Error + Send + 'static,
                <<C::Context as Join<U>>::Future as Future<C::Context>>::Error: Error + Send + 'static,
                <<C::Context as Fork<<C::Context as Notify<($($name,)*)>>::Notification>>::Target as Future<C::Context>>::Error: Error + Send + 'static,
                <<C::Context as Finalize<<C::Context as Fork<<C::Context as Notify<($($name,)*)>>::Notification>>::Finalize>>::Output as Future<C::Context>>::Error: Error + Send + 'static,
                <C::Context as Join<U>>::Future: Unpin,
            {
                type Future = JoinContextOwned<C, Self, fn(C::Context) -> Self>;

                #[allow(non_snake_case)]
                fn coalesce() -> Self::Future {
                    JoinContextOwned::new(|context| {
                        let erased = ErasedFnOnce {
                            context,
                            data: PhantomData
                        };
                        Box::new(move |$($name,)*| erased.call(($($name,)*)))
                    })
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
