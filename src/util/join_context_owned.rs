use crate::{CloneContext, Contextualize, Future, Read};
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
pub enum JoinContextOwnedError<T, U> {
    #[error("failed to join owned context: {0}")]
    Contextualize(#[source] T),
    #[error("failed to read handle for owned context: {0}")]
    Read(#[source] U),
}

enum JoinContextOwnedState<C: ?Sized + Read<<C as Contextualize>::Handle> + CloneContext> {
    Read,
    Context(C::JoinOutput),
    Done,
}

pub struct JoinContextOwned<
    C: ?Sized + Read<<C as Contextualize>::Handle> + CloneContext,
    T,
    F: FnOnce(C::Context) -> T,
> {
    state: JoinContextOwnedState<C>,
    conv: Option<F>,
}

impl<C: ?Sized + Read<<C as Contextualize>::Handle> + CloneContext, T, F: FnOnce(C::Context) -> T> Unpin
    for JoinContextOwned<C, T, F>
{
}

impl<C: ?Sized + Read<<C as Contextualize>::Handle> + CloneContext, T, F: FnOnce(C::Context) -> T>
    Future<C> for JoinContextOwned<C, T, F>
where
    C: Unpin,
    C::JoinOutput: Unpin,
{
    type Ok = T;
    type Error = JoinContextOwnedError<<C::JoinOutput as Future<C>>::Error, C::Error>;

    fn poll<R: BorrowMut<C>>(
        mut self: Pin<&mut Self>,
        cx: &mut Context,
        mut ctx: R,
    ) -> Poll<Result<Self::Ok, Self::Error>> {
        let this = &mut *self;

        let ctx = ctx.borrow_mut();

        loop {
            match &mut this.state {
                JoinContextOwnedState::Read => {
                    let handle = ready!(Pin::new(&mut *ctx).read(cx))
                        .map_err(JoinContextOwnedError::Read)?;
                    this.state = JoinContextOwnedState::Context(ctx.join_owned(handle));
                }
                JoinContextOwnedState::Context(future) => {
                    let context = ready!(Pin::new(future).poll(cx, ctx.borrow_mut()))
                        .map_err(JoinContextOwnedError::Contextualize)?;
                    this.state = JoinContextOwnedState::Done;
                    return Poll::Ready(Ok((this.conv.take().unwrap())(context)));
                }
                JoinContextOwnedState::Done => panic!("JoinContextOwned polled after completion"),
            }
        }
    }
}

impl<C: ?Sized + Read<<C as Contextualize>::Handle> + CloneContext, T, F: FnOnce(C::Context) -> T>
    JoinContextOwned<C, T, F>
{
    pub fn new(conv: F) -> Self {
        JoinContextOwned {
            state: JoinContextOwnedState::Read,
            conv: Some(conv),
        }
    }
}
