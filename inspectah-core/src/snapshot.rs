// Stub for Task 10 (traits) — Task 12 replaces this with the full implementation.
// The Renderer trait needs to reference InspectionSnapshot by type.

/// Placeholder snapshot type. Task 12 fills in all fields and serde round-trips.
#[derive(Debug, Clone, Default)]
pub struct InspectionSnapshot {
    pub schema_version: u32,
}
