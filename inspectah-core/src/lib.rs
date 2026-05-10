pub mod types;

pub(crate) fn is_false(v: &bool) -> bool {
    !*v
}
