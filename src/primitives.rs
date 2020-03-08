use crate::{ready, Coalesce, Future, Read, Ready, Unravel, Write};
use core::{
    borrow::BorrowMut,
    marker::PhantomData,
    pin::Pin,
    task::{Context, Poll},
};

pub struct PrimitiveUnravel<T> {
    data: Option<T>,
}

impl<T: Unpin, C: Unpin + Write<T>> Future<C> for PrimitiveUnravel<T> {
    type Ok = ();
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
                return Poll::Ready(Ok(()));
            }
        }
    }
}

pub struct PrimitiveCoalesce<T> {
    data: PhantomData<T>,
}

impl<T: Unpin, C: Unpin + Read<T>> Future<C> for PrimitiveCoalesce<T> {
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
            impl<C: Unpin + Write<Self>> Unravel<C> for $ty {
                type Future = PrimitiveUnravel<Self>;

                fn unravel(self) -> Self::Future {
                    PrimitiveUnravel { data: None }
                }
            }

            impl<C: Unpin + Read<Self>> Coalesce<C> for $ty {
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

impl<C: Unpin + Write<Self>> Unravel<C> for () {
    type Future = Ready<()>;

    fn unravel(self) -> Self::Future {
        ready(())
    }
}

impl<C: Unpin + Read<Self>> Coalesce<C> for () {
    type Future = Ready<()>;

    fn coalesce() -> Self::Future {
        ready(())
    }
}

impl<T: Unpin + ?Sized, C: Unpin + Write<Self>> Unravel<C> for PhantomData<T> {
    type Future = Ready<()>;

    fn unravel(self) -> Self::Future {
        ready(())
    }
}

impl<T: Unpin + ?Sized, C: Unpin + Read<Self>> Coalesce<C> for PhantomData<T> {
    type Future = Ready<PhantomData<T>>;

    fn coalesce() -> Self::Future {
        ready(PhantomData)
    }
}

impl<T: Unpin, C: Unpin + Write<Self>> Unravel<C> for [T; 0] {
    type Future = Ready<()>;

    fn unravel(self) -> Self::Future {
        ready(())
    }
}

impl<T: Unpin, C: Unpin + Read<Self>> Coalesce<C> for [T; 0] {
    type Future = Ready<[T; 0]>;

    fn coalesce() -> Self::Future {
        ready([])
    }
}
