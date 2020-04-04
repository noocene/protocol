mod default;
pub use default::{Default, DefaultAdapter};

pub trait Adapt<T> {
    type Adapter;

    fn wrap(item: T) -> Self::Adapter;
    fn unwrap(target: Self::Adapter) -> T;
}
