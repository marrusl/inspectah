pub mod engine;
pub(crate) mod patterns;

pub use engine::{
    mask_token_username, redact, redact_string, scan_content, RedactOptions, Sensitivity,
};
