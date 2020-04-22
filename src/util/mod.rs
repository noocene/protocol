mod fork_context_ref;
pub use fork_context_ref::*;
mod join_context_owned;
pub use join_context_owned::*;
mod join_context_shared;
pub use join_context_shared::*;
mod with_context;
pub use with_context::*;

#[cfg(feature = "alloc")]
#[doc(hidden)]
pub mod derive_deps;

use crate::{ContextReference, Notify, ReferenceContext};

pub type RefContextTarget<C> = <<C as ReferenceContext>::Context as ContextReference<C>>::Target;

pub type Notification<C, T> = <C as Notify<T>>::Notification;
