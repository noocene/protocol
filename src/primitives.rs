use crate::{
    future::{ok, Ready},
    Coalesce, Future, Read, Unravel, Write,
};
use core::{
    borrow::BorrowMut,
    marker::PhantomData,
    pin::Pin,
    task::{Context, Poll},
};
use futures::ready;

pub struct PrimitiveUnravel<T> {
    data: Option<T>,
}

impl<T: Unpin, C: ?Sized + Unpin + Write<T>> Future<C> for PrimitiveUnravel<T> {
    type Ok = Ready<(), <C as Write<T>>::Error>;
    type Error = <C as Write<T>>::Error;

    fn poll<R: BorrowMut<C>>(
        mut self: Pin<&mut Self>,
        cx: &mut Context,
        mut ctx: R,
    ) -> Poll<Result<Self::Ok, Self::Error>> {
        let ctx = ctx.borrow_mut();

        loop {
            if let Some(data) = self.data.take() {
                let mut ct = Pin::new(&mut *ctx);
                match ct.as_mut().poll_ready(cx) {
                    Poll::Ready(r) => {
                        r?;
                        ct.write(data)?;
                    }
                    Poll::Pending => {
                        self.data = Some(data);
                        return Poll::Pending;
                    }
                }
            } else {
                ready!(Pin::new(&mut *ctx).poll_flush(cx))?;
                return Poll::Ready(Ok(ok(())));
            }
        }
    }
}

pub struct PrimitiveCoalesce<T> {
    data: PhantomData<T>,
}

impl<T: Unpin, C: ?Sized + Unpin + Read<T>> Future<C> for PrimitiveCoalesce<T> {
    type Ok = T;
    type Error = <C as Read<T>>::Error;

    fn poll<R: BorrowMut<C>>(
        self: Pin<&mut Self>,
        cx: &mut Context,
        mut ctx: R,
    ) -> Poll<Result<Self::Ok, Self::Error>> {
        let ctx = ctx.borrow_mut();

        Pin::new(ctx).read(cx)
    }
}

macro_rules! impl_primitives {
    ($($ty:ty)+) => {
        $(
            impl<C: ?Sized + Unpin + Write<Self>> Unravel<C> for $ty {
                type Finalize = Ready<(), C::Error>;
                type Target = PrimitiveUnravel<Self>;

                fn unravel(self) -> Self::Target {
                    PrimitiveUnravel { data: Some(self) }
                }
            }

            impl<C: ?Sized + Unpin + Read<Self>> Coalesce<C> for $ty {
                type Future = PrimitiveCoalesce<Self>;

                fn coalesce() -> Self::Future {
                    PrimitiveCoalesce { data: PhantomData }
                }
            }
        )+
    };
}

impl_primitives!(u8 u16 u32 u64 u128 i8 i16 i32 i64 i128 f32 f64 bool char usize isize);

#[cfg(feature = "alloc")]
mod allocated {
    use super::*;
    use alloc::string::String;

    impl_primitives!(String);
}

impl<C: ?Sized> Unravel<C> for () {
    type Finalize = Ready<()>;
    type Target = Ready<Ready<()>>;

    fn unravel(self) -> Self::Target {
        ok(ok(()))
    }
}

impl<C: ?Sized> Coalesce<C> for () {
    type Future = Ready<()>;

    fn coalesce() -> Self::Future {
        ok(())
    }
}

impl<T: Unpin + ?Sized, C: ?Sized> Unravel<C> for PhantomData<T> {
    type Finalize = Ready<()>;
    type Target = Ready<Ready<()>>;

    fn unravel(self) -> Self::Target {
        ok(ok(()))
    }
}

impl<T: Unpin + ?Sized, C: ?Sized> Coalesce<C> for PhantomData<T> {
    type Future = Ready<PhantomData<T>>;

    fn coalesce() -> Self::Future {
        ok(PhantomData)
    }
}

impl<T: Unpin, C: ?Sized> Unravel<C> for [T; 0] {
    type Finalize = Ready<()>;
    type Target = Ready<Ready<()>>;

    fn unravel(self) -> Self::Target {
        ok(ok(()))
    }
}

impl<T: Unpin, C: ?Sized> Coalesce<C> for [T; 0] {
    type Future = Ready<[T; 0]>;

    fn coalesce() -> Self::Future {
        ok([])
    }
}
