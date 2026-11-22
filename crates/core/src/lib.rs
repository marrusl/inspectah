pub mod aggregate;
pub mod baseline;
pub mod machine_state;
pub mod pipeline;
pub mod redaction;
pub mod repo_ownership;
pub mod rpm_filename;
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
