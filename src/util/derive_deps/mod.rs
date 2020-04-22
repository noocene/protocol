mod borrow_coalesce;
mod borrow_unravel;
mod complete;
mod move_coalesce;
pub use borrow_coalesce::{BorrowCoalesce, CoalesceError as BorrowCoalesceError};
pub use borrow_unravel::{BorrowUnravel, UnravelError as BorrowUnravelError};
pub use complete::Complete;
use core::marker::PhantomData;
pub use core_error::Error;
pub use futures::ready;
pub use move_coalesce::{CoalesceError as MoveCoalesceError, MoveCoalesce};
pub use thiserror::Error;

use crate::Future;
pub use void::Void;

pub trait TraitCoalesce<C: ?Sized> {
    type Coalesce;
    type Future: Future<C, Ok = Self::Coalesce>;
    type Handle;

    fn new(handle: Self::Handle) -> Self::Future;
}

pub trait Trait {
    type MutArgs;
    type MutRets;
    type RefArgs;
    type RefRets;
    type MoveArgs;
    type MoveRets;

    type Coalesce;
    fn coalesce<C: ?Sized>(
        handle: <Self::Coalesce as TraitCoalesce<C>>::Handle,
    ) -> <Self::Coalesce as TraitCoalesce<C>>::Future
    where
        Self::Coalesce: TraitCoalesce<C>,
    {
        <Self::Coalesce as TraitCoalesce<C>>::new(handle)
    }
}

pub trait ObjectMove<T: ?Sized + Trait> {
    fn call_move(self, args: T::MoveArgs) -> T::MoveRets;
}

pub trait ObjectRef<T: ?Sized + Trait> {
    fn call_ref(&self, args: T::RefArgs) -> T::RefRets;
}

pub trait ObjectMut<T: ?Sized + Trait> {
    fn call_mut(&mut self, args: T::MutArgs) -> T::MutRets;
}

pub trait Object<T: ?Sized + Trait>: ObjectRef<T> + ObjectMut<T> + ObjectMove<T> {}

impl<T: ?Sized + Trait, O: ObjectRef<T> + ObjectMut<T> + ObjectMove<T>> Object<T> for O {}

pub struct ObjectWrapperMut<'a, T: ?Sized + Trait, U: ?Sized>(pub &'a mut U, PhantomData<T>);

impl<'a, T: ?Sized + Trait, U: ?Sized> ObjectWrapperMut<'a, T, U>
where
    Self: ObjectMut<T>,
{
    pub fn new(instance: &'a mut U) -> Self {
        ObjectWrapperMut(instance, PhantomData)
    }
}

pub struct ObjectWrapperRef<'a, T: ?Sized + Trait, U: ?Sized>(pub &'a U, PhantomData<T>);

impl<'a, T: ?Sized + Trait, U: ?Sized> ObjectWrapperRef<'a, T, U>
where
    Self: ObjectRef<T>,
{
    pub fn new(instance: &'a U) -> Self {
        ObjectWrapperRef(instance, PhantomData)
    }
}

pub struct ObjectWrapperMove<T: ?Sized + Trait, U>(pub U, PhantomData<T>);

impl<T: ?Sized + Trait, U> ObjectWrapperMove<T, U>
where
    Self: ObjectMove<T>,
{
    pub fn new(instance: U) -> Self {
        ObjectWrapperMove(instance, PhantomData)
    }
}
