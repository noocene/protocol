use crate::{
    future::{
        finalize::{EventualFinalize, EventualFinalizeError, TupleFinalizeInner},
        Finalize, FutureExt, MapErr, MapOk,
    },
    Coalesce, Dispatch, Fork, Future, Join, Read, Unravel, Write,
};
use core::{
    borrow::BorrowMut,
    mem::replace,
    pin::Pin,
    task::{Context, Poll},
};
use core_error::Error;
use futures::ready;
use thiserror::Error;

macro_rules! tuple_impls {
    ($($coalesce:ident $cstate:ident $unravel:ident $ustate:ident $u_error:ident $c_error:ident => ($first_n:tt ($first:ident $second:ident) $($n:tt ($ty:ident $next:ident $next_n:tt))+) $last_n:tt $last:ident )+) => {
        $(
            pub enum $ustate<$first, $($ty,)+ $last> {
                None,
                $first($first),
                $last($last),
                $($ty($ty),)+
                Writing,
                Flushing,
                Target,
                Done
            }

            pub enum $cstate<$first, $($ty,)+ $last> {
                Reading,
                $first($first),
                $($ty($ty),)+
                $last($last),
                Done,
            }

            #[derive(Debug, Error)]
            #[bounds(
                where
                    E: Error + 'static,
                    $first: Error + 'static,
                    $($ty: Error + 'static,)+
                    $last: Error + 'static,
                    T: Error + 'static,
                    U: Error + 'static,
            )]
            pub enum $u_error<E, $first, $($ty,)+ $last, T, U> {
                #[error("failed to write handle for tuple")]
                Transport(#[source] E),
                #[error("failed to fork element in tuple")]
                $first(#[source] $first),
                #[error("failed to fork element in tuple")]
                $last(#[source] $last),
                $(
                    #[error("failed to fork element in tuple")]
                    $ty(#[source] $ty),
                )+
                #[error("failed to target element in tuple")]
                Target(#[source] T),
                #[error("failed to finalize element in tuple")]
                Finalize(#[source] U)
            }

            #[derive(Debug, Error)]
            #[bounds(
                where
                    E: Error + 'static,
                    $first: Error + 'static,
                    $($ty: Error + 'static,)+
                    $last: Error + 'static,
            )]
            pub enum $c_error<E, $first, $($ty,)+ $last> {
                #[error("failed to read handle for tuple")]
                Transport(#[source] E),
                #[error("failed to join element in tuple")]
                $first(#[source] $first),
                #[error("failed to join element in tuple")]
                $last(#[source] $last),
                $(
                    #[error("failed to join element in tuple")]
                    $ty(#[source] $ty),
                )+
            }

            pub struct $unravel<
                $first: Unpin,
                $($ty: Unpin,)+
                $last: Unpin,
                C: ?Sized
                    + Write<(<C as Dispatch<$first>>::Handle, $(<C as Dispatch<$ty>>::Handle,)+ <C as Dispatch<$last>>::Handle)>
                    + Fork<$first>
                    + Fork<$last>
                    $(+ Fork<$ty>)++
                    Unpin,
            > {
                handles: (
                    Option<<C as Dispatch<$first>>::Handle>,
                    $(Option<<C as Dispatch<$ty>>::Handle>,)+
                    Option<<C as Dispatch<$last>>::Handle>,
                ),
                targets: EventualFinalize<C, (
                    Option<<C as Fork<$first>>::Target>,
                    $(Option<<C as Fork<$ty>>::Target>,)+
                    Option<<C as Fork<$last>>::Target>,
                ), (
                    $first,
                    $($ty,)+
                    $last,
                )>,
                data: (Option<$first>, $(Option<$ty>,)+ Option<$last>),
                state: $ustate<<C as Fork<$first>>::Future, $(<C as Fork<$ty>>::Future,)+ <C as Fork<$last>>::Future>
            }

            pub struct $coalesce<
                $first: Unpin,
                $($ty: Unpin,)+
                $last: Unpin,
                C: ?Sized
                    + Read<(<C as Dispatch<$first>>::Handle, $(<C as Dispatch<$ty>>::Handle,)+ <C as Dispatch<$last>>::Handle)>
                    + Join<$first>
                    + Join<$last>
                    $(+ Join<$ty>)++
                    Unpin,
            > {
                handles: (
                    Option<<C as Dispatch<$first>>::Handle>,
                    $(Option<<C as Dispatch<$ty>>::Handle>,)+
                    Option<<C as Dispatch<$last>>::Handle>,
                ),
                data: (Option<$first>, $(Option<$ty>,)+ Option<$last>),
                state: $cstate<<C as Join<$first>>::Future, $(<C as Join<$ty>>::Future,)+ <C as Join<$last>>::Future>
            }

            impl<
                    $first: Unpin, $($ty: Unpin,)+ $last: Unpin,
                    C: ?Sized
                        + Write<(<C as Dispatch<$first>>::Handle, $(<C as Dispatch<$ty>>::Handle,)+ <C as Dispatch<$last>>::Handle)>
                        + Fork<$first>
                        + Fork<$last>
                        $(+ Fork<$ty>)++
                        Unpin,
                > Future<C> for $unravel<$first, $($ty,)+ $last, C>
            where
                <C as Fork<$first>>::Future: Unpin,
                <C as Fork<$first>>::Target: Unpin,
                <C as Fork<$first>>::Finalize: Unpin,
                <C as Dispatch<$first>>::Handle: Unpin,
                <C as Fork<$last>>::Future: Unpin,
                <C as Fork<$last>>::Finalize: Unpin,
                <C as Fork<$last>>::Target: Unpin,
                <C as Dispatch<$last>>::Handle: Unpin,
                $(<C as Fork<$ty>>::Future: Unpin,)+
                $(<C as Fork<$ty>>::Finalize: Unpin,)+
                $(<C as Fork<$ty>>::Target: Unpin,)+
                $(<C as Dispatch<$ty>>::Handle: Unpin,)+
            {
                type Ok = MapErr<
                    TupleFinalizeInner<
                        (
                            Option<<C as Fork<$first>>::Finalize>,
                            $(Option<<C as Fork<$ty>>::Finalize>,)+
                            Option<<C as Fork<$last>>::Finalize>,
                        ),
                        ($first, $($ty,)+ $last),
                    >,
                    fn(
                        <TupleFinalizeInner<
                        (
                            Option<<C as Fork<$first>>::Finalize>,
                            $(Option<<C as Fork<$ty>>::Finalize>,)+
                            Option<<C as Fork<$last>>::Finalize>,
                        ),
                        ($first, $($ty,)+ $last),
                    > as Future<C>>::Error,
                    ) -> $u_error<
                    <C as Write<(<C as Dispatch<$first>>::Handle, $(<C as Dispatch<$ty>>::Handle,)+ <C as Dispatch<$last>>::Handle)>>::Error,
                    <<C as Fork<$first>>::Future as Future<C>>::Error,
                    $(<<C as Fork<$ty>>::Future as Future<C>>::Error,)+
                    <<C as Fork<$last>>::Future as Future<C>>::Error,
                    <Finalize<C, ($first,
                    $($ty,)+
                    $last)> as Future<C>>::Error,
                        <TupleFinalizeInner<
                        (
                            Option<<C as Fork<$first>>::Finalize>,
                            $(Option<<C as Fork<$ty>>::Finalize>,)+
                            Option<<C as Fork<$last>>::Finalize>,
                        ),
                        ($first, $($ty,)+ $last),
                    > as Future<C>>::Error,
                    >,
                >;
                type Error = $u_error<
                    <C as Write<(<C as Dispatch<$first>>::Handle, $(<C as Dispatch<$ty>>::Handle,)+ <C as Dispatch<$last>>::Handle)>>::Error,
                    <<C as Fork<$first>>::Future as Future<C>>::Error,
                    $(<<C as Fork<$ty>>::Future as Future<C>>::Error,)+
                    <<C as Fork<$last>>::Future as Future<C>>::Error,
                    <Finalize<C, ($first,
                    $($ty,)+
                    $last)> as Future<C>>::Error,
                    <TupleFinalizeInner<
                        (
                            Option<<C as Fork<$first>>::Finalize>,
                            $(Option<<C as Fork<$ty>>::Finalize>,)+
                            Option<<C as Fork<$last>>::Finalize>,
                        ),
                        ($first, $($ty,)+ $last),
                    > as Future<C>>::Error
                >;

                fn poll<R: BorrowMut<C>>(
                    mut self: Pin<&mut Self>,
                    cx: &mut Context,
                    mut ctx: R,
                ) -> Poll<Result<Self::Ok, Self::Error>> {
                    let ctx = ctx.borrow_mut();

                    let this = &mut *self;

                    loop {
                        match &mut this.state {
                            $ustate::None => {
                                replace(
                                    &mut this.state,
                                    $ustate::$first(
                                        ctx.fork(this.data.0.take().expect("data incomplete")),
                                    ),
                                );
                            }
                            $ustate::$first(future) => {
                                let (target, handle) = ready!(Pin::new(future).poll(cx, &mut *ctx))
                                    .map_err($u_error::$first)?;
                                this.handles.0 = Some(handle);
                                this.targets.data().unwrap().0 = Some(target);
                                replace(&mut this.state, $ustate::$second(ctx.fork(this.data.1.take().expect("data incomplete"))));
                            }
                            $($ustate::$ty(future) => {
                                let (target, handle) = ready!(Pin::new(future).poll(cx, &mut *ctx))
                                    .map_err($u_error::$ty)?;
                                this.handles.$n = Some(handle);
                                this.targets.data().unwrap().$n = Some(target);

                                replace(&mut this.state, $ustate::$next(ctx.fork(this.data.$next_n.take().expect("data incomplete"))));
                            })+
                            $ustate::$last(future) => {
                                let (target, handle) = ready!(Pin::new(future).poll(cx, &mut *ctx))
                                    .map_err($u_error::$last)?;
                                this.handles.$last_n = Some(handle);
                                this.targets.data().unwrap().$last_n = Some(target);

                                replace(&mut this.state, $ustate::Writing);
                            }
                            $ustate::Writing => {
                                let mut ct = Pin::new(&mut *ctx);
                                ready!(ct.as_mut().poll_ready(cx)).map_err($u_error::Transport)?;
                                ct.write((
                                    this.handles.0.take().expect("handles incomplete"),
                                    $(this.handles.$n.take().expect("handles incomplete"),)+
                                    this.handles.$last_n.take().expect("handles incomplete"),
                                ))
                                .map_err($u_error::Transport)?;
                                replace(&mut this.state, $ustate::Flushing);
                            }
                            $ustate::Flushing => {
                                let mut ct = Pin::new(&mut *ctx);
                                ready!(ct.as_mut().poll_flush(cx)).map_err($u_error::Transport)?;
                                this.targets.complete();
                                replace(&mut this.state, $ustate::Target);
                            }
                            $ustate::Target => {
                                let finalize = ready!(Pin::new(&mut this.targets).poll(cx, &mut *ctx))
                                    .map_err(EventualFinalizeError::unwrap_complete)
                                    .map_err($u_error::Target)?;
                                replace(&mut this.state, $ustate::Done);
                                return Poll::Ready(Ok(FutureExt::<C>::map_err(finalize, $u_error::Finalize)));
                            }
                            $ustate::Done => panic!("Tuple unravel polled after completion"),
                        }
                    }
                }
            }

            impl<
                    $first: Unpin, $($ty: Unpin,)+ $last: Unpin,
                    C: ?Sized
                        + Read<(<C as Dispatch<$first>>::Handle, $(<C as Dispatch<$ty>>::Handle,)+ <C as Dispatch<$last>>::Handle)>
                        + Join<$first>
                        + Join<$last>
                        $(+ Join<$ty>)++
                        Unpin,
                > Future<C> for $coalesce<$first, $($ty,)+ $last, C>
            where
                <C as Join<$first>>::Future: Unpin,
                <C as Dispatch<$first>>::Handle: Unpin,
                <C as Join<$last>>::Future: Unpin,
                <C as Dispatch<$last>>::Handle: Unpin,
                $(<C as Join<$ty>>::Future: Unpin,)+
                $(<C as Dispatch<$ty>>::Handle: Unpin,)+
            {
                type Ok = ($first, $($ty,)+ $last);
                type Error = $c_error<
                    <C as Read<(<C as Dispatch<$first>>::Handle, $(<C as Dispatch<$ty>>::Handle,)+ <C as Dispatch<$last>>::Handle)>>::Error,
                    <<C as Join<$first>>::Future as Future<C>>::Error,
                    $(<<C as Join<$ty>>::Future as Future<C>>::Error,)+
                    <<C as Join<$last>>::Future as Future<C>>::Error,
                >;

                fn poll<R: BorrowMut<C>>(
                    mut self: Pin<&mut Self>,
                    cx: &mut Context,
                    mut ctx: R,
                ) -> Poll<Result<Self::Ok, Self::Error>> {
                    let ctx = ctx.borrow_mut();

                    let this = &mut *self;

                    loop {
                        match &mut this.state {
                            $cstate::Reading => {
                                let mut ct = Pin::new(&mut *ctx);
                                let handles = ready!(ct.as_mut().read(cx)).map_err($c_error::Transport)?;
                                let first = handles.0;
                                this.handles = (None, $(Some(handles.$n),)+ Some(handles.$last_n));
                                replace(
                                    &mut this.state,
                                    $cstate::$first(<C as Join<$first>>::join(ctx, first)),
                                );
                            }
                            $cstate::$first(future) => {
                                this.data.0 = Some(
                                    ready!(Pin::new(future).poll(cx, &mut *ctx))
                                        .map_err($c_error::$first)?,
                                );

                                replace(
                                    &mut this.state,
                                    $cstate::$second(<C as Join<$second>>::join(
                                        ctx,
                                        this.handles.1.take().expect("handles incomplete"),
                                    )),
                                );
                            }
                            $($cstate::$ty(future) => {
                                this.data.$n = Some(
                                    ready!(Pin::new(future).poll(cx, &mut *ctx))
                                        .map_err($c_error::$ty)?,
                                );

                                replace(
                                    &mut this.state,
                                    $cstate::$next(<C as Join<$next>>::join(
                                        ctx,
                                        this.handles.$next_n.take().expect("handles incomplete"),
                                    )),
                                );
                            })+
                            $cstate::$last(future) => {
                                let data = ready!(Pin::new(future).poll(cx, &mut *ctx))
                                    .map_err($c_error::$last)?;

                                replace(&mut this.state, $cstate::Done);

                                return Poll::Ready(Ok((this.data.0.take().expect("data incomplete"), $(this.data.$n.take().expect("data incomplete"),)+ data)));
                            }
                            $cstate::Done => panic!("Tuple coalesce polled after completion"),
                        }
                    }
                }
            }

            impl<
                    $first: Unpin, $($ty: Unpin,)+ $last: Unpin,
                    C: ?Sized
                        + Write<(<C as Dispatch<$first>>::Handle, $(<C as Dispatch<$ty>>::Handle,)+ <C as Dispatch<$last>>::Handle)>
                        + Fork<$first>
                        + Fork<$last>
                        $(+ Fork<$ty>)++
                        Unpin,
                > Unravel<C> for ($first, $($ty,)+ $last)
            where
                <C as Fork<$first>>::Future: Unpin,
                <C as Fork<$first>>::Target: Unpin,
                <C as Fork<$first>>::Finalize: Unpin,
                <C as Dispatch<$first>>::Handle: Unpin,
                <C as Fork<$last>>::Future: Unpin,
                <C as Fork<$last>>::Target: Unpin,
                <C as Fork<$last>>::Finalize: Unpin,
                <C as Dispatch<$last>>::Handle: Unpin,
                $(<C as Fork<$ty>>::Future: Unpin,)+
                $(<C as Fork<$ty>>::Finalize: Unpin,)+
                $(<C as Fork<$ty>>::Target: Unpin,)+
                $(<C as Dispatch<$ty>>::Handle: Unpin,)+
            {
                type Finalize = MapErr<
                    TupleFinalizeInner<
                        (
                            Option<<C as Fork<$first>>::Finalize>,
                            $(Option<<C as Fork<$ty>>::Finalize>,)+
                            Option<<C as Fork<$last>>::Finalize>,
                        ),
                        ($first, $($ty,)+ $last),
                    >,
                    fn(
                        <TupleFinalizeInner<
                        (
                            Option<<C as Fork<$first>>::Finalize>,
                            $(Option<<C as Fork<$ty>>::Finalize>,)+
                            Option<<C as Fork<$last>>::Finalize>,
                        ),
                        ($first, $($ty,)+ $last),
                    > as Future<C>>::Error,
                    ) -> $u_error<
                    <C as Write<(<C as Dispatch<$first>>::Handle, $(<C as Dispatch<$ty>>::Handle,)+ <C as Dispatch<$last>>::Handle)>>::Error,
                    <<C as Fork<$first>>::Future as Future<C>>::Error,
                    $(<<C as Fork<$ty>>::Future as Future<C>>::Error,)+
                    <<C as Fork<$last>>::Future as Future<C>>::Error,
                    <Finalize<C, ($first,
                    $($ty,)+
                    $last)> as Future<C>>::Error,
                        <TupleFinalizeInner<
                        (
                            Option<<C as Fork<$first>>::Finalize>,
                            $(Option<<C as Fork<$ty>>::Finalize>,)+
                            Option<<C as Fork<$last>>::Finalize>,
                        ),
                        ($first, $($ty,)+ $last),
                    > as Future<C>>::Error,
                    >,
                >;

                type Target = $unravel<$first, $($ty,)+ $last, C>;

                fn unravel(self) -> Self::Target {
                    $unravel {
                        data: (Some(self.0), $(Some(self.$n),)+ Some(self.$last_n)),
                        state: $ustate::None,
                        handles: (None::<<C as Dispatch<$first>>::Handle>, $(None::<<C as Dispatch<$ty>>::Handle>,)+ None::<<C as Dispatch<$last>>::Handle>),
                        targets: EventualFinalize::new((None::<<C as Fork<$first>>::Target>, $(None::<<C as Fork<$ty>>::Target>,)+ None::<<C as Fork<$last>>::Target>)),
                    }
                }
            }

            impl<
                    $first: Unpin, $($ty: Unpin,)+ $last: Unpin,
                    C: ?Sized
                        + Read<(<C as Dispatch<$first>>::Handle, $(<C as Dispatch<$ty>>::Handle,)+ <C as Dispatch<$last>>::Handle)>
                        + Join<$first>
                        + Join<$last>
                        $(+ Join<$ty>)++
                        Unpin,
                > Coalesce<C> for ($first, $($ty,)+ $last)
            where
                <C as Join<$first>>::Future: Unpin,
                <C as Dispatch<$first>>::Handle: Unpin,
                <C as Join<$last>>::Future: Unpin,
                <C as Dispatch<$last>>::Handle: Unpin,
                $(<C as Join<$ty>>::Future: Unpin,)+
                $(<C as Dispatch<$ty>>::Handle: Unpin,)+
            {
                type Future = $coalesce<$first, $($ty,)+ $last, C>;

                fn coalesce() -> Self::Future {
                    $coalesce {
                        data: (None::<$first>, $(None::<$ty>,)+ None::<$last>),
                        state: $cstate::Reading,
                        handles: (None::<<C as Dispatch<$first>>::Handle>, $(None::<<C as Dispatch<$ty>>::Handle>,)+ None::<<C as Dispatch<$last>>::Handle>),
                    }
                }
            }
        )+
    }
}

enum Tuple2UnravelState<T, U> {
    None,
    T(T),
    U(U),
    Writing,
    Flushing,
    Target,
    Done,
}

enum Tuple2CoalesceState<T, U> {
    Reading,
    T(T),
    U(U),
    Done,
}

#[derive(Debug, Error)]
#[bounds(
    where
        T: Error + 'static,
        U: Error + 'static,
        V: Error + 'static,
        W: Error + 'static,
        X: Error + 'static,
)]
pub enum Tuple2UnravelError<T, U, V, W, X> {
    #[error("failed to write handle for tuple: {0}")]
    Transport(#[source] T),
    #[error("failed to fork element in tuple: {0}")]
    DispatchT(#[source] U),
    #[error("failed to fork element in tuple: {0}")]
    DispatchU(#[source] V),
    #[error("failed to target element in tuple: {0}")]
    Target(#[source] W),
    #[error("failed to finalize element in tuple: {0}")]
    Finalize(#[source] X),
}

#[derive(Debug, Error)]
#[bounds(
    where
        T: Error + 'static,
        U: Error + 'static,
        V: Error + 'static,
)]
pub enum Tuple2CoalesceError<T, U, V> {
    #[error("failed to read handle for tuple: {0}")]
    Transport(#[source] T),
    #[error("failed to join element in tuple: {0}")]
    DispatchT(#[source] U),
    #[error("failed to join element in tuple: {0}")]
    DispatchU(#[source] V),
}

pub struct Tuple2Unravel<
    T: Unpin,
    U: Unpin,
    C: ?Sized
        + Write<(<C as Dispatch<T>>::Handle, <C as Dispatch<U>>::Handle)>
        + Fork<T>
        + Fork<U>
        + Unpin,
> {
    handles: (
        Option<<C as Dispatch<T>>::Handle>,
        Option<<C as Dispatch<U>>::Handle>,
    ),
    targets: EventualFinalize<
        C,
        (
            Option<<C as Fork<T>>::Target>,
            Option<<C as Fork<U>>::Target>,
        ),
        (T, U),
    >,
    data: (Option<T>, Option<U>),
    state: Tuple2UnravelState<<C as Fork<T>>::Future, <C as Fork<U>>::Future>,
}

pub struct Tuple2Coalesce<
    T: Unpin,
    U: Unpin,
    C: ?Sized
        + Read<(<C as Dispatch<T>>::Handle, <C as Dispatch<U>>::Handle)>
        + Join<T>
        + Join<U>
        + Unpin,
> {
    handles: (
        Option<<C as Dispatch<T>>::Handle>,
        Option<<C as Dispatch<U>>::Handle>,
    ),
    data: (Option<T>, Option<U>),
    state: Tuple2CoalesceState<<C as Join<T>>::Future, <C as Join<U>>::Future>,
}

impl<
    T: Unpin,
    U: Unpin,
    C: ?Sized
        + Write<(<C as Dispatch<T>>::Handle, <C as Dispatch<U>>::Handle)>
        + Fork<T>
        + Fork<U>
        + Unpin,
> Future<C> for Tuple2Unravel<T, U, C>
where
    <C as Fork<T>>::Future: Unpin,
    <C as Fork<U>>::Future: Unpin,
    <C as Fork<T>>::Target: Unpin,
    <C as Fork<U>>::Target: Unpin,
    <C as Fork<T>>::Finalize: Unpin,
    <C as Fork<U>>::Finalize: Unpin,
    <C as Dispatch<T>>::Handle: Unpin,
    <C as Dispatch<U>>::Handle: Unpin,
{
    type Ok = MapErr<
        TupleFinalizeInner<
            (
                Option<<C as Fork<T>>::Finalize>,
                Option<<C as Fork<U>>::Finalize>,
            ),
            (T, U),
        >,
        fn(
            <TupleFinalizeInner<
                (
                    Option<<C as Fork<T>>::Finalize>,
                    Option<<C as Fork<U>>::Finalize>,
                ),
                (T, U),
            > as Future<C>>::Error,
        ) -> Tuple2UnravelError<
            <C as Write<(<C as Dispatch<T>>::Handle, <C as Dispatch<U>>::Handle)>>::Error,
            <<C as Fork<T>>::Future as Future<C>>::Error,
            <<C as Fork<U>>::Future as Future<C>>::Error,
            <Finalize<C, (T, U)> as Future<C>>::Error,
            <TupleFinalizeInner<
                (
                    Option<<C as Fork<T>>::Finalize>,
                    Option<<C as Fork<U>>::Finalize>,
                ),
                (T, U),
            > as Future<C>>::Error,
        >,
    >;
    type Error = Tuple2UnravelError<
        <C as Write<(<C as Dispatch<T>>::Handle, <C as Dispatch<U>>::Handle)>>::Error,
        <<C as Fork<T>>::Future as Future<C>>::Error,
        <<C as Fork<U>>::Future as Future<C>>::Error,
        <Finalize<C, (T, U)> as Future<C>>::Error,
        <TupleFinalizeInner<
            (
                Option<<C as Fork<T>>::Finalize>,
                Option<<C as Fork<U>>::Finalize>,
            ),
            (T, U),
        > as Future<C>>::Error,
    >;

    fn poll<R: BorrowMut<C>>(
        mut self: Pin<&mut Self>,
        cx: &mut Context,
        mut ctx: R,
    ) -> Poll<Result<Self::Ok, Self::Error>> {
        let ctx = ctx.borrow_mut();

        let this = &mut *self;

        loop {
            match &mut this.state {
                Tuple2UnravelState::None => {
                    replace(
                        &mut this.state,
                        Tuple2UnravelState::T(
                            ctx.fork(this.data.0.take().expect("data incomplete")),
                        ),
                    );
                }
                Tuple2UnravelState::T(future) => {
                    let (target, handle) = ready!(Pin::new(future).poll(cx, &mut *ctx))
                        .map_err(Tuple2UnravelError::DispatchT)?;
                    this.handles.0 = Some(handle);
                    this.targets.data().unwrap().0 = Some(target);
                    replace(
                        &mut this.state,
                        Tuple2UnravelState::U(
                            ctx.fork(this.data.1.take().expect("data incomplete")),
                        ),
                    );
                }
                Tuple2UnravelState::U(future) => {
                    let (target, handle) = ready!(Pin::new(future).poll(cx, &mut *ctx))
                        .map_err(Tuple2UnravelError::DispatchU)?;
                    this.handles.1 = Some(handle);
                    this.targets.data().unwrap().1 = Some(target);
                    replace(&mut this.state, Tuple2UnravelState::Writing);
                }
                Tuple2UnravelState::Writing => {
                    let mut ct = Pin::new(&mut *ctx);
                    ready!(ct.as_mut().poll_ready(cx)).map_err(Tuple2UnravelError::Transport)?;
                    ct.write((
                        this.handles.0.take().expect("handles incomplete"),
                        this.handles.1.take().expect("handles incomplete"),
                    ))
                    .map_err(Tuple2UnravelError::Transport)?;
                    replace(&mut this.state, Tuple2UnravelState::Flushing);
                }
                Tuple2UnravelState::Flushing => {
                    let mut ct = Pin::new(&mut *ctx);
                    ready!(ct.as_mut().poll_flush(cx)).map_err(Tuple2UnravelError::Transport)?;
                    this.targets.complete();
                    replace(&mut this.state, Tuple2UnravelState::Target);
                }
                Tuple2UnravelState::Target => {
                    let finalize = ready!(Pin::new(&mut this.targets).poll(cx, &mut *ctx))
                        .map_err(EventualFinalizeError::unwrap_complete)
                        .map_err(Tuple2UnravelError::Target)?;
                    replace(&mut this.state, Tuple2UnravelState::Done);
                    return Poll::Ready(Ok(FutureExt::<C>::map_err(
                        finalize,
                        Tuple2UnravelError::Finalize,
                    )));
                }
                Tuple2UnravelState::Done => panic!("Tuple unravel polled after completion"),
            }
        }
    }
}

impl<
    T: Unpin,
    U: Unpin,
    C: ?Sized
        + Read<(<C as Dispatch<T>>::Handle, <C as Dispatch<U>>::Handle)>
        + Join<T>
        + Join<U>
        + Unpin,
> Future<C> for Tuple2Coalesce<T, U, C>
where
    <C as Join<T>>::Future: Unpin,
    <C as Join<U>>::Future: Unpin,
    <C as Dispatch<T>>::Handle: Unpin,
    <C as Dispatch<U>>::Handle: Unpin,
{
    type Ok = (T, U);
    type Error = Tuple2CoalesceError<
        <C as Read<(<C as Dispatch<T>>::Handle, <C as Dispatch<U>>::Handle)>>::Error,
        <<C as Join<T>>::Future as Future<C>>::Error,
        <<C as Join<U>>::Future as Future<C>>::Error,
    >;

    fn poll<R: BorrowMut<C>>(
        mut self: Pin<&mut Self>,
        cx: &mut Context,
        mut ctx: R,
    ) -> Poll<Result<Self::Ok, Self::Error>> {
        let ctx = ctx.borrow_mut();

        let this = &mut *self;

        loop {
            match &mut this.state {
                Tuple2CoalesceState::Reading => {
                    let mut ct = Pin::new(&mut *ctx);
                    let handles =
                        ready!(ct.as_mut().read(cx)).map_err(Tuple2CoalesceError::Transport)?;
                    let first = handles.0;
                    this.handles = (None, Some(handles.1));
                    replace(
                        &mut this.state,
                        Tuple2CoalesceState::T(<C as Join<T>>::join(ctx, first)),
                    );
                }
                Tuple2CoalesceState::T(future) => {
                    this.data.0 = Some(
                        ready!(Pin::new(future).poll(cx, &mut *ctx))
                            .map_err(Tuple2CoalesceError::DispatchT)?,
                    );

                    replace(
                        &mut this.state,
                        Tuple2CoalesceState::U(<C as Join<U>>::join(
                            ctx,
                            this.handles.1.take().expect("handles incomplete"),
                        )),
                    );
                }
                Tuple2CoalesceState::U(future) => {
                    let data = ready!(Pin::new(future).poll(cx, &mut *ctx))
                        .map_err(Tuple2CoalesceError::DispatchU)?;

                    replace(&mut this.state, Tuple2CoalesceState::Done);

                    return Poll::Ready(Ok((this.data.0.take().expect("data incomplete"), data)));
                }
                Tuple2CoalesceState::Done => panic!("Tuple coalesce polled after completion"),
            }
        }
    }
}

impl<
    T: Unpin,
    U: Unpin,
    C: ?Sized
        + Write<(<C as Dispatch<T>>::Handle, <C as Dispatch<U>>::Handle)>
        + Fork<T>
        + Fork<U>
        + Unpin,
> Unravel<C> for (T, U)
where
    <C as Fork<T>>::Future: Unpin,
    <C as Fork<U>>::Future: Unpin,
    <C as Fork<T>>::Target: Unpin,
    <C as Fork<U>>::Target: Unpin,
    <C as Fork<T>>::Finalize: Unpin,
    <C as Fork<U>>::Finalize: Unpin,
    <C as Dispatch<T>>::Handle: Unpin,
    <C as Dispatch<U>>::Handle: Unpin,
{
    type Finalize = MapErr<
        TupleFinalizeInner<
            (
                Option<<C as Fork<T>>::Finalize>,
                Option<<C as Fork<U>>::Finalize>,
            ),
            (T, U),
        >,
        fn(
            <TupleFinalizeInner<
                (
                    Option<<C as Fork<T>>::Finalize>,
                    Option<<C as Fork<U>>::Finalize>,
                ),
                (T, U),
            > as Future<C>>::Error,
        ) -> Tuple2UnravelError<
            <C as Write<(<C as Dispatch<T>>::Handle, <C as Dispatch<U>>::Handle)>>::Error,
            <<C as Fork<T>>::Future as Future<C>>::Error,
            <<C as Fork<U>>::Future as Future<C>>::Error,
            <Finalize<C, (T, U)> as Future<C>>::Error,
            <TupleFinalizeInner<
                (
                    Option<<C as Fork<T>>::Finalize>,
                    Option<<C as Fork<U>>::Finalize>,
                ),
                (T, U),
            > as Future<C>>::Error,
        >,
    >;
    type Target = Tuple2Unravel<T, U, C>;

    fn unravel(self) -> Self::Target {
        Tuple2Unravel {
            data: (Some(self.0), Some(self.1)),
            handles: (None, None),
            targets: EventualFinalize::new((None, None)),
            state: Tuple2UnravelState::None,
        }
    }
}

impl<
    T: Unpin,
    U: Unpin,
    C: ?Sized
        + Read<(<C as Dispatch<T>>::Handle, <C as Dispatch<U>>::Handle)>
        + Join<T>
        + Join<U>
        + Unpin,
> Coalesce<C> for (T, U)
where
    <C as Join<T>>::Future: Unpin,
    <C as Join<U>>::Future: Unpin,
    <C as Dispatch<T>>::Handle: Unpin,
    <C as Dispatch<U>>::Handle: Unpin,
{
    type Future = Tuple2Coalesce<T, U, C>;

    fn coalesce() -> Self::Future {
        Tuple2Coalesce {
            data: (None, None),
            handles: (None, None),
            state: Tuple2CoalesceState::Reading,
        }
    }
}

pub enum FlatUnravelState<T, C: ?Sized + Write<<C as Dispatch<T>>::Handle> + Fork<T> + Unpin> {
    Data(T),
    Fork(C::Future),
    Write(C::Handle, C::Target),
    Flush(C::Target),
    Target(C::Target),
    Done,
}

pub struct FlatUnravel<T, C: ?Sized + Write<<C as Dispatch<T>>::Handle> + Fork<T> + Unpin> {
    state: FlatUnravelState<T, C>,
}

impl<T, C: ?Sized + Write<<C as Dispatch<T>>::Handle> + Fork<T> + Unpin> Unpin
    for FlatUnravel<T, C>
{
}

impl<T, C: ?Sized + Write<<C as Dispatch<T>>::Handle> + Fork<T> + Unpin> FlatUnravel<T, C> {
    pub fn new(item: T) -> Self {
        FlatUnravel {
            state: FlatUnravelState::Data(item),
        }
    }
}

pub enum FlatCoalesceState<T, C: ?Sized + Read<<C as Dispatch<T>>::Handle> + Join<T> + Unpin> {
    Read,
    Join(<C as Join<T>>::Future),
    Done,
}

impl<T, C: ?Sized + Read<<C as Dispatch<T>>::Handle> + Join<T> + Unpin> Unpin
    for FlatCoalesce<T, C>
{
}

pub struct FlatCoalesce<T, C: ?Sized + Read<<C as Dispatch<T>>::Handle> + Join<T> + Unpin> {
    state: FlatCoalesceState<T, C>,
}

impl<T, C: ?Sized + Read<<C as Dispatch<T>>::Handle> + Join<T> + Unpin> FlatCoalesce<T, C> {
    pub fn new() -> Self {
        FlatCoalesce {
            state: FlatCoalesceState::Read,
        }
    }
}

#[derive(Debug, Error)]
#[bounds(
    where
        T: Error + 'static,
        U: Error + 'static,
        V: Error + 'static,
)]
pub enum FlatUnravelError<T, U, V> {
    #[error("failed to write handle for tuple: {0}")]
    Transport(#[source] T),
    #[error("failed to fork element in tuple: {0}")]
    Dispatch(#[source] U),
    #[error("failed to finalize element in tuple: {0}")]
    Target(#[source] V),
}

#[derive(Debug, Error)]
#[bounds(
    where
        T: Error + 'static,
        U: Error + 'static,
)]
pub enum FlatCoalesceError<T, U> {
    #[error("failed to read handle for tuple: {0}")]
    Transport(#[source] T),
    #[error("failed to join element in tuple: {0}")]
    Dispatch(#[source] U),
}

impl<T, C: ?Sized + Write<<C as Dispatch<T>>::Handle> + Fork<T> + Unpin> Future<C>
    for FlatUnravel<T, C>
where
    C::Future: Unpin,
    C::Target: Unpin,
    C::Handle: Unpin,
{
    type Ok = MapErr<
        C::Finalize,
        fn(
            <C::Finalize as Future<C>>::Error,
        ) -> FlatUnravelError<
            C::Error,
            <C::Future as Future<C>>::Error,
            <C::Target as Future<C>>::Error,
        >,
    >;
    type Error = FlatUnravelError<
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
            match &mut this.state {
                FlatUnravelState::Data(_) => {
                    let data = replace(&mut this.state, FlatUnravelState::Done);
                    if let FlatUnravelState::Data(data) = data {
                        replace(&mut this.state, FlatUnravelState::Fork(ctx.fork(data)));
                    } else {
                        panic!("invalid state in FlatUnravel Data")
                    }
                }
                FlatUnravelState::Fork(future) => {
                    let (target, handle) = ready!(Pin::new(&mut *future).poll(cx, &mut *ctx))
                        .map_err(FlatUnravelError::Dispatch)?;
                    replace(&mut this.state, FlatUnravelState::Write(handle, target));
                }
                FlatUnravelState::Write(_, _) => {
                    let mut ctx = Pin::new(&mut *ctx);
                    ready!(ctx.as_mut().poll_ready(cx)).map_err(FlatUnravelError::Transport)?;
                    let data = replace(&mut this.state, FlatUnravelState::Done);
                    if let FlatUnravelState::Write(data, target) = data {
                        ctx.write(data).map_err(FlatUnravelError::Transport)?;
                        replace(&mut this.state, FlatUnravelState::Flush(target));
                    } else {
                        panic!("invalid state in FlatUnravel Write")
                    }
                }
                FlatUnravelState::Flush(_) => {
                    ready!(Pin::new(&mut *ctx).poll_flush(cx))
                        .map_err(FlatUnravelError::Transport)?;
                    let data = replace(&mut this.state, FlatUnravelState::Done);
                    if let FlatUnravelState::Flush(target) = data {
                        replace(&mut this.state, FlatUnravelState::Target(target));
                    } else {
                        panic!("invalid state in FlatUnravel Write")
                    }
                }
                FlatUnravelState::Target(target) => {
                    let finalize =
                        ready!(Pin::new(target).poll(cx, ctx)).map_err(FlatUnravelError::Target)?;
                    replace(&mut this.state, FlatUnravelState::Done);
                    return Poll::Ready(Ok(finalize.map_err(FlatUnravelError::Target)));
                }
                FlatUnravelState::Done => panic!("FlatUnravel polled after completion"),
            }
        }
    }
}

impl<T, C: ?Sized + Read<<C as Dispatch<T>>::Handle> + Join<T> + Unpin> Future<C>
    for FlatCoalesce<T, C>
where
    C::Future: Unpin,
    C::Handle: Unpin,
{
    type Ok = T;
    type Error = FlatCoalesceError<C::Error, <C::Future as Future<C>>::Error>;

    fn poll<R: BorrowMut<C>>(
        mut self: Pin<&mut Self>,
        cx: &mut Context,
        mut ctx: R,
    ) -> Poll<Result<Self::Ok, Self::Error>> {
        let ctx = ctx.borrow_mut();

        let this = &mut *self;

        loop {
            match &mut this.state {
                FlatCoalesceState::Read => {
                    let mut ctx = Pin::new(&mut *ctx);
                    let handle =
                        ready!(ctx.as_mut().read(cx)).map_err(FlatCoalesceError::Transport)?;
                    replace(&mut this.state, FlatCoalesceState::Join(ctx.join(handle)));
                }
                FlatCoalesceState::Join(future) => {
                    let item = ready!(Pin::new(future).poll(cx, &mut *ctx))
                        .map_err(FlatCoalesceError::Dispatch)?;
                    replace(&mut this.state, FlatCoalesceState::Done);
                    return Poll::Ready(Ok(item));
                }
                FlatCoalesceState::Done => panic!("FlatUnravel polled after completion"),
            }
        }
    }
}

impl<T, C: ?Sized + Write<<C as Dispatch<T>>::Handle> + Fork<T> + Unpin> Unravel<C> for (T,)
where
    C::Future: Unpin,
    C::Target: Unpin,
    C::Finalize: Unpin,
    C::Handle: Unpin,
{
    type Finalize = MapErr<
        C::Finalize,
        fn(
            <C::Finalize as Future<C>>::Error,
        ) -> FlatUnravelError<
            C::Error,
            <C::Future as Future<C>>::Error,
            <C::Target as Future<C>>::Error,
        >,
    >;
    type Target = FlatUnravel<T, C>;

    fn unravel(self) -> Self::Target {
        FlatUnravel::new(self.0)
    }
}

impl<T, C: ?Sized + Read<<C as Dispatch<T>>::Handle> + Join<T> + Unpin> Coalesce<C> for (T,)
where
    C::Future: Unpin,
    C::Handle: Unpin,
{
    type Future = MapOk<FlatCoalesce<T, C>, fn(T) -> (T,)>;

    fn coalesce() -> Self::Future {
        FlatCoalesce::new().map_ok(|item| (item,))
    }
}

tuple_impls! {
    Tuple3Coalesce  Tuple3CoalesceState  Tuple3Unravel  Tuple3UnravelState  Tuple3UnravelError  Tuple3CoalesceError  => (0 (T0 T1) 1 (T1 T2 2)) 2 T2
    Tuple4Coalesce  Tuple4CoalesceState  Tuple4Unravel  Tuple4UnravelState  Tuple4UnravelError  Tuple4CoalesceError  => (0 (T0 T1) 1 (T1 T2 2) 2 (T2 T3 3)) 3 T3
    Tuple5Coalesce  Tuple5CoalesceState  Tuple5Unravel  Tuple5UnravelState  Tuple5UnravelError  Tuple5CoalesceError  => (0 (T0 T1) 1 (T1 T2 2) 2 (T2 T3 3) 3 (T3 T4 4)) 4 T4
    Tuple6Coalesce  Tuple6CoalesceState  Tuple6Unravel  Tuple6UnravelState  Tuple6UnravelError  Tuple6CoalesceError  => (0 (T0 T1) 1 (T1 T2 2) 2 (T2 T3 3) 3 (T3 T4 4) 4 (T4 T5 5)) 5 T5
    Tuple7Coalesce  Tuple7CoalesceState  Tuple7Unravel  Tuple7UnravelState  Tuple7UnravelError  Tuple7CoalesceError  => (0 (T0 T1) 1 (T1 T2 2) 2 (T2 T3 3) 3 (T3 T4 4) 4 (T4 T5 5) 5 (T5 T6 6)) 6 T6
    Tuple8Coalesce  Tuple8CoalesceState  Tuple8Unravel  Tuple8UnravelState  Tuple8UnravelError  Tuple8CoalesceError  => (0 (T0 T1) 1 (T1 T2 2) 2 (T2 T3 3) 3 (T3 T4 4) 4 (T4 T5 5) 5 (T5 T6 6) 6 (T6 T7 7)) 7 T7
    Tuple9Coalesce  Tuple9CoalesceState  Tuple9Unravel  Tuple9UnravelState  Tuple9UnravelError  Tuple9CoalesceError  => (0 (T0 T1) 1 (T1 T2 2) 2 (T2 T3 3) 3 (T3 T4 4) 4 (T4 T5 5) 5 (T5 T6 6) 6 (T6 T7 7) 7 (T7 T8 8)) 8 T8
    Tuple10Coalesce Tuple10CoalesceState Tuple10Unravel Tuple10UnravelState Tuple10UnravelError Tuple10CoalesceError => (0 (T0 T1) 1 (T1 T2 2) 2 (T2 T3 3) 3 (T3 T4 4) 4 (T4 T5 5) 5 (T5 T6 6) 6 (T6 T7 7) 7 (T7 T8 8) 8 (T8 T9 9)) 9 T9
    Tuple11Coalesce Tuple11CoalesceState Tuple11Unravel Tuple11UnravelState Tuple11UnravelError Tuple11CoalesceError => (0 (T0 T1) 1 (T1 T2 2) 2 (T2 T3 3) 3 (T3 T4 4) 4 (T4 T5 5) 5 (T5 T6 6) 6 (T6 T7 7) 7 (T7 T8 8) 8 (T8 T9 9) 9 (T9 T10 10)) 10 T10
    Tuple12Coalesce Tuple12CoalesceState Tuple12Unravel Tuple12UnravelState Tuple12UnravelError Tuple12CoalesceError => (0 (T0 T1) 1 (T1 T2 2) 2 (T2 T3 3) 3 (T3 T4 4) 4 (T4 T5 5) 5 (T5 T6 6) 6 (T6 T7 7) 7 (T7 T8 8) 8 (T8 T9 9) 9 (T9 T10 10) 10 (T10 T11 11)) 11 T11
    Tuple13Coalesce Tuple13CoalesceState Tuple13Unravel Tuple13UnravelState Tuple13UnravelError Tuple13CoalesceError => (0 (T0 T1) 1 (T1 T2 2) 2 (T2 T3 3) 3 (T3 T4 4) 4 (T4 T5 5) 5 (T5 T6 6) 6 (T6 T7 7) 7 (T7 T8 8) 8 (T8 T9 9) 9 (T9 T10 10) 10 (T10 T11 11) 11 (T11 T12 12)) 12 T12
    Tuple14Coalesce Tuple14CoalesceState Tuple14Unravel Tuple14UnravelState Tuple14UnravelError Tuple14CoalesceError => (0 (T0 T1) 1 (T1 T2 2) 2 (T2 T3 3) 3 (T3 T4 4) 4 (T4 T5 5) 5 (T5 T6 6) 6 (T6 T7 7) 7 (T7 T8 8) 8 (T8 T9 9) 9 (T9 T10 10) 10 (T10 T11 11) 11 (T11 T12 12) 12 (T12 T13 13)) 13 T13
    Tuple15Coalesce Tuple15CoalesceState Tuple15Unravel Tuple15UnravelState Tuple15UnravelError Tuple15CoalesceError => (0 (T0 T1) 1 (T1 T2 2) 2 (T2 T3 3) 3 (T3 T4 4) 4 (T4 T5 5) 5 (T5 T6 6) 6 (T6 T7 7) 7 (T7 T8 8) 8 (T8 T9 9) 9 (T9 T10 10) 10 (T10 T11 11) 11 (T11 T12 12) 12 (T12 T13 13) 13 (T13 T14 14)) 14 T14
    Tuple16Coalesce Tuple16CoalesceState Tuple16Unravel Tuple16UnravelState Tuple16UnravelError Tuple16CoalesceError => (0 (T0 T1) 1 (T1 T2 2) 2 (T2 T3 3) 3 (T3 T4 4) 4 (T4 T5 5) 5 (T5 T6 6) 6 (T6 T7 7) 7 (T7 T8 8) 8 (T8 T9 9) 9 (T9 T10 10) 10 (T10 T11 11) 11 (T11 T12 12) 12 (T12 T13 13) 13 (T13 T14 14) 14 (T14 T15 15)) 15 T15
}
