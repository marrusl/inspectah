pub mod baseline;
pub mod fleet;
pub mod normalize;
pub mod pipeline;
pub mod snapshot;
pub mod traits;
pub mod types;

pub(crate) fn is_false(v: &bool) -> bool {
    !*v
}

/// Deserialize JSON `null` as `T::default()` (typically `Vec::new()`).
///
/// Go serializes empty slices as `null`. Rust `Vec<T>` cannot deserialize
/// `null` even with `#[serde(default)]` (which only covers *missing* fields).
/// Apply via `#[serde(default, deserialize_with = "crate::deserialize_null_default")]`.
pub(crate) fn deserialize_null_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Default + serde::Deserialize<'de>,
{
    use serde::Deserialize;
    Ok(Option::<T>::deserialize(deserializer)?.unwrap_or_default())
}
