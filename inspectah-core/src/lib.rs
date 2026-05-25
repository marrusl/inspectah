pub mod baseline;
pub mod fleet;
pub mod pipeline;
pub mod snapshot;
pub mod traits;
pub mod types;

pub(crate) fn is_false(v: &bool) -> bool {
    !*v
}
