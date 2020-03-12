use crate::{
    future::ordered::{EventualOrdered, EventualOrderedError},
    ready, Coalesce, Dispatch, Fork, Future, Join, Read, Unravel, Write,
};
use alloc::vec::{IntoIter, Vec};
use core::{
    borrow::BorrowMut,
    iter::{FromIterator, Rev},
    marker::PhantomData,
    mem::replace,
    pin::Pin,
    task::{Context, Poll},
};
use core_error::Error;
use thiserror::Error;

enum VecUnravelState {
    Writing,
    Flushing,
    Targets,
    Done,
}

pub struct VecUnravel<
    T: IntoIterator,
    C: ?Sized + Write<Vec<<C as Dispatch<T::Item>>::Handle>> + Fork<T::Item>,
> {
    fork: Option<C::Future>,
    handles: Vec<C::Handle>,
    targets: EventualOrdered<Vec<C::Target>>,
    state: VecUnravelState,
    data: T::IntoIter,
}

pub struct VecCoalesce<
    T,
    U: FromIterator<T>,
    C: ?Sized + Read<Vec<<C as Dispatch<T>>::Handle>> + Join<T> + Unpin,
> {
    handles: Option<IntoIter<C::Handle>>,
    join: Option<C::Future>,
    data: Vec<T>,
    ty: PhantomData<U>,
}

#[derive(Debug, Error)]
#[bounds(
    where
        T: Error + 'static,
        U: Error + 'static,
        V: Error + 'static
)]
pub enum VecUnravelError<T, U, V> {
    #[error("failed to write handle for Vec: {0}")]
    Transport(#[source] T),
    #[error("failed to fork item in Vec: {0}")]
    Dispatch(#[source] U),
    #[error("failed to finalize item in Vec: {0}")]
    Target(#[source] V),
}

#[derive(Debug, Error)]
#[bounds(
    where
        T: Error + 'static,
        U: Error + 'static,
)]
pub enum VecCoalesceError<T, U> {
    #[error("failed to read handle for Vec: {0}")]
    Transport(#[source] T),
    #[error("failed to join item in Vec: {0}")]
    Dispatch(#[source] U),
}

impl<
        T: IntoIterator,
        C: ?Sized + Write<Vec<<C as Dispatch<T::Item>>::Handle>> + Fork<T::Item> + Unpin,
    > Future<C> for VecUnravel<T, C>
where
    T::IntoIter: Unpin,
    C::Handle: Unpin,
    C::Target: Unpin,
    C::Future: Unpin,
{
    type Ok = ();
    type Error =
        VecUnravelError<C::Error, <C::Future as Future<C>>::Error, <C::Target as Future<C>>::Error>;

    fn poll<R: BorrowMut<C>>(
        mut self: Pin<&mut Self>,
        cx: &mut Context,
        mut ctx: R,
    ) -> Poll<Result<Self::Ok, Self::Error>> {
        let ctx = ctx.borrow_mut();

        let this = &mut *self;

        loop {
            if let Some(future) = this.fork.as_mut() {
                let (target, handle) = ready!(Pin::new(future).poll(cx, &mut *ctx))
                    .map_err(VecUnravelError::Dispatch)?;
                this.handles.push(handle);
                this.targets.data().unwrap().push(target);
                this.fork.take();
            } else if let Some(item) = this.data.next() {
                this.fork = Some(ctx.fork(item));
            } else {
                let mut ct = Pin::new(&mut *ctx);
                match this.state {
                    VecUnravelState::Writing => {
                        ready!(ct.as_mut().poll_ready(cx)).map_err(VecUnravelError::Transport)?;
                        ct.write(replace(&mut this.handles, Vec::new()))
                            .map_err(VecUnravelError::Transport)?;
                        this.state = VecUnravelState::Flushing;
                    }
                    VecUnravelState::Flushing => {
                        ready!(ct.as_mut().poll_flush(cx)).map_err(VecUnravelError::Transport)?;
                        this.targets.complete();
                        this.state = VecUnravelState::Targets;
                    }
                    VecUnravelState::Targets => {
                        ready!(Pin::new(&mut this.targets)
                            .poll(cx, &mut *ctx)
                            .map_err(EventualOrderedError::unwrap_complete)
                            .map_err(VecUnravelError::Target))?;
                        this.state = VecUnravelState::Done;
                        return Poll::Ready(Ok(()));
                    }
                    VecUnravelState::Done => panic!("VecUnravel polled after completion"),
                }
            }
        }
    }
}

impl<
        T,
        U: FromIterator<T>,
        C: ?Sized + Read<Vec<<C as Dispatch<T>>::Handle>> + Join<T> + Unpin,
    > Future<C> for VecCoalesce<T, U, C>
where
    C::Future: Unpin,
    C::Handle: Unpin,
    U: Unpin,
    T: Unpin,
{
    type Ok = U;
    type Error = VecCoalesceError<C::Error, <C::Future as Future<C>>::Error>;

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
                        .map_err(VecCoalesceError::Dispatch)?;
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
                        .map_err(VecCoalesceError::Transport)?
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
    C::Target: Unpin,
{
    type Future = VecUnravel<Rev<IntoIter<T>>, C>;

    fn unravel(self) -> Self::Future {
        let data = self.into_iter().rev();

        VecUnravel {
            fork: None,
            handles: Vec::with_capacity(data.size_hint().0),
            targets: EventualOrdered::new(Vec::with_capacity(data.size_hint().0)),
            data,
            state: VecUnravelState::Writing,
        }
    }
}

impl<T, C: ?Sized + Read<Vec<<C as Dispatch<T>>::Handle>> + Join<T> + Unpin> Coalesce<C> for Vec<T>
where
    T: Unpin,
    C::Handle: Unpin,
    C::Future: Unpin,
{
    type Future = VecCoalesce<T, Vec<T>, C>;

    fn coalesce() -> Self::Future {
        VecCoalesce {
            data: Vec::new(),
            handles: None,
            join: None,
            ty: PhantomData,
        }
    }
}
