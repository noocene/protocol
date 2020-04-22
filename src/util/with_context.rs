use crate::Future;
use core::{
    borrow::BorrowMut,
    marker::PhantomData,
    mem::replace,
    pin::Pin,
    task::{Context, Poll},
};
use futures::ready;

enum State<Fut, F, T> {
    None,
    F(F, T),
    Fut(Fut),
}

pub struct WithContext<C: ?Sized, Fut: Unpin + Future<C>, F: FnOnce(&mut C, T) -> Fut, T>(
    State<Fut, F, T>,
    PhantomData<(Fut, C)>,
);

impl<C: ?Sized, F: FnOnce(&mut C, T) -> Fut, Fut: Unpin + Future<C>, T> WithContext<C, Fut, F, T> {
    pub fn new(data: T, call: F) -> Self {
        WithContext(State::F(call, data), PhantomData)
    }
}

impl<C: ?Sized, F: FnOnce(&mut C, T) -> Fut, Fut: Unpin + Future<C>, T> Unpin
    for WithContext<C, Fut, F, T>
{
}

impl<C: ?Sized, F: FnOnce(&mut C, T) -> Fut, Fut: Unpin + Future<C>, T> Future<C>
    for WithContext<C, Fut, F, T>
{
    type Ok = Fut::Ok;
    type Error = Fut::Error;

    fn poll<R: BorrowMut<C>>(
        mut self: Pin<&mut Self>,
        cx: &mut Context,
        mut ctx: R,
    ) -> Poll<Result<Self::Ok, Self::Error>> {
        let this = &mut *self;
        let ctx = ctx.borrow_mut();

        loop {
            match &mut this.0 {
                State::F(_, _) => {
                    let state = replace(&mut this.0, State::None);
                    if let State::F(call, data) = state {
                        this.0 = State::Fut((call)(&mut *ctx, data));
                    } else {
                        panic!("invalid state")
                    }
                }
                State::Fut(fut) => {
                    let item = ready!(Pin::new(fut).poll(cx, &mut *ctx))?;
                    this.0 = State::None;
                    return Poll::Ready(Ok(item));
                }
                State::None => panic!("WithContext polled after completion"),
            }
        }
    }
}
