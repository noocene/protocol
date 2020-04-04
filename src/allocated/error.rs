use crate::{
    future::MapOk, Coalesce, Dispatch, FlatCoalesce, FlatUnravel, Fork, Future, FutureExt, Join,
    Read, Unravel, Write,
};
use alloc::{boxed::Box, format, string::String, vec, vec::Vec};
use arrayvec::ArrayVec;
use core::fmt::{self, Debug, Display, Formatter};
use core_error::Error;

type ErrorData = ([String; 2], Vec<[String; 2]>);

pub struct ErasedError {
    display: String,
    debug: String,
    source: Option<Box<ErasedError>>,
}

impl Display for ErasedError {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "{}", self.display)
    }
}

impl Debug for ErasedError {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "{}", self.debug)
    }
}

impl Error for ErasedError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.source.as_ref().map(|source| source as &dyn Error)
    }
}

fn into_data<T: ?Sized + Error + 'static>(initial: Box<T>) -> ErrorData {
    let mut data = vec![];
    let mut error = initial.source();
    while let Some(e) = error {
        data.push([format!("{}", e), format!("{:?}", e)]);
        error = e.source();
    }
    ([format!("{}", initial), format!("{:?}", initial)], data)
}

fn from_data(initial: ErrorData) -> ErasedError {
    let data = initial.1.into_iter();
    fn construct(
        item: [String; 2],
        mut remainder: impl Iterator<Item = [String; 2]>,
    ) -> ErasedError {
        let mut item = ArrayVec::from(item).into_iter();
        ErasedError {
            display: item.next().unwrap(),
            debug: item.next().unwrap(),
            source: remainder
                .next()
                .map(|item| Box::new(construct(item, remainder))),
        }
    }
    construct(initial.0, data)
}

macro_rules! marker_variants {
    ($(
        $($marker:ident)*
    ),+) => {
        $(
            impl<C: ?Sized + Write<<C as Dispatch<ErrorData>>::Handle> + Fork<ErrorData> + Unpin> Unravel<C>
                for Box<dyn Error $(+ $marker)*>
            where
                C::Future: Unpin,
                C::Target: Unpin,
                C::Handle: Unpin,
                C::Finalize: Unpin,
            {
                type Finalize = <FlatUnravel<ErrorData, C> as Future<C>>::Ok;
                type Target = FlatUnravel<ErrorData, C>;

                fn unravel(self) -> Self::Target {
                    FlatUnravel::new(into_data(self))
                }
            }

            impl<C: ?Sized + Read<<C as Dispatch<ErrorData>>::Handle> + Join<ErrorData> + Unpin> Coalesce<C>
                for Box<dyn Error $(+ $marker)*>
            where
                C::Future: Unpin,
                C::Handle: Unpin,
            {
                type Future = MapOk<FlatCoalesce<ErrorData, C>, fn(ErrorData) -> Self>;

                fn coalesce() -> Self::Future {
                    FlatCoalesce::new().map_ok(|data| Box::new(from_data(data)))
                }
            }
        )*
    };
}

marker_variants! {
    ,
    Sync,
    Send, Sync Send
}
