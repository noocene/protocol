mod error;
mod functions;
mod future;
mod stream;
mod vec;
use crate::{
    future::{FutureExt, MapOk},
    Coalesce, Unravel,
};
use alloc::boxed::Box;
use core::{
    fmt::{self, Debug, Formatter},
    future::Future,
};
use core_error::Error;
use thiserror::Error;

#[derive(Error)]
#[error(transparent)]
pub struct ProtocolError(Box<dyn Error + Send>);

impl<C: ?Sized> Unravel<C> for ProtocolError
where
    Box<dyn Error + Send>: Unravel<C>,
{
    type Finalize = <Box<dyn Error + Send> as Unravel<C>>::Finalize;
    type Target = <Box<dyn Error + Send> as Unravel<C>>::Target;

    fn unravel(self) -> Self::Target {
        self.0.unravel()
    }
}

impl<C: ?Sized> Coalesce<C> for ProtocolError
where
    Box<dyn Error + Send>: Coalesce<C>,
    <Box<dyn Error + Send> as Coalesce<C>>::Future: Unpin,
{
    type Future = MapOk<
        <Box<dyn Error + Send> as Coalesce<C>>::Future,
        fn(Box<dyn Error + Send>) -> ProtocolError,
    >;

    fn coalesce() -> Self::Future {
        Box::<dyn Error + Send>::coalesce().map_ok(ProtocolError)
    }
}

impl ProtocolError {
    pub fn new<T: Error + Send + 'static>(error: T) -> Self {
        ProtocolError(Box::new(error))
    }
}

impl Debug for ProtocolError {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        <dyn Error as Debug>::fmt(self.0.as_ref(), f)
    }
}

pub trait FromError<E> {
    fn from_error(error: E) -> Self;
}

impl<T, S, E: From<S>> FromError<S> for Result<T, E> {
    fn from_error(error: S) -> Self {
        Err(error.into())
    }
}

pub trait Flatten<E, F: Future<Output = Result<Self, E>>>: Sized {
    fn flatten(future: F) -> Self;
}
