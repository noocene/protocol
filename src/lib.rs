#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(feature = "alloc")]
extern crate alloc;

use core::{
    borrow::BorrowMut,
    pin::Pin,
    task::{Context as FContext, Poll},
};
use thiserror::Error;

mod arrays;
pub mod future;
pub use future::{Future, FutureExt};
pub mod adapter;
mod option;
mod util;
pub use util::*;
mod primitives;
mod result;
mod tuples;

pub use derive::protocol;
pub use tuples::{FlatCoalesce, FlatUnravel};

#[cfg(feature = "alloc")]
pub mod allocated;
#[cfg(feature = "alloc")]
pub use allocated::ProtocolError;

#[derive(Debug, Error)]
#[error("attempted to coalesce bottom type {0}")]
pub struct BottomCoalesce(pub &'static str);

pub trait Notify<T>:
    Fork<<Self as Notify<T>>::Notification> + Join<<Self as Notify<T>>::Notification>
{
    type Notification;
    type Wrap: Future<Self, Ok = Self::Notification>;
    type Unwrap: Future<Self, Ok = T>;

    fn wrap(&mut self, item: T) -> Self::Wrap;
    fn unwrap(&mut self, notification: Self::Notification) -> Self::Unwrap;
}

pub trait Unravel<C: ?Sized> {
    type Finalize: Future<C, Ok = (), Error = <Self::Target as Future<C>>::Error>;
    type Target: Future<C, Ok = Self::Finalize>;

    fn unravel(self) -> Self::Target;
}

pub trait Finalize<T: Future<Self::Target>> {
    type Target;
    type Output: Future<Self, Ok = ()>;

    fn finalize(&mut self, future: T) -> Self::Output;
}

pub trait FinalizeImmediate<T: Future<Self::Target>> {
    type Target;
    type Error;

    fn finalize_immediate(&mut self, future: T) -> Result<(), Self::Error>;
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

pub trait ContextReference<C: ?Sized> {
    type Target;

    fn with<'a, 'b: 'a, R: BorrowMut<C> + 'b>(&'a mut self, ctx: R) -> &'a mut Self::Target;
}

pub trait ReferenceContext: Contextualize {
    type Context: ContextReference<Self>;
    type JoinOutput: Future<Self, Ok = Self::Context>;
    type ForkOutput: Future<Self, Ok = (Self::Context, Self::Handle)>;

    fn join_ref(&mut self, handle: Self::Handle) -> Self::JoinOutput;
    fn fork_ref(&mut self) -> Self::ForkOutput;
}

pub trait Contextualize {
    type Handle;
}

pub trait CloneContext: Contextualize {
    type Context;
    type JoinOutput: Future<Self, Ok = Self::Context>;
    type ForkOutput: Future<Self, Ok = (Self::Context, Self::Handle)>;

    fn join_owned(&mut self, handle: Self::Handle) -> Self::JoinOutput;
    fn fork_owned(&mut self) -> Self::ForkOutput;
}

pub trait ShareContext: Contextualize {
    type Context: Clone;
    type JoinOutput: Future<Self, Ok = Self::Context>;
    type ForkOutput: Future<Self, Ok = (Self::Context, Self::Handle)>;

    fn join_shared(&mut self, handle: Self::Handle) -> Self::JoinOutput;
    fn fork_shared(&mut self) -> Self::ForkOutput;
}

pub trait Write<T> {
    type Error;

    fn poll_ready(self: Pin<&mut Self>, cx: &mut FContext) -> Poll<Result<(), Self::Error>>;
    fn write(self: Pin<&mut Self>, data: T) -> Result<(), Self::Error>;
    fn poll_flush(self: Pin<&mut Self>, cx: &mut FContext) -> Poll<Result<(), Self::Error>>;
}

impl<'a, T, U: Unpin + Write<T>> Write<T> for &'a mut U {
    type Error = U::Error;

    fn poll_ready(mut self: Pin<&mut Self>, cx: &mut FContext) -> Poll<Result<(), Self::Error>> {
        <U as Write<T>>::poll_ready(Pin::new(&mut **self), cx)
    }
    fn write(mut self: Pin<&mut Self>, data: T) -> Result<(), Self::Error> {
        <U as Write<T>>::write(Pin::new(&mut **self), data)
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut FContext) -> Poll<Result<(), Self::Error>> {
        <U as Write<T>>::poll_flush(Pin::new(&mut **self), cx)
    }
}

pub trait Read<T> {
    type Error;

    fn read(self: Pin<&mut Self>, cx: &mut FContext) -> Poll<Result<T, Self::Error>>;
}

impl<'a, T, U: Unpin + Read<T>> Read<T> for &'a mut U {
    type Error = U::Error;

    fn read(mut self: Pin<&mut Self>, cx: &mut FContext) -> Poll<Result<T, Self::Error>> {
        <U as Read<T>>::read(Pin::new(&mut **self), cx)
    }
}
