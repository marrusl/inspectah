pub mod types;
pub mod traits;
pub mod snapshot;

pub(crate) fn is_false(v: &bool) -> bool {
    !*v
}
