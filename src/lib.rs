#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(feature = "alloc")]
extern crate alloc;

use core::{
    borrow::BorrowMut,
    pin::Pin,
    task::{Context, Poll},
};
mod arrays;
pub mod future;
pub use future::{Future, FutureExt};
mod option;
mod primitives;
mod result;
mod tuples;

#[cfg(feature = "alloc")]
pub mod allocated;
#[cfg(feature = "alloc")]
pub use allocated::ProtocolError;

pub trait Notify<T>:
    Fork<<Self as Notify<T>>::Notification> + Join<<Self as Notify<T>>::Notification>
{
    type Notification;
    type Wrap: Future<Self, Ok = Self::Notification>;
    type Unwrap: Future<Self, Ok = T>;

    fn wrap(&mut self, item: T) -> Self::Wrap;
    fn unwrap(&mut self, notifiaction: Self::Notification) -> Self::Unwrap;
}

pub trait Unravel<C: ?Sized> {
    type Finalize: Future<C, Ok = (), Error = <Self::Target as Future<C>>::Error>;
    type Target: Future<C, Ok = Self::Finalize>;

    fn unravel(self) -> Self::Target;
}

pub trait Coalesce<C: ?Sized>: Sized {
    type Future: Future<C, Ok = Self>;

    fn coalesce() -> Self::Future;
}

pub trait Dispatch<P> {
    type Handle;
}

pub trait Fork<P>: Dispatch<P> {
    type Finalize: Future<Self, Ok = (), Error = <Self::Target as Future<Self>>::Error>;
    type Target: Future<Self, Ok = Self::Finalize>;
    type Future: Future<Self, Ok = (Self::Target, Self::Handle)>;

    fn fork(&mut self, item: P) -> Self::Future;
}

pub trait Join<P>: Dispatch<P> {
    type Future: Future<Self, Ok = P>;

    fn join(&mut self, handle: Self::Handle) -> Self::Future;
}

pub trait UnravelContext<C: ?Sized> {
    type Target;

    fn with<'a, 'b: 'a, R: BorrowMut<C> + 'b>(&'a mut self, ctx: R) -> &'a mut Self::Target;
}

pub trait ContextualizeUnravel: Contextualizer {
    type Context: UnravelContext<Self>;
    type Output: Future<Self, Ok = (Self::Context, Self::Handle)>;

    fn contextualize(&mut self) -> Self::Output;
}

pub trait Contextualizer {
    type Handle;
}

pub trait CoalesceContextualizer: Contextualizer {
    type Target;
}

pub trait ContextualizeCoalesce: CoalesceContextualizer {
    type Context;
    type Output: Future<Self, Ok = Self::Context>;

    fn contextualize(&mut self, handle: Self::Handle) -> Self::Output;
}

pub trait Write<T> {
    type Error;

    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), Self::Error>>;
    fn write(self: Pin<&mut Self>, data: T) -> Result<(), Self::Error>;
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), Self::Error>>;
}

impl<'a, T, U: Unpin + Write<T>> Write<T> for &'a mut U {
    type Error = U::Error;

    fn poll_ready(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        <U as Write<T>>::poll_ready(Pin::new(&mut **self), cx)
    }
    fn write(mut self: Pin<&mut Self>, data: T) -> Result<(), Self::Error> {
        <U as Write<T>>::write(Pin::new(&mut **self), data)
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        <U as Write<T>>::poll_flush(Pin::new(&mut **self), cx)
    }
}

pub trait Read<T> {
    type Error;

    fn read(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<T, Self::Error>>;
}

impl<'a, T, U: Unpin + Read<T>> Read<T> for &'a mut U {
    type Error = U::Error;

    fn read(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<T, Self::Error>> {
        <U as Read<T>>::read(Pin::new(&mut **self), cx)
    }
}
