use crate::{
    future::MapErr, Coalesce, Dispatch, Fork, Future, FutureExt, Join, Read, Unravel, Write,
};
use alloc::{boxed::Box, format, string::String, vec, vec::Vec};
use arrayvec::ArrayVec;
use core::{
    borrow::BorrowMut,
    fmt::{self, Debug, Display, Formatter},
    mem::replace,
    pin::Pin,
    task::{Context, Poll},
};
use core_error::Error;
use futures::ready;
use thiserror::Error;

type ErrorData = ([String; 2], Vec<[String; 2]>);

pub struct ErasedError {
    display: String,
    debug: String,
    source: Option<Box<ErasedError>>,
}

impl Display for ErasedError {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "{}", self.display)
    }
}

impl Debug for ErasedError {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "{}", self.debug)
    }
}

impl Error for ErasedError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.source.as_ref().map(|source| source as &dyn Error)
    }
}

struct ErrorWrapper<T: ?Sized + Error + 'static>(Box<T>);

impl<T: ?Sized + Error + 'static> Display for ErrorWrapper<T> {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl<T: ?Sized + Error + 'static> Debug for ErrorWrapper<T> {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl<T: ?Sized + Error + 'static> Error for ErrorWrapper<T> {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.0.source()
    }
}

fn into_data<T: ?Sized + Error + 'static>(error: Box<T>) -> ErrorData {
    let mut data = vec![];
    let wrapped = ErrorWrapper(error);
    let mut error = Some(&wrapped as &(dyn Error + 'static));
    while let Some(e) = error {
        data.push([format!("{}", e), format!("{:?}", e)]);
        error = e.source();
    }
    ([format!("{}", wrapped), format!("{:?}", wrapped)], data)
}

fn from_data(initial: ErrorData) -> ErasedError {
    let data = initial.1.into_iter();
    fn construct(
        item: [String; 2],
        mut remainder: impl Iterator<Item = [String; 2]>,
    ) -> ErasedError {
        let mut item = ArrayVec::from(item).into_iter();
        ErasedError {
            display: item.next().unwrap(),
            debug: item.next().unwrap(),
            source: remainder
                .next()
                .map(|item| Box::new(construct(item, remainder))),
        }
    }
    construct(initial.0, data)
}

pub enum ErrorUnravel<
    C: ?Sized + Write<<C as Dispatch<ErrorData>>::Handle> + Fork<ErrorData> + Unpin,
> {
    Data(ErrorData),
    Fork(C::Future),
    Write(C::Handle, C::Target),
    Flush(C::Target),
    Target(C::Target),
    Done,
}

pub enum ErrorCoalesceState<
    C: ?Sized + Read<<C as Dispatch<ErrorData>>::Handle> + Join<ErrorData> + Unpin,
> {
    Read,
    Join(C::Future),
    Done,
}

pub struct ErrorCoalesce<
    T,
    F: FnMut(ErasedError) -> T,
    C: ?Sized + Read<<C as Dispatch<ErrorData>>::Handle> + Join<ErrorData> + Unpin,
> {
    conv: F,
    state: ErrorCoalesceState<C>,
}

#[derive(Debug, Error)]
#[bounds(
    where
        T: Error + 'static,
        U: Error + 'static,
        V: Error + 'static,
)]
pub enum ErrorUnravelError<T, U, V> {
    #[error("failed to write handle for error: {0}")]
    Transport(#[source] T),
    #[error("failed to fork data in error: {0}")]
    Dispatch(#[source] U),
    #[error("failed to finalize data in error: {0}")]
    Target(#[source] V),
}

#[derive(Debug, Error)]
#[bounds(
    where
        T: Error + 'static,
        U: Error + 'static,
)]
pub enum ErrorCoalesceError<T, U> {
    #[error("failed to read handle for error: {0}")]
    Transport(#[source] T),
    #[error("failed to join data in error: {0}")]
    Dispatch(#[source] U),
}

impl<C: ?Sized + Write<<C as Dispatch<ErrorData>>::Handle> + Fork<ErrorData> + Unpin> Future<C>
    for ErrorUnravel<C>
where
    C::Future: Unpin,
    C::Target: Unpin,
    C::Handle: Unpin,
{
    type Ok = MapErr<
        C::Finalize,
        fn(
            <C::Finalize as Future<C>>::Error,
        ) -> ErrorUnravelError<
            C::Error,
            <C::Future as Future<C>>::Error,
            <C::Target as Future<C>>::Error,
        >,
    >;
    type Error = ErrorUnravelError<
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
                ErrorUnravel::Data(_) => {
                    let data = replace(this, ErrorUnravel::Done);
                    if let ErrorUnravel::Data(data) = data {
                        replace(this, ErrorUnravel::Fork(ctx.fork(data)));
                    } else {
                        panic!("invalid state in ErrorUnravel Data")
                    }
                }
                ErrorUnravel::Fork(future) => {
                    let (target, handle) = ready!(Pin::new(&mut *future).poll(cx, &mut *ctx))
                        .map_err(ErrorUnravelError::Dispatch)?;
                    replace(this, ErrorUnravel::Write(handle, target));
                }
                ErrorUnravel::Write(_, _) => {
                    let mut ctx = Pin::new(&mut *ctx);
                    ready!(ctx.as_mut().poll_ready(cx)).map_err(ErrorUnravelError::Transport)?;
                    let data = replace(this, ErrorUnravel::Done);
                    if let ErrorUnravel::Write(data, target) = data {
                        ctx.write(data).map_err(ErrorUnravelError::Transport)?;
                        replace(this, ErrorUnravel::Flush(target));
                    } else {
                        panic!("invalid state in ErrorUnravel Write")
                    }
                }
                ErrorUnravel::Flush(_) => {
                    ready!(Pin::new(&mut *ctx).poll_ready(cx))
                        .map_err(ErrorUnravelError::Transport)?;
                    let data = replace(this, ErrorUnravel::Done);
                    if let ErrorUnravel::Flush(target) = data {
                        replace(this, ErrorUnravel::Target(target));
                    } else {
                        panic!("invalid state in ErrorUnravel Write")
                    }
                }
                ErrorUnravel::Target(target) => {
                    let finalize = ready!(Pin::new(target).poll(cx, ctx))
                        .map_err(ErrorUnravelError::Target)?;
                    replace(this, ErrorUnravel::Done);
                    return Poll::Ready(Ok(finalize.map_err(ErrorUnravelError::Target)));
                }
                ErrorUnravel::Done => panic!("ErrorUnravel polled after completion"),
            }
        }
    }
}

impl<
        T,
        F: FnMut(ErasedError) -> T,
        C: ?Sized + Read<<C as Dispatch<ErrorData>>::Handle> + Join<ErrorData> + Unpin,
    > Unpin for ErrorCoalesce<T, F, C>
{
}

impl<
        T,
        F: FnMut(ErasedError) -> T,
        C: ?Sized + Read<<C as Dispatch<ErrorData>>::Handle> + Join<ErrorData> + Unpin,
    > Future<C> for ErrorCoalesce<T, F, C>
where
    C::Future: Unpin,
    C::Handle: Unpin,
{
    type Ok = T;
    type Error = ErrorCoalesceError<C::Error, <C::Future as Future<C>>::Error>;

    fn poll<R: BorrowMut<C>>(
        mut self: Pin<&mut Self>,
        cx: &mut Context,
        mut ctx: R,
    ) -> Poll<Result<Self::Ok, Self::Error>> {
        let ctx = ctx.borrow_mut();

        let this = &mut *self;

        loop {
            match &mut this.state {
                ErrorCoalesceState::Read => {
                    let mut ctx = Pin::new(&mut *ctx);
                    let handle =
                        ready!(ctx.as_mut().read(cx)).map_err(ErrorCoalesceError::Transport)?;
                    this.state = ErrorCoalesceState::Join(ctx.join(handle));
                }
                ErrorCoalesceState::Join(future) => {
                    let data = ready!(Pin::new(future).poll(cx, &mut *ctx))
                        .map_err(ErrorCoalesceError::Dispatch)?;
                    this.state = ErrorCoalesceState::Done;
                    return Poll::Ready(Ok((this.conv)(from_data(data))));
                }
                ErrorCoalesceState::Done => panic!("ErrorUnravel polled after completion"),
            }
        }
    }
}

macro_rules! marker_variants {
    ($(
        $($marker:ident)*
    ),+) => {
        $(
            impl<C: ?Sized + Write<<C as Dispatch<ErrorData>>::Handle> + Fork<ErrorData> + Unpin> Unravel<C>
                for Box<dyn Error $(+ $marker)*>
            where
                C::Future: Unpin,
                C::Target: Unpin,
                C::Handle: Unpin,
                C::Finalize: Unpin,
            {
                type Finalize = MapErr<
                    C::Finalize,
                    fn(
                        <C::Finalize as Future<C>>::Error,
                    ) -> ErrorUnravelError<
                        C::Error,
                        <C::Future as Future<C>>::Error,
                        <C::Target as Future<C>>::Error,
                    >,
                >;
                type Target = ErrorUnravel<C>;

                fn unravel(self) -> Self::Target {
                    ErrorUnravel::Data(into_data(self))
                }
            }

            impl<C: ?Sized + Read<<C as Dispatch<ErrorData>>::Handle> + Join<ErrorData> + Unpin> Coalesce<C>
                for Box<dyn Error $(+ $marker)*>
            where
                C::Future: Unpin,
                C::Handle: Unpin,
            {
                type Future = ErrorCoalesce<Box<dyn Error $(+ $marker)*>, fn(ErasedError) -> Box<dyn Error $(+ $marker)*>, C>;

                fn coalesce() -> Self::Future {
                    ErrorCoalesce {
                        state: ErrorCoalesceState::Read,
                        conv: |item| Box::new(item),
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