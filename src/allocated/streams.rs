use super::{FromError, ProtocolError};
use crate::{
    Coalesce, CoalesceContextualizer, ContextualizeCoalesce, Dispatch, Fork, Future, Join, Read,
    Unravel, Write,
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
use futures::{ready, Stream, TryFutureExt};
use thiserror::Error;

pub enum StreamCoalesceState<T> {
    None,
    Contextualize(T),
    Done,
}

pub struct StreamCoalesce<
    'a,
    O,
    P: Fn(
        <C as ContextualizeCoalesce<
            ErasedStreamCoalesce<T, <C as CoalesceContextualizer>::Target>,
        >>::Future,
    ) -> O,
    T: Unpin,
    C: ?Sized + ContextualizeCoalesce<ErasedStreamCoalesce<T, <C as CoalesceContextualizer>::Target>>,
> where
    C::Target: Unpin + Read<Option<<C::Target as Dispatch<T>>::Handle>> + Join<T>,
    <C::Target as Join<T>>::Future: Unpin,
{
    conv: P,
    lifetime: PhantomData<&'a ()>,
    state: StreamCoalesceState<C::Output>,
}

pub enum StreamUnravelState<
    T: Stream,
    C: ?Sized + Write<Option<<C as Dispatch<T::Item>>::Handle>> + Fork<T::Item> + Unpin,
> {
    Stream,
    Fork(C::Future),
    Write(Option<C::Handle>, Option<C::Target>),
    Flush(Option<C::Target>),
    Target(C::Target),
    Done,
}

pub struct StreamUnravel<
    T: Stream,
    C: ?Sized + Write<Option<<C as Dispatch<T::Item>>::Handle>> + Fork<T::Item> + Unpin,
> {
    state: StreamUnravelState<T, C>,
    stream: T,
}

#[derive(Debug, Error)]
#[bounds(
    where
        T: Error + 'static,
        U: Error + 'static,
        V: Error + 'static,
)]
pub enum StreamUnravelError<T, U, V> {
    #[error("failed to write handle for erased future: {0}")]
    Transport(#[source] T),
    #[error("failed to fork erased future content: {0}")]
    Dispatch(#[source] U),
    #[error("failed to finalize erased future content: {0}")]
    Target(#[source] V),
}

impl<
        T: Stream + Unpin,
        C: ?Sized + Write<Option<<C as Dispatch<T::Item>>::Handle>> + Fork<T::Item> + Unpin,
    > Future<C> for StreamUnravel<T, C>
where
    C::Future: Unpin,
    C::Target: Unpin,
    C::Handle: Unpin,
{
    type Ok = ();
    type Error = StreamUnravelError<
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
            match &mut this.state {
                StreamUnravelState::Stream => {
                    if let Some(item) = ready!(Pin::new(&mut this.stream).poll_next(cx)) {
                        replace(&mut this.state, StreamUnravelState::Fork(ctx.fork(item)));
                    } else {
                        replace(&mut this.state, StreamUnravelState::Write(None, None));
                    }
                }
                StreamUnravelState::Fork(future) => {
                    let (target, handle) = ready!(Pin::new(&mut *future).poll(cx, &mut *ctx))
                        .map_err(StreamUnravelError::Dispatch)?;
                    replace(
                        &mut this.state,
                        StreamUnravelState::Write(Some(handle), Some(target)),
                    );
                }
                StreamUnravelState::Write(_, _) => {
                    let mut ctx = Pin::new(&mut *ctx);
                    ready!(ctx.as_mut().poll_ready(cx)).map_err(StreamUnravelError::Transport)?;
                    let data = replace(&mut this.state, StreamUnravelState::Done);
                    if let StreamUnravelState::Write(data, target) = data {
                        ctx.write(data).map_err(StreamUnravelError::Transport)?;
                        replace(&mut this.state, StreamUnravelState::Flush(target));
                    } else {
                        panic!("invalid state in StreamUnravel Write")
                    }
                }
                StreamUnravelState::Flush(_) => {
                    ready!(Pin::new(&mut *ctx).poll_ready(cx))
                        .map_err(StreamUnravelError::Transport)?;
                    let data = replace(&mut this.state, StreamUnravelState::Done);
                    if let StreamUnravelState::Flush(target) = data {
                        if let Some(target) = target {
                            replace(&mut this.state, StreamUnravelState::Target(target));
                        } else {
                            replace(&mut this.state, StreamUnravelState::Done);
                        }
                    } else {
                        panic!("invalid state in StreamUnravel Write")
                    }
                }
                StreamUnravelState::Target(target) => {
                    ready!(Pin::new(target).poll(cx, ctx)).map_err(StreamUnravelError::Target)?;
                    replace(&mut this.state, StreamUnravelState::Stream);
                    return Poll::Ready(Ok(()));
                }
                StreamUnravelState::Done => panic!("StreamUnravel polled after completion"),
            }
        }
    }
}

pub enum ErasedStreamCoalesce<T, C: ?Sized + Join<T>> {
    Read,
    Join(C::Future),
}

#[derive(Debug, Error)]
#[bounds(
    where
        T: Error + 'static,
        E: Error + 'static,
)]
pub enum ErasedStreamCoalesceError<T, E> {
    #[error("failed to read handle for erased future: {0}")]
    Transport(T),
    #[error("failed to join erased future content: {0}")]
    Dispatch(E),
}

pub struct FutureToStream<F>(F);

impl<T, F: Unpin + future::Future<Output = Option<T>>> Stream for FutureToStream<F> {
    type Item = T;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        Pin::new(&mut self.0).poll(cx)
    }
}

impl<C: Unpin + ?Sized + Read<Option<<C as Dispatch<T>>::Handle>> + Join<T>, T> Future<C>
    for ErasedStreamCoalesce<T, C>
where
    C::Future: Unpin,
{
    type Ok = Option<T>;
    type Error = ErasedStreamCoalesceError<C::Error, <C::Future as Future<C>>::Error>;

    fn poll<R: BorrowMut<C>>(
        mut self: Pin<&mut Self>,
        cx: &mut Context,
        mut ctx: R,
    ) -> Poll<Result<Self::Ok, Self::Error>> {
        let ctx = ctx.borrow_mut();

        loop {
            match &mut *self {
                ErasedStreamCoalesce::Read => {
                    let mut ctx = Pin::new(&mut *ctx);
                    if let Some(handle) = ready!(ctx.as_mut().read(cx))
                        .map_err(ErasedStreamCoalesceError::Transport)?
                    {
                        replace(&mut *self, ErasedStreamCoalesce::Join(ctx.join(handle)));
                    } else {
                        replace(&mut *self, ErasedStreamCoalesce::Read);
                        return Poll::Ready(Ok(None));
                    }
                }
                ErasedStreamCoalesce::Join(future) => {
                    let item = ready!(Pin::new(future).poll(cx, &mut *ctx))
                        .map_err(ErasedStreamCoalesceError::Dispatch)?;
                    replace(&mut *self, ErasedStreamCoalesce::Read);
                    return Poll::Ready(Ok(Some(item)));
                }
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
            + ContextualizeCoalesce<ErasedStreamCoalesce<T, <C as CoalesceContextualizer>::Target>>,
    > Future<C> for StreamCoalesce<'a, O, P, T, C>
where
    C::Output: Unpin,
    P: Unpin,
    C::Future: 'a,
    C::Target: Unpin + Read<Option<<C::Target as Dispatch<T>>::Handle>> + Join<T>,
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
                StreamCoalesceState::None => {
                    replace(
                        &mut this.state,
                        StreamCoalesceState::Contextualize(
                            ctx.contextualize(ErasedStreamCoalesce::Read),
                        ),
                    );
                }
                StreamCoalesceState::Contextualize(future) => {
                    let item = ready!(Pin::new(future).poll(cx, &mut *ctx))?;
                    replace(&mut this.state, StreamCoalesceState::Done);
                    return Poll::Ready(Ok((this.conv)(item)));
                }
                StreamCoalesceState::Done => panic!("StreamCoalesce polled after completion"),
            }
        }
    }
}

macro_rules! marker_variants {
    ($(
        $($marker:ident)*
    ),+) => {
        $(
            impl<'a, T: Unpin + FromError<ProtocolError>, C: ContextualizeCoalesce<ErasedStreamCoalesce<T, <C as CoalesceContextualizer>::Target>>> Coalesce<C> for Pin<Box<dyn Stream<Item = T> + 'a $(+ $marker)*>>
            where
                C::Output: Unpin,
                C::Future: Unpin + 'a $(+ $marker)*,
                C::Target: Unpin + Read<Option<<C::Target as Dispatch<T>>::Handle>> + Join<T>,
                <C::Target as Join<T>>::Future: Unpin,
                <C::Target as Read<Option<<C::Target as Dispatch<T>>::Handle>>>::Error: Error + 'static,
                <<C::Target as Join<T>>::Future as Future<C::Target>>::Error: Error + 'static
            {
                type Future = StreamCoalesce<'a, Self, fn(C::Future) -> Self, T, C>;

                fn coalesce() -> Self::Future {
                    fn conv<'a, T: Unpin + FromError<ProtocolError>, C: ContextualizeCoalesce<ErasedStreamCoalesce<T, <C as CoalesceContextualizer>::Target>>>(
                        fut: C::Future,
                    ) -> Pin<Box<dyn Stream<Item = T> + 'a $(+ $marker)*>>
                    where
                        C::Future: Unpin + 'a $(+ $marker)*,
                        <C::Target as Join<T>>::Future: Unpin,
                        C::Target: Unpin + Read<Option<<C::Target as Dispatch<T>>::Handle>> + Join<T>,
                        <C::Target as Read<Option<<C::Target as Dispatch<T>>::Handle>>>::Error: Error + 'static,
                        <<C::Target as Join<T>>::Future as Future<C::Target>>::Error: Error + 'static,
                    {
                        Box::pin(
                            FutureToStream(fut.unwrap_or_else(|e| Some(T::from_error(ProtocolError(Box::new(e))))))
                        )
                    }

                    StreamCoalesce {
                        lifetime: PhantomData,
                        state: StreamCoalesceState::None,
                        conv: conv::<'a, T, C>,
                    }
                }
            }

            impl<'a, T, C: ?Sized + Write<Option<<C as Dispatch<T>>::Handle>> + Fork<T> + Unpin> Unravel<C>
                for Pin<Box<dyn Stream<Item = T> + 'a $(+ $marker)*>>
            where
                C::Future: Unpin,
                C::Target: Unpin,
                C::Handle: Unpin,
            {
                type Future = StreamUnravel<Self, C>;

                fn unravel(self) -> Self::Future {
                    StreamUnravel {
                        state: StreamUnravelState::Stream,
                        stream: self
                    }
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
