use crate::{ready, Coalesce, Dispatch, Fork, Future, Join, Read, Unravel, Write};
use alloc::vec::{IntoIter, Vec};
use core::{
    borrow::BorrowMut,
    iter::FromIterator,
    marker::PhantomData,
    mem::replace,
    pin::Pin,
    task::{Context, Poll},
};

enum IteratorUnravelState {
    Writing,
    Flushing,
    Done,
}

pub struct IteratorUnravel<
    T: IntoIterator,
    C: ?Sized + Write<Vec<<C as Dispatch<T::Item>>::Handle>> + Fork<T::Item>,
> {
    fork: Option<C::Future>,
    handles: Vec<C::Handle>,
    state: IteratorUnravelState,
    data: T::IntoIter,
}

pub struct IteratorCoalesce<
    T,
    U: FromIterator<T>,
    C: ?Sized + Read<Vec<<C as Dispatch<T>>::Handle>> + Join<T> + Unpin,
> {
    handles: Option<IntoIter<C::Handle>>,
    join: Option<C::Future>,
    data: Vec<T>,
    ty: PhantomData<U>,
}

#[derive(Debug)]
pub enum IteratorError<T, U> {
    Transport(T),
    Dispatch(U),
}

impl<
        T: IntoIterator,
        C: ?Sized + Write<Vec<<C as Dispatch<T::Item>>::Handle>> + Fork<T::Item> + Unpin,
    > Future<C> for IteratorUnravel<T, C>
where
    T::IntoIter: Unpin,
    C::Handle: Unpin,
    C::Future: Unpin,
{
    type Ok = ();
    type Error = IteratorError<C::Error, <C::Future as Future<C>>::Error>;

    fn poll<R: BorrowMut<C>>(
        mut self: Pin<&mut Self>,
        cx: &mut Context,
        mut ctx: R,
    ) -> Poll<Result<Self::Ok, Self::Error>> {
        let ctx = ctx.borrow_mut();

        let this = &mut *self;

        loop {
            if let Some(future) = this.fork.as_mut() {
                let handle = ready!(Pin::new(future).poll(cx, &mut *ctx))
                    .map_err(IteratorError::Dispatch)?;
                this.handles.push(handle);
                this.fork.take();
            } else if let Some(item) = this.data.next() {
                this.fork = Some(ctx.fork(item));
            } else {
                let mut ctx = Pin::new(&mut *ctx);
                match this.state {
                    IteratorUnravelState::Writing => {
                        ready!(ctx.as_mut().poll_ready(cx)).map_err(IteratorError::Transport)?;
                        ctx.write(replace(&mut this.handles, Vec::new()))
                            .map_err(IteratorError::Transport)?;
                        this.state = IteratorUnravelState::Flushing;
                    }
                    IteratorUnravelState::Flushing => {
                        ready!(ctx.as_mut().poll_flush(cx)).map_err(IteratorError::Transport)?;
                        this.state = IteratorUnravelState::Done;
                    }
                    IteratorUnravelState::Done => panic!("IteratorUnravel polled after completion"),
                }
            }
        }
    }
}

impl<
        T,
        U: FromIterator<T>,
        C: ?Sized + Read<Vec<<C as Dispatch<T>>::Handle>> + Join<T> + Unpin,
    > Future<C> for IteratorCoalesce<T, U, C>
where
    C::Future: Unpin,
    C::Handle: Unpin,
    U: Unpin,
    T: Unpin,
{
    type Ok = U;
    type Error = IteratorError<C::Error, <C::Future as Future<C>>::Error>;

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
            if let Some(handles) = &mut this.handles {
                if let Some(future) = this.join.as_mut() {
                    let handle = ready!(Pin::new(future).poll(cx, &mut *ctx))
                        .map_err(IteratorError::Dispatch)?;
                    this.data.push(handle);
                    this.join.take();
                } else if let Some(handle) = handles.next() {
                    this.join = Some(ctx.join(handle));
                } else {
                    return Poll::Ready(Ok(replace(&mut this.data, Vec::new())
                        .into_iter()
                        .collect()));
                }
            } else {
                this.handles = Some(
                    ready!(Pin::new(&mut *ctx).read(cx))
                        .map_err(IteratorError::Transport)?
                        .into_iter(),
                );
            }
        }
    }
}

impl<T, C: ?Sized + Write<Vec<<C as Dispatch<T>>::Handle>> + Fork<T> + Unpin> Unravel<C> for Vec<T>
where
    T: Unpin,
    C::Handle: Unpin,
    C::Future: Unpin,
{
    type Future = IteratorUnravel<Vec<T>, C>;

    fn unravel(self) -> Self::Future {
        let data = self.into_iter();

        IteratorUnravel {
            fork: None,
            handles: Vec::with_capacity(data.size_hint().0),
            data,
            state: IteratorUnravelState::Writing,
        }
    }
}

impl<T, C: ?Sized + Read<Vec<<C as Dispatch<T>>::Handle>> + Join<T> + Unpin> Coalesce<C> for Vec<T>
where
    T: Unpin,
    C::Handle: Unpin,
    C::Future: Unpin,
{
    type Future = IteratorCoalesce<T, Vec<T>, C>;

    fn coalesce() -> Self::Future {
        IteratorCoalesce {
            data: Vec::new(),
            handles: None,
            join: None,
            ty: PhantomData,
        }
    }
}
