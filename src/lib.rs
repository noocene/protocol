#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(feature = "alloc")]
extern crate alloc;

use core::{
    borrow::BorrowMut,
    pin::Pin,
    task::{Context, Poll},
};
use futures::ready;
use void::Void;
mod option;
mod pass;
mod result;
pub use pass::Pass;

#[cfg(feature = "alloc")]
mod allocated;

pub trait Future<C: ?Sized> {
    type Ok;
    type Error;

    fn poll<R: BorrowMut<C>>(
        self: Pin<&mut Self>,
        cx: &mut Context,
        ctx: R,
    ) -> Poll<Result<Self::Ok, Self::Error>>;
}

pub struct Ready<T> {
    data: Option<T>,
}

impl<C: ?Sized, T: Unpin> Future<C> for Ready<T> {
    type Ok = T;
    type Error = Void;

    fn poll<R: BorrowMut<C>>(
        mut self: Pin<&mut Self>,
        _: &mut Context,
        _: R,
    ) -> Poll<Result<Self::Ok, Self::Error>> {
        Poll::Ready(Ok(self.data.take().expect("Ready polled after completion")))
    }
}

pub fn ready<T>(data: T) -> Ready<T> {
    Ready { data: Some(data) }
}

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
    type Future: Future<Self, Ok = Self::Handle>;

    fn fork(&mut self, item: P) -> Self::Future;
}

pub trait Join<P>: Dispatch<P> {
    type Future: Future<Self, Ok = P>;

    fn join(&mut self, handle: Self::Handle) -> Self::Future;
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
