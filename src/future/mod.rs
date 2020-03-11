use core::{
    borrow::BorrowMut,
    pin::Pin,
    task::{Context, Poll},
};

mod ready;
pub use ready::{ready, Ready};
pub mod unordered;
pub use unordered::Unordered;

pub trait Future<C: ?Sized> {
    type Ok;
    type Error;

    fn poll<R: BorrowMut<C>>(
        self: Pin<&mut Self>,
        cx: &mut Context,
        ctx: R,
    ) -> Poll<Result<Self::Ok, Self::Error>>;
}
