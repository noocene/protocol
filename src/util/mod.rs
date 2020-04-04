mod fork_context_ref;
pub use fork_context_ref::ForkContextRef;
mod join_context_owned;
pub use join_context_owned::JoinContextOwned;

use crate::{ContextReference, Notify, ReferenceContext};

pub type RefContextTarget<C> = <<C as ReferenceContext>::Context as ContextReference<C>>::Target;

pub type Notification<C, T> = <C as Notify<T>>::Notification;
