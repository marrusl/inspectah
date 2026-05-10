pub mod types;
pub mod traits;
pub mod snapshot;
pub mod pipeline;

pub(crate) fn is_false(v: &bool) -> bool {
    !*v
}
