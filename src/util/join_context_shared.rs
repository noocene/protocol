use crate::{Contextualize, Future, Read, ShareContext};
use core::{
    borrow::BorrowMut,
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
pub enum JoinContextSharedError<T, U> {
    #[error("failed to join shared context: {0}")]
    Contextualize(#[source] T),
    #[error("failed to read handle for shared context: {0}")]
    Read(#[source] U),
}

enum JoinContextSharedState<C: ?Sized + Read<<C as Contextualize>::Handle> + ShareContext> {
    Read,
    Context(C::JoinOutput),
    Done,
}

pub struct JoinContextShared<
    C: ?Sized + Read<<C as Contextualize>::Handle> + ShareContext,
    T,
    F: FnOnce(C::Context) -> T,
> {
    state: JoinContextSharedState<C>,
    conv: Option<F>,
}

impl<C: ?Sized + Read<<C as Contextualize>::Handle> + ShareContext, T, F: FnOnce(C::Context) -> T> Unpin
    for JoinContextShared<C, T, F>
{
}

impl<C: ?Sized + Read<<C as Contextualize>::Handle> + ShareContext, T, F: FnOnce(C::Context) -> T>
    Future<C> for JoinContextShared<C, T, F>
where
    C: Unpin,
    C::JoinOutput: Unpin,
{
    type Ok = T;
    type Error = JoinContextSharedError<<C::JoinOutput as Future<C>>::Error, C::Error>;

    fn poll<R: BorrowMut<C>>(
        mut self: Pin<&mut Self>,
        cx: &mut Context,
        mut ctx: R,
    ) -> Poll<Result<Self::Ok, Self::Error>> {
        let this = &mut *self;

        let ctx = ctx.borrow_mut();

        loop {
            match &mut this.state {
                JoinContextSharedState::Read => {
                    let handle = ready!(Pin::new(&mut *ctx).read(cx))
                        .map_err(JoinContextSharedError::Read)?;
                    this.state = JoinContextSharedState::Context(ctx.join_shared(handle));
                }
                JoinContextSharedState::Context(future) => {
                    let context = ready!(Pin::new(future).poll(cx, ctx.borrow_mut()))
                        .map_err(JoinContextSharedError::Contextualize)?;
                    this.state = JoinContextSharedState::Done;
                    return Poll::Ready(Ok((this.conv.take().unwrap())(context)));
                }
                JoinContextSharedState::Done => panic!("JoinContextShared polled after completion"),
            }
        }
    }
}

impl<C: ?Sized + Read<<C as Contextualize>::Handle> + ShareContext, T, F: FnOnce(C::Context) -> T>
    JoinContextShared<C, T, F>
{
    pub fn new(conv: F) -> Self {
        JoinContextShared {
            state: JoinContextSharedState::Read,
            conv: Some(conv),
        }
    }
}
