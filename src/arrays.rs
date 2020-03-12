use crate::{
    future::ordered::{EventualOrdered, EventualOrderedError},
    ready, Coalesce, Dispatch, Fork, Future, Join, Read, Unravel, Write,
};
use arrayvec::{ArrayVec, IntoIter};
use core::{
    borrow::BorrowMut,
    iter::Rev,
    mem::replace,
    pin::Pin,
    task::{Context, Poll},
};
use core_error::Error;
use thiserror::Error;

macro_rules! array_impl {
    ($($len:literal as $coalesce:ident + $unravel:ident)*) => {
        $(
            pub struct $unravel<T, C: ?Sized + Write<[<C as Dispatch<T>>::Handle; $len]> + Fork<T>> {
                fork: Option<C::Future>,
                handles: ArrayVec<[C::Handle; $len]>,
                targets: EventualOrdered<ArrayVec<[C::Target; $len]>>,
                state: ArrayUnravelState,
                data: Rev<IntoIter<[T; $len]>>,
            }

            pub struct $coalesce<T, C: ?Sized + Read<[<C as Dispatch<T>>::Handle; $len]> + Join<T> + Unpin>
            {
                handles: Option<IntoIter<[C::Handle; $len]>>,
                join: Option<C::Future>,
                data: ArrayVec<[T; $len]>,
            }

            impl<T: Unpin, C: ?Sized + Write<[<C as Dispatch<T>>::Handle; $len]> + Fork<T> + Unpin> Future<C>
                for $unravel<T, C>
            where
                C::Handle: Unpin,
                C::Target: Unpin,
                C::Future: Unpin,
            {
                type Ok = ();
                type Error = ArrayUnravelError<C::Error, <C::Future as Future<C>>::Error, <C::Target as Future<C>>::Error>;

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
                                .map_err(ArrayUnravelError::Dispatch)?;
                            this.handles.push(handle);
                            this.targets.data().unwrap().push(target);
                            this.fork.take();
                        } else if let Some(item) = this.data.next() {
                            this.fork = Some(ctx.fork(item));
                        } else {
                            let mut ctx = Pin::new(&mut *ctx);
                            match this.state {
                                ArrayUnravelState::Writing => {
                                    ready!(ctx.as_mut().poll_ready(cx)).map_err(ArrayUnravelError::Transport)?;
                                    ctx.write(
                                        replace(&mut this.handles, ArrayVec::new())
                                            .into_inner()
                                            .unwrap_or_else(|_| panic!("handles incomplete")),
                                    )
                                    .map_err(ArrayUnravelError::Transport)?;
                                    this.state = ArrayUnravelState::Flushing;
                                }
                                ArrayUnravelState::Flushing => {
                                    ready!(ctx.as_mut().poll_flush(cx))
                                        .map_err(ArrayUnravelError::Transport)?;
                                    this.targets.complete();
                                    this.state = ArrayUnravelState::Targets;
                                }
                                ArrayUnravelState::Targets => {
                                    ready!(Pin::new(&mut this.targets)
                                        .poll(cx, &mut *ctx)
                                        .map_err(EventualOrderedError::unwrap_complete)
                                        .map_err(ArrayUnravelError::Target))?;
                                    this.state = ArrayUnravelState::Done;
                                    return Poll::Ready(Ok(()));
                                }
                                ArrayUnravelState::Done => panic!("ArrayUnravel polled after completion"),
                            }
                        }
                    }
                }
            }

            impl<T, C: ?Sized + Read<[<C as Dispatch<T>>::Handle; $len]> + Join<T> + Unpin> Future<C>
                for $coalesce<T, C>
            where
                C::Future: Unpin,
                C::Handle: Unpin,
                T: Unpin,
            {
                type Ok = [T; $len];
                type Error = ArrayCoalesceError<C::Error, <C::Future as Future<C>>::Error>;

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
                                    .map_err(ArrayCoalesceError::Dispatch)?;
                                this.data.push(handle);
                                this.join.take();
                            } else if let Some(handle) = handles.next() {
                                this.join = Some(ctx.join(handle));
                            } else {
                                return Poll::Ready(Ok(replace(&mut this.data, ArrayVec::new())
                                    .into_inner()
                                    .unwrap_or_else(|_| panic!("data incomplete"))));
                            }
                        } else {
                            this.handles = Some(
                                ArrayVec::from(
                                    ready!(Pin::new(&mut *ctx).read(cx)).map_err(ArrayCoalesceError::Transport)?,
                                )
                                .into_iter(),
                            );
                        }
                    }
                }
            }

            impl<T, C: ?Sized + Write<[<C as Dispatch<T>>::Handle; $len]> + Fork<T> + Unpin> Unravel<C> for [T; $len]
            where
                T: Unpin,
                C::Target: Unpin,
                C::Handle: Unpin,
                C::Future: Unpin,
            {
                type Future = $unravel<T, C>;

                fn unravel(self) -> Self::Future {
                    let data = ArrayVec::from(self).into_iter().rev();

                    $unravel {
                        fork: None,
                        handles: ArrayVec::new(),
                        targets: EventualOrdered::new(ArrayVec::new()),
                        data,
                        state: ArrayUnravelState::Writing,
                    }
                }
            }

            impl<T, C: ?Sized + Read<[<C as Dispatch<T>>::Handle; $len]> + Join<T> + Unpin> Coalesce<C> for [T; $len]
            where
                T: Unpin,
                C::Handle: Unpin,
                C::Future: Unpin,
            {
                type Future = $coalesce<T, C>;

                fn coalesce() -> Self::Future {
                    $coalesce {
                        data: ArrayVec::new(),
                        handles: None,
                        join: None,
                    }
                }
            }
        )*
    };
}

enum ArrayUnravelState {
    Writing,
    Flushing,
    Targets,
    Done,
}

#[derive(Debug, Error)]
#[bounds(
    where
        T: Error + 'static,
        U: Error + 'static,
        V: Error + 'static
)]
pub enum ArrayUnravelError<T, U, V> {
    #[error("failed to write handle for array: {0}")]
    Transport(#[source] T),
    #[error("failed to fork item in array: {0}")]
    Dispatch(#[source] U),
    #[error("failed to finalize item in array: {0}")]
    Target(#[source] V),
}

#[derive(Debug, Error)]
#[bounds(
    where
        T: Error + 'static,
        U: Error + 'static,
)]
pub enum ArrayCoalesceError<T, U> {
    #[error("failed to read handle for array: {0}")]
    Transport(#[source] T),
    #[error("failed to join item in array: {0}")]
    Dispatch(#[source] U),
}

array_impl! {
    0001 as Array0001Coalesce + Array0001Unravel
    0002 as Array0002Coalesce + Array0002Unravel
    0003 as Array0003Coalesce + Array0003Unravel
    0004 as Array0004Coalesce + Array0004Unravel
    0005 as Array0005Coalesce + Array0005Unravel
    0006 as Array0006Coalesce + Array0006Unravel
    0007 as Array0007Coalesce + Array0007Unravel
    0008 as Array0008Coalesce + Array0008Unravel
    0009 as Array0009Coalesce + Array0009Unravel
    0010 as Array0010Coalesce + Array0010Unravel
    0011 as Array0011Coalesce + Array0011Unravel
    0012 as Array0012Coalesce + Array0012Unravel
    0013 as Array0013Coalesce + Array0013Unravel
    0014 as Array0014Coalesce + Array0014Unravel
    0015 as Array0015Coalesce + Array0015Unravel
    0016 as Array0016Coalesce + Array0016Unravel
    0017 as Array0017Coalesce + Array0017Unravel
    0018 as Array0018Coalesce + Array0018Unravel
    0019 as Array0019Coalesce + Array0019Unravel
    0020 as Array0020Coalesce + Array0020Unravel
    0021 as Array0021Coalesce + Array0021Unravel
    0022 as Array0022Coalesce + Array0022Unravel
    0023 as Array0023Coalesce + Array0023Unravel
    0024 as Array0024Coalesce + Array0024Unravel
    0025 as Array0025Coalesce + Array0025Unravel
    0026 as Array0026Coalesce + Array0026Unravel
    0027 as Array0027Coalesce + Array0027Unravel
    0028 as Array0028Coalesce + Array0028Unravel
    0029 as Array0029Coalesce + Array0029Unravel
    0030 as Array0030Coalesce + Array0030Unravel
    0031 as Array0031Coalesce + Array0031Unravel
    0032 as Array0032Coalesce + Array0032Unravel
    0064 as Array0064Coalesce + Array0064Unravel
    0128 as Array0128Coalesce + Array0128Unravel
    0256 as Array0256Coalesce + Array0256Unravel
    0512 as Array0512Coalesce + Array0512Unravel
    1024 as Array1024Coalesce + Array1024Unravel
    2048 as Array2048Coalesce + Array2048Unravel
    4096 as Array4096Coalesce + Array4096Unravel
    8192 as Array8192Coalesce + Array8192Unravel
}
