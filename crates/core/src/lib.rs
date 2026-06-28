pub mod aggregate;
pub mod baseline;
pub mod pipeline;
pub mod redaction;
pub mod snapshot;
pub mod traits;
pub mod types;
pub mod util;

pub(crate) fn is_false(v: &bool) -> bool {
    !*v
}

pub fn default_true() -> bool {
    true
}
