mod futures;
mod vec;
use alloc::boxed::Box;
use core::fmt::{self, Debug, Formatter};
use core_error::Error;
use thiserror::Error;

#[derive(Error)]
#[error(transparent)]
pub struct ProtocolError(Box<dyn Error>);

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
