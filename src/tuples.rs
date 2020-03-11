use crate::{
    future::{
        unordered::{EventualUnordered, EventualUnorderedError},
        Unordered,
    },
    ready, Coalesce, Dispatch, Fork, Future, Join, Read, Unravel, Write,
};
use core::{
    borrow::BorrowMut,
    mem::replace,
    pin::Pin,
    task::{Context, Poll},
};

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

            #[derive(Debug)]
            pub enum $u_error<E, $first, $($ty,)+ $last, T> {
                Transport(E),
                $first($first),
                $last($last),
                $($ty($ty),)+
                Target(T)
            }

            #[derive(Debug)]
            pub enum $c_error<E, $first, $($ty,)+ $last> {
                Transport(E),
                $first($first),
                $last($last),
                $($ty($ty),)+
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
                targets: EventualUnordered<(
                    Option<<C as Fork<$first>>::Target>,
                    $(Option<<C as Fork<$ty>>::Target>,)+
                    Option<<C as Fork<$last>>::Target>,
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
                <C as Dispatch<$first>>::Handle: Unpin,
                <C as Fork<$last>>::Future: Unpin,
                <C as Fork<$last>>::Target: Unpin,
                <C as Dispatch<$last>>::Handle: Unpin,
                $(<C as Fork<$ty>>::Future: Unpin,)+
                $(<C as Fork<$ty>>::Target: Unpin,)+
                $(<C as Dispatch<$ty>>::Handle: Unpin,)+
            {
                type Ok = ();
                type Error = $u_error<
                    <C as Write<(<C as Dispatch<$first>>::Handle, $(<C as Dispatch<$ty>>::Handle,)+ <C as Dispatch<$last>>::Handle)>>::Error,
                    <<C as Fork<$first>>::Future as Future<C>>::Error,
                    $(<<C as Fork<$ty>>::Future as Future<C>>::Error,)+
                    <<C as Fork<$last>>::Future as Future<C>>::Error,
                    <Unordered<(Option<<C as Fork<$first>>::Target>,
                    $(Option<<C as Fork<$ty>>::Target>,)+
                    Option<<C as Fork<$last>>::Target>)> as Future<C>>::Error
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
                                ready!(Pin::new(&mut this.targets).poll(cx, &mut *ctx))
                                    .map_err(EventualUnorderedError::unwrap_complete)
                                    .map_err($u_error::Target)?;
                                replace(&mut this.state, $ustate::Done);
                                return Poll::Ready(Ok(()));
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
                <C as Dispatch<$first>>::Handle: Unpin,
                <C as Fork<$last>>::Future: Unpin,
                <C as Fork<$last>>::Target: Unpin,
                <C as Dispatch<$last>>::Handle: Unpin,
                $(<C as Fork<$ty>>::Future: Unpin,)+
                $(<C as Fork<$ty>>::Target: Unpin,)+
                $(<C as Dispatch<$ty>>::Handle: Unpin,)+
            {
                type Future = $unravel<$first, $($ty,)+ $last, C>;

                fn unravel(self) -> Self::Future {
                    $unravel {
                        data: (Some(self.0), $(Some(self.$n),)+ Some(self.$last_n)),
                        state: $ustate::None,
                        handles: (None::<<C as Dispatch<$first>>::Handle>, $(None::<<C as Dispatch<$ty>>::Handle>,)+ None::<<C as Dispatch<$last>>::Handle>),
                        targets: EventualUnordered::new((None::<<C as Fork<$first>>::Target>, $(None::<<C as Fork<$ty>>::Target>,)+ None::<<C as Fork<$last>>::Target>)),
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

#[derive(Debug)]
pub enum Tuple2UnravelError<T, U, V, W> {
    Transport(T),
    DispatchT(U),
    DispatchU(V),
    Target(W),
}

#[derive(Debug)]
pub enum Tuple2CoalesceError<T, U, V> {
    Transport(T),
    DispatchT(U),
    DispatchU(V),
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
    targets: EventualUnordered<(
        Option<<C as Fork<T>>::Target>,
        Option<<C as Fork<U>>::Target>,
    )>,
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
    <C as Dispatch<T>>::Handle: Unpin,
    <C as Dispatch<U>>::Handle: Unpin,
{
    type Ok = ();
    type Error = Tuple2UnravelError<
        <C as Write<(<C as Dispatch<T>>::Handle, <C as Dispatch<U>>::Handle)>>::Error,
        <<C as Fork<T>>::Future as Future<C>>::Error,
        <<C as Fork<U>>::Future as Future<C>>::Error,
        <Unordered<(
            Option<<C as Fork<T>>::Target>,
            Option<<C as Fork<U>>::Target>,
        )> as Future<C>>::Error,
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
                    ready!(Pin::new(&mut this.targets).poll(cx, &mut *ctx))
                        .map_err(EventualUnorderedError::unwrap_complete)
                        .map_err(Tuple2UnravelError::Target)?;
                    replace(&mut this.state, Tuple2UnravelState::Done);
                    return Poll::Ready(Ok(()));
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
    <C as Dispatch<T>>::Handle: Unpin,
    <C as Dispatch<U>>::Handle: Unpin,
{
    type Future = Tuple2Unravel<T, U, C>;

    fn unravel(self) -> Self::Future {
        Tuple2Unravel {
            data: (Some(self.0), Some(self.1)),
            handles: (None, None),
            targets: EventualUnordered::new((None, None)),
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

pub enum Tuple1Unravel<T: Unpin, C: ?Sized + Write<<C as Dispatch<T>>::Handle> + Fork<T> + Unpin> {
    Data(T),
    Fork(C::Future),
    Write(C::Handle, C::Target),
    Flush(C::Target),
    Target(C::Target),
    Done,
}

pub enum Tuple1Coalesce<T: Unpin, C: ?Sized + Read<<C as Dispatch<T>>::Handle> + Join<T> + Unpin> {
    Read,
    Join(<C as Join<T>>::Future),
    Done,
}

#[derive(Debug)]
pub enum Tuple1UnravelError<T, U, V> {
    Transport(T),
    Dispatch(U),
    Target(V),
}

#[derive(Debug)]
pub enum Tuple1CoalesceError<T, U> {
    Transport(T),
    Dispatch(U),
}

impl<T: Unpin, C: ?Sized + Write<<C as Dispatch<T>>::Handle> + Fork<T> + Unpin> Future<C>
    for Tuple1Unravel<T, C>
where
    C::Future: Unpin,
    C::Target: Unpin,
    C::Handle: Unpin,
{
    type Ok = ();
    type Error = Tuple1UnravelError<
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
            match this {
                Tuple1Unravel::Data(_) => {
                    let data = replace(this, Tuple1Unravel::Done);
                    if let Tuple1Unravel::Data(data) = data {
                        replace(this, Tuple1Unravel::Fork(ctx.fork(data)));
                    } else {
                        panic!("invalid state in Tuple1Unravel Data")
                    }
                }
                Tuple1Unravel::Fork(future) => {
                    let (target, handle) = ready!(Pin::new(&mut *future).poll(cx, &mut *ctx))
                        .map_err(Tuple1UnravelError::Dispatch)?;
                    replace(this, Tuple1Unravel::Write(handle, target));
                }
                Tuple1Unravel::Write(_, _) => {
                    let mut ctx = Pin::new(&mut *ctx);
                    ready!(ctx.as_mut().poll_ready(cx)).map_err(Tuple1UnravelError::Transport)?;
                    let data = replace(this, Tuple1Unravel::Done);
                    if let Tuple1Unravel::Write(data, target) = data {
                        ctx.write(data).map_err(Tuple1UnravelError::Transport)?;
                        replace(this, Tuple1Unravel::Flush(target));
                    } else {
                        panic!("invalid state in Tuple1Unravel Write")
                    }
                }
                Tuple1Unravel::Flush(_) => {
                    ready!(Pin::new(&mut *ctx).poll_ready(cx))
                        .map_err(Tuple1UnravelError::Transport)?;
                    let data = replace(this, Tuple1Unravel::Done);
                    if let Tuple1Unravel::Flush(target) = data {
                        replace(this, Tuple1Unravel::Target(target));
                    } else {
                        panic!("invalid state in Tuple1Unravel Write")
                    }
                }
                Tuple1Unravel::Target(target) => {
                    ready!(Pin::new(target).poll(cx, ctx)).map_err(Tuple1UnravelError::Target)?;
                    replace(this, Tuple1Unravel::Done);
                    return Poll::Ready(Ok(()));
                }
                Tuple1Unravel::Done => panic!("Tuple1Unravel polled after completion"),
            }
        }
    }
}

impl<T: Unpin, C: ?Sized + Read<<C as Dispatch<T>>::Handle> + Join<T> + Unpin> Future<C>
    for Tuple1Coalesce<T, C>
where
    C::Future: Unpin,
    C::Handle: Unpin,
{
    type Ok = (T,);
    type Error = Tuple1CoalesceError<C::Error, <C::Future as Future<C>>::Error>;

    fn poll<R: BorrowMut<C>>(
        mut self: Pin<&mut Self>,
        cx: &mut Context,
        mut ctx: R,
    ) -> Poll<Result<Self::Ok, Self::Error>> {
        let ctx = ctx.borrow_mut();

        let this = &mut *self;

        loop {
            match this {
                Tuple1Coalesce::Read => {
                    let mut ctx = Pin::new(&mut *ctx);
                    let handle =
                        ready!(ctx.as_mut().read(cx)).map_err(Tuple1CoalesceError::Transport)?;
                    replace(this, Tuple1Coalesce::Join(ctx.join(handle)));
                }
                Tuple1Coalesce::Join(future) => {
                    return Poll::Ready(Ok((ready!(Pin::new(future).poll(cx, &mut *ctx))
                        .map_err(Tuple1CoalesceError::Dispatch)?,)));
                }
                Tuple1Coalesce::Done => panic!("Tuple1Unravel polled after completion"),
            }
        }
    }
}

impl<T: Unpin, C: ?Sized + Write<<C as Dispatch<T>>::Handle> + Fork<T> + Unpin> Unravel<C> for (T,)
where
    C::Future: Unpin,
    C::Target: Unpin,
    C::Handle: Unpin,
{
    type Future = Tuple1Unravel<T, C>;

    fn unravel(self) -> Self::Future {
        Tuple1Unravel::Data(self.0)
    }
}

impl<T: Unpin, C: ?Sized + Read<<C as Dispatch<T>>::Handle> + Join<T> + Unpin> Coalesce<C> for (T,)
where
    C::Future: Unpin,
    C::Handle: Unpin,
{
    type Future = Tuple1Coalesce<T, C>;

    fn coalesce() -> Self::Future {
        Tuple1Coalesce::Read
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
