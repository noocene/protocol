use super::Adapt;
use crate::{
    future::{ok, Ready},
    Coalesce, Unravel,
};
use core::{default, marker::PhantomData};

pub struct DefaultAdapter<T: default::Default + ?Sized> {
    data: PhantomData<T>,
}

impl<T: default::Default + ?Sized> Unpin for DefaultAdapter<T> {}

impl<T: default::Default + ?Sized, C: ?Sized> Coalesce<C> for DefaultAdapter<T> {
    type Future = Ready<DefaultAdapter<T>>;

    fn coalesce() -> Self::Future {
        ok(DefaultAdapter { data: PhantomData })
    }
}

pub struct Default;

impl<T: default::Default + ?Sized> Adapt<T> for Default {
    type Adapter = DefaultAdapter<T>;

    fn wrap(_: T) -> Self::Adapter {
        DefaultAdapter { data: PhantomData }
    }

    fn unwrap(_: Self::Adapter) -> T {
        T::default()
    }
}

impl<T: default::Default + ?Sized, C: ?Sized> Unravel<C> for DefaultAdapter<T> {
    type Target = Ready<Ready<()>>;
    type Finalize = Ready<()>;

    fn unravel(self) -> Self::Target {
        ok(ok(()))
    }
}
