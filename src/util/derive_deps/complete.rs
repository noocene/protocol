use crate::{FinalizeImmediate, Future, Write};
use core::{
    borrow::BorrowMut,
    mem::replace,
    pin::Pin,
    task::{Context, Poll},
};
use futures::ready;

pub enum Complete<T> {
    Write(T),
    Flush,
    Done,
}

impl<T> Unpin for Complete<T> {}

impl<T, C: Unpin + Write<T>> Future<C> for Complete<T> {
    type Ok = ();
    type Error = C::Error;

    fn poll<R: BorrowMut<C>>(
        mut self: Pin<&mut Self>,
        cx: &mut Context,
        mut ctx: R,
    ) -> Poll<Result<Self::Ok, Self::Error>> {
        let this = &mut *self;

        loop {
            match this {
                Complete::Write(_) => {
                    let mut ctx = Pin::new(ctx.borrow_mut());
                    ready!(ctx.as_mut().poll_ready(cx))?;
                    let data = replace(this, Complete::Done);
                    if let Complete::Write(data) = data {
                        ctx.write(data)?;
                        *this = Complete::Flush;
                    } else {
                        panic!("invalid state in erased object terminator")
                    }
                }
                Complete::Flush => {
                    ready!(Pin::new(ctx.borrow_mut()).poll_flush(cx))?;
                    *this = Complete::Done;
                    return Poll::Ready(Ok(()));
                }
                Complete::Done => panic!("erased object terminator polled after completion"),
            }
        }
    }
}

impl<T> Complete<T> {
    pub fn complete<C: FinalizeImmediate<Self>, R: BorrowMut<C>>(mut ctx: R, item: T)
    where
        <C as FinalizeImmediate<Self>>::Target: Write<T> + Unpin,
    {
        let _ = ctx.borrow_mut().finalize_immediate(Complete::Write(item));
    }
}
