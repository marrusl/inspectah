pub mod types;
pub mod traits;
pub mod snapshot;
pub mod pipeline;
pub mod normalize;

pub(crate) fn is_false(v: &bool) -> bool {
    !*v
}
