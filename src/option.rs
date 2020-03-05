use crate::{ready, Coalesce, Dispatch, Fork, Future, Join, Read, Unravel, Write};
use core::{
    borrow::BorrowMut,
    mem::replace,
    pin::Pin,
    task::{Context, Poll},
};

pub enum OptionUnravel<
    T: Unpin,
    C: ?Sized + Write<Option<<C as Dispatch<T>>::Handle>> + Fork<T> + Unpin,
> {
    Some(T),
    None,
    Fork(C::Future),
    Write(C::Handle),
    Flush,
    Done,
}

pub enum OptionCoalesce<
    T: Unpin,
    C: ?Sized + Read<Option<<C as Dispatch<T>>::Handle>> + Join<T> + Unpin,
> {
    Read,
    Join(C::Future),
    Done,
}

impl<T: Unpin, C: ?Sized + Write<Option<<C as Dispatch<T>>::Handle>> + Fork<T> + Unpin>
    From<Option<T>> for OptionUnravel<T, C>
{
    fn from(data: Option<T>) -> Self {
        if let Some(data) = data {
            OptionUnravel::Some(data)
        } else {
            OptionUnravel::None
        }
    }
}

#[derive(Debug)]
pub enum OptionError<T, U> {
    Transport(T),
    Dispatch(U),
}

impl<T: Unpin, C: ?Sized + Write<Option<<C as Dispatch<T>>::Handle>> + Fork<T> + Unpin> Future<C>
    for OptionUnravel<T, C>
where
    C::Future: Unpin,
    C::Handle: Unpin,
{
    type Ok = ();
    type Error = OptionError<C::Error, <C::Future as Future<C>>::Error>;

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
            match this {
                OptionUnravel::None => {
                    let mut ctx = Pin::new(&mut *ctx);
                    ready!(ctx.as_mut().poll_ready(cx)).map_err(OptionError::Transport)?;
                    ctx.write(None).map_err(OptionError::Transport)?;
                    replace(this, OptionUnravel::Flush);
                }
                OptionUnravel::Some(_) => {
                    let data = replace(this, OptionUnravel::Done);
                    if let OptionUnravel::Some(data) = data {
                        replace(this, OptionUnravel::Fork(ctx.fork(data)));
                    } else {
                        panic!("invalid state in OptionUnravel Some")
                    }
                }
                OptionUnravel::Fork(future) => {
                    let handle = ready!(Pin::new(&mut *future).poll(cx, &mut *ctx))
                        .map_err(OptionError::Dispatch)?;
                    replace(this, OptionUnravel::Write(handle));
                }
                OptionUnravel::Write(_) => {
                    let mut ctx = Pin::new(&mut *ctx);
                    ready!(ctx.as_mut().poll_ready(cx)).map_err(OptionError::Transport)?;
                    let data = replace(this, OptionUnravel::Done);
                    if let OptionUnravel::Write(data) = data {
                        ctx.write(Some(data)).map_err(OptionError::Transport)?;
                        replace(this, OptionUnravel::Flush);
                    } else {
                        panic!("invalid state in OptionUnravel Write")
                    }
                }
                OptionUnravel::Flush => {
                    ready!(Pin::new(&mut *ctx).poll_ready(cx)).map_err(OptionError::Transport)?;
                    replace(this, OptionUnravel::Done);
                    return Poll::Ready(Ok(()));
                }
                OptionUnravel::Done => panic!("OptionUnravel polled after completion"),
            }
        }
    }
}

impl<T: Unpin, C: ?Sized + Read<Option<<C as Dispatch<T>>::Handle>> + Join<T> + Unpin> Future<C>
    for OptionCoalesce<T, C>
where
    C::Future: Unpin,
    C::Handle: Unpin,
{
    type Ok = Option<T>;
    type Error = OptionError<C::Error, <C::Future as Future<C>>::Error>;

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
            match this {
                OptionCoalesce::Read => {
                    let mut ctx = Pin::new(&mut *ctx);
                    match ready!(ctx.as_mut().read(cx)).map_err(OptionError::Transport)? {
                        None => {
                            replace(this, OptionCoalesce::Done);
                            return Poll::Ready(Ok(None));
                        }
                        Some(handle) => {
                            replace(this, OptionCoalesce::Join(ctx.join(handle)));
                        }
                    }
                }
                OptionCoalesce::Join(future) => {
                    return Poll::Ready(Ok(Some(
                        ready!(Pin::new(future).poll(cx, &mut *ctx))
                            .map_err(OptionError::Dispatch)?,
                    )));
                }
                OptionCoalesce::Done => panic!("OptionUnravel polled after completion"),
            }
        }
    }
}

impl<T: Unpin, C: ?Sized + Write<Option<<C as Dispatch<T>>::Handle>> + Fork<T> + Unpin> Unravel<C>
    for Option<T>
where
    C::Future: Unpin,
    C::Handle: Unpin,
{
    type Future = OptionUnravel<T, C>;

    fn unravel(self) -> Self::Future {
        self.into()
    }
}

impl<T: Unpin, C: ?Sized + Read<Option<<C as Dispatch<T>>::Handle>> + Join<T> + Unpin> Coalesce<C>
    for Option<T>
where
    C::Future: Unpin,
    C::Handle: Unpin,
{
    type Future = OptionCoalesce<T, C>;

    fn coalesce() -> Self::Future {
        OptionCoalesce::Read
    }
}
