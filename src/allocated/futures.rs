use super::{FromError, ProtocolError};
use crate::{
    Coalesce, Contextualize, Contextualizer, Dispatch, Fork, Future, Join, Read, Unravel, Write,
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
use futures::{ready, TryFutureExt};
use thiserror::Error;

pub enum FutureCoalesceState<T> {
    None,
    Contextualize(T),
    Done,
}

pub struct FutureCoalesce<
    'a,
    O,
    P: Fn(<C as Contextualize<ErasedFutureCoalesce<T, <C as Contextualizer>::Target>>>::Future) -> O,
    T: Unpin,
    C: ?Sized + Contextualize<ErasedFutureCoalesce<T, <C as Contextualizer>::Target>>,
> where
    C::Target: Unpin + Read<<C::Target as Dispatch<T>>::Handle> + Join<T>,
    <C::Target as Join<T>>::Future: Unpin,
{
    conv: P,
    lifetime: PhantomData<&'a ()>,
    state: FutureCoalesceState<C::Output>,
}

pub enum ErasedFutureUnravel<
    T: future::Future,
    C: ?Sized + Write<<C as Dispatch<T::Output>>::Handle> + Fork<T::Output> + Unpin,
> {
    Future(T),
    Fork(C::Future),
    Write(C::Handle, C::Target),
    Flush(C::Target),
    Target(C::Target),
    Done,
}

#[derive(Debug, Error)]
#[bounds(
    where
        T: Error + 'static,
        U: Error + 'static,
        V: Error + 'static,
)]
pub enum ErasedFutureUnravelError<T, U, V> {
    #[error("failed to write handle for erased future: {0}")]
    Transport(#[source] T),
    #[error("failed to fork erased future content: {0}")]
    Dispatch(#[source] U),
    #[error("failed to finalize erased future content: {0}")]
    Target(#[source] V),
}

impl<
        T: future::Future + Unpin,
        C: ?Sized + Write<<C as Dispatch<T::Output>>::Handle> + Fork<T::Output> + Unpin,
    > Future<C> for ErasedFutureUnravel<T, C>
where
    C::Future: Unpin,
    C::Target: Unpin,
    C::Handle: Unpin,
{
    type Ok = ();
    type Error = ErasedFutureUnravelError<
        C::Error,
        <C::Future as Future<C>>::Error,
        <C::Target as Future<C>>::Error,
    >;

    fn poll<R: BorrowMut<C>>(
        mut self: Pin<&mut Self>,
        cx: &mut Context,
        mut ctx: R,
    ) -> Poll<Result<Self::Ok, Self::Error>> {
        let ctx = ctx.borrow_mut();

        let this = &mut *self;

        loop {
            match this {
                ErasedFutureUnravel::Future(future) => {
                    let item = ready!(Pin::new(future).poll(cx));
                    replace(this, ErasedFutureUnravel::Fork(ctx.fork(item)));
                }
                ErasedFutureUnravel::Fork(future) => {
                    let (target, handle) = ready!(Pin::new(&mut *future).poll(cx, &mut *ctx))
                        .map_err(ErasedFutureUnravelError::Dispatch)?;
                    replace(this, ErasedFutureUnravel::Write(handle, target));
                }
                ErasedFutureUnravel::Write(_, _) => {
                    let mut ctx = Pin::new(&mut *ctx);
                    ready!(ctx.as_mut().poll_ready(cx))
                        .map_err(ErasedFutureUnravelError::Transport)?;
                    let data = replace(this, ErasedFutureUnravel::Done);
                    if let ErasedFutureUnravel::Write(data, target) = data {
                        ctx.write(data)
                            .map_err(ErasedFutureUnravelError::Transport)?;
                        replace(this, ErasedFutureUnravel::Flush(target));
                    } else {
                        panic!("invalid state in Tuple1Unravel Write")
                    }
                }
                ErasedFutureUnravel::Flush(_) => {
                    ready!(Pin::new(&mut *ctx).poll_ready(cx))
                        .map_err(ErasedFutureUnravelError::Transport)?;
                    let data = replace(this, ErasedFutureUnravel::Done);
                    if let ErasedFutureUnravel::Flush(target) = data {
                        replace(this, ErasedFutureUnravel::Target(target));
                    } else {
                        panic!("invalid state in Tuple1Unravel Write")
                    }
                }
                ErasedFutureUnravel::Target(target) => {
                    ready!(Pin::new(target).poll(cx, ctx))
                        .map_err(ErasedFutureUnravelError::Target)?;
                    replace(this, ErasedFutureUnravel::Done);
                    return Poll::Ready(Ok(()));
                }
                ErasedFutureUnravel::Done => panic!("Tuple1Unravel polled after completion"),
            }
        }
    }
}

pub enum ErasedFutureCoalesce<T, C: ?Sized + Join<T>> {
    Read,
    Join(C::Future),
    Done,
}

#[derive(Debug, Error)]
#[bounds(
    where
        T: Error + 'static,
        E: Error + 'static,
)]
pub enum ErasedFutureCoalesceError<T, E> {
    #[error("failed to read handle for erased future: {0}")]
    Transport(T),
    #[error("failed to join erased future content: {0}")]
    Dispatch(E),
}

impl<C: Unpin + ?Sized + Read<<C as Dispatch<T>>::Handle> + Join<T>, T> Future<C>
    for ErasedFutureCoalesce<T, C>
where
    C::Future: Unpin,
{
    type Ok = T;
    type Error = ErasedFutureCoalesceError<C::Error, <C::Future as Future<C>>::Error>;

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
                ErasedFutureCoalesce::Join(future) => {
                    let item = ready!(Pin::new(future).poll(cx, &mut *ctx))
                        .map_err(ErasedFutureCoalesceError::Dispatch)?;
                    replace(&mut *self, ErasedFutureCoalesce::Done);
                    return Poll::Ready(Ok(item));
                }
                ErasedFutureCoalesce::Done => panic!("Tuple1Unravel polled after completion"),
            }
        }
    }
}

impl<
        'a,
        O,
        P: Fn(C::Future) -> O,
        T: Unpin,
        C: ?Sized + Contextualize<ErasedFutureCoalesce<T, <C as Contextualizer>::Target>>,
    > Future<C> for FutureCoalesce<'a, O, P, T, C>
where
    C::Output: Unpin,
    P: Unpin,
    C::Future: 'a,
    C::Target: Unpin + Read<<C::Target as Dispatch<T>>::Handle> + Join<T>,
    <C::Target as Join<T>>::Future: Unpin,
{
    type Ok = O;
    type Error = <C::Output as Future<C>>::Error;

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
                FutureCoalesceState::None => {
                    replace(
                        &mut this.state,
                        FutureCoalesceState::Contextualize(
                            ctx.contextualize(ErasedFutureCoalesce::Read),
                        ),
                    );
                }
                FutureCoalesceState::Contextualize(future) => {
                    let item = ready!(Pin::new(future).poll(cx, &mut *ctx))?;
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
            impl<'a, T: Unpin + FromError<ProtocolError>, C: Contextualize<ErasedFutureCoalesce<T, <C as Contextualizer>::Target>>> Coalesce<C> for Pin<Box<dyn future::Future<Output = T> + 'a $(+ $marker)*>>
            where
                C::Output: Unpin,
                C::Future: 'a $(+ $marker)*,
                C::Target: Unpin + Read<<C::Target as Dispatch<T>>::Handle> + Join<T>,
                <C::Target as Join<T>>::Future: Unpin,
                <C::Target as Read<<C::Target as Dispatch<T>>::Handle>>::Error: Error + 'static,
                <<C::Target as Join<T>>::Future as Future<C::Target>>::Error: Error + 'static
            {
                type Future = FutureCoalesce<'a, Self, fn(C::Future) -> Self, T, C>;

                fn coalesce() -> Self::Future {
                    fn conv<'a, T: Unpin + FromError<ProtocolError>, C: Contextualize<ErasedFutureCoalesce<T, <C as Contextualizer>::Target>>>(
                        fut: C::Future,
                    ) -> Pin<Box<dyn future::Future<Output = T> + 'a $(+ $marker)*>>
                    where
                        C::Future: 'a $(+ $marker)*,
                        <C::Target as Join<T>>::Future: Unpin,
                        C::Target: Unpin + Read<<C::Target as Dispatch<T>>::Handle> + Join<T>,
                        <C::Target as Read<<C::Target as Dispatch<T>>::Handle>>::Error: Error + 'static,
                        <<C::Target as Join<T>>::Future as Future<C::Target>>::Error: Error + 'static
                    {
                        Box::pin(fut.unwrap_or_else(|e| T::from_error(ProtocolError(Box::new(e)))))
                    }

                    FutureCoalesce {
                        lifetime: PhantomData,
                        state: FutureCoalesceState::None,
                        conv: conv::<'a, T, C>,
                    }
                }
            }

            impl<'a, T, C: ?Sized + Write<<C as Dispatch<T>>::Handle> + Fork<T> + Unpin> Unravel<C>
                for Pin<Box<dyn future::Future<Output = T> + 'a $(+ $marker)*>>
            where
                C::Future: Unpin,
                C::Target: Unpin,
                C::Handle: Unpin,
            {
                type Future = ErasedFutureUnravel<Self, C>;

                fn unravel(self) -> Self::Future {
                    ErasedFutureUnravel::Future(self)
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
