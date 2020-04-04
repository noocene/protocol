use crate::{Contextualize, Future, ReferenceContext, Write};
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
        T: Error + 'static,
        U: Error + 'static,
)]
pub enum ForkContextRefError<T, U> {
    #[error("failed to fork reference context: {0}")]
    Contextualize(#[source] T),
    #[error("failed to write handle for reference context: {0}")]
    Write(#[source] U),
}

enum ForkContextRefState<C: ?Sized + Write<<C as Contextualize>::Handle> + ReferenceContext> {
    None,
    Context(C::ForkOutput),
    Write(C::Context, C::Handle),
    Flush(C::Context),
    Done,
}

pub struct ForkContextRef<
    C: ?Sized + Write<<C as Contextualize>::Handle> + ReferenceContext,
    T,
    U,
    F: FnOnce(U, C::Context) -> T,
> {
    state: ForkContextRefState<C>,
    conv: Option<(F, U)>,
}

impl<
    C: ?Sized + Write<<C as Contextualize>::Handle> + ReferenceContext,
    T,
    U,
    F: FnOnce(U, C::Context) -> T,
> Unpin for ForkContextRef<C, T, U, F>
{
}

impl<
    C: ?Sized + Write<<C as Contextualize>::Handle> + ReferenceContext,
    T,
    U,
    F: FnOnce(U, C::Context) -> T,
> Future<C> for ForkContextRef<C, T, U, F>
where
    C: Unpin,
    C::ForkOutput: Unpin,
{
    type Ok = T;
    type Error = ForkContextRefError<<C::ForkOutput as Future<C>>::Error, C::Error>;

    fn poll<R: BorrowMut<C>>(
        mut self: Pin<&mut Self>,
        cx: &mut Context,
        mut ctx: R,
    ) -> Poll<Result<Self::Ok, Self::Error>> {
        let this = &mut *self;

        loop {
            match &mut this.state {
                ForkContextRefState::None => {
                    this.state = ForkContextRefState::Context(ctx.borrow_mut().fork_ref())
                }
                ForkContextRefState::Context(future) => {
                    let (context, handle) = ready!(Pin::new(future).poll(cx, ctx.borrow_mut()))
                        .map_err(ForkContextRefError::Contextualize)?;
                    this.state = ForkContextRefState::Write(context, handle);
                }
                ForkContextRefState::Write(_, _) => {
                    let mut ctx = Pin::new(ctx.borrow_mut());
                    ready!(ctx.as_mut().poll_ready(cx)).map_err(ForkContextRefError::Write)?;
                    let data = replace(&mut this.state, ForkContextRefState::None);
                    if let ForkContextRefState::Write(context, handle) = data {
                        ctx.write(handle).map_err(ForkContextRefError::Write)?;
                        this.state = ForkContextRefState::Flush(context);
                    } else {
                        panic!("invalid state")
                    }
                }
                ForkContextRefState::Flush(_) => {
                    let mut ctx = Pin::new(ctx.borrow_mut());
                    ready!(ctx.as_mut().poll_flush(cx)).map_err(ForkContextRefError::Write)?;
                    let data = replace(&mut this.state, ForkContextRefState::None);
                    if let ForkContextRefState::Flush(context) = data {
                        this.state = ForkContextRefState::Done;
                        let (conv, state) = self.conv.take().unwrap();
                        return Poll::Ready(Ok((conv)(state, context)));
                    } else {
                        panic!("invalid state")
                    }
                }
                ForkContextRefState::Done => panic!("ForkContextRef polled after completion"),
            }
        }
    }
}

impl<
    C: ?Sized + Write<<C as Contextualize>::Handle> + ReferenceContext,
    T,
    U,
    F: FnOnce(U, C::Context) -> T,
> ForkContextRef<C, T, U, F>
{
    pub fn new(state: U, conv: F) -> Self {
        ForkContextRef {
            state: ForkContextRefState::None,
            conv: Some((conv, state)),
        }
    }
}
