#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(feature = "alloc")]
extern crate alloc;

use core::{
    future as cfuture,
    pin::Pin,
    task::{Context, Poll},
};
use futures::ready;
mod arrays;
pub mod future;
pub use future::Future;
mod option;
mod primitives;
mod result;
mod tuples;

#[cfg(feature = "alloc")]
pub mod allocated;

pub trait Unravel<C: ?Sized> {
    type Future: Future<C, Ok = ()>;

    fn unravel(self) -> Self::Future;
}

pub trait Coalesce<C: ?Sized>: Sized {
    type Future: Future<C, Ok = Self>;

    fn coalesce() -> Self::Future;
}

pub trait Dispatch<P> {
    type Handle;
}

pub trait Fork<P>: Dispatch<P> {
    type Target: Future<Self, Ok = ()>;
    type Future: Future<Self, Ok = (Self::Target, Self::Handle)>;

    fn fork(&mut self, item: P) -> Self::Future;
}

pub trait Join<P>: Dispatch<P> {
    type Future: Future<Self, Ok = P>;

    fn join(&mut self, handle: Self::Handle) -> Self::Future;
}

pub trait CoalesceContextualizer {
    type Target;
}

pub trait ContextualizeCoalesce<F: Future<Self::Target>>: CoalesceContextualizer {
    type Future: cfuture::Future<Output = Result<F::Ok, F::Error>>;
    type Output: Future<Self, Ok = Self::Future>;

    fn contextualize(&mut self, future: F) -> Self::Output;
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
