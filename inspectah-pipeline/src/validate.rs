use inspectah_core::pipeline::{Collected, Pipeline, Validated};
use inspectah_core::snapshot::{migrate, SCHEMA_VERSION};

/// Errors from schema validation.
#[derive(Debug, thiserror::Error)]
pub enum ValidationError {
    #[error(
        "unsupported schema version: {version} (accepted: {min}-{max})",
        min = 12,
        max = SCHEMA_VERSION
    )]
    UnsupportedVersion { version: u32 },

    #[error("missing required section: {section}")]
    MissingSection { section: String },
}

/// Validate and optionally migrate a collected snapshot.
///
/// - Checks schema version is in the accepted range (12..=SCHEMA_VERSION).
/// - Migrates older snapshots to current version.
/// - Returns `Pipeline<Validated>` on success.
pub fn validate(pipeline: Pipeline<Collected>) -> Result<Pipeline<Validated>, ValidationError> {
    let mut snapshot = pipeline.state.snapshot;

    // Schema version check
    const MIN_SCHEMA: u32 = 12;
    if snapshot.schema_version < MIN_SCHEMA || snapshot.schema_version > SCHEMA_VERSION {
        return Err(ValidationError::UnsupportedVersion {
            version: snapshot.schema_version,
        });
    }

    // Migrate if needed
    migrate(&mut snapshot);

    // Required sections check: os_release should exist for meaningful snapshots,
    // but we treat this as a warning rather than a hard failure to support
    // partial/degraded snapshots from the collect phase.
    // (rpm, config, etc. are all Optional by design)

    Ok(Pipeline {
        state: Validated { snapshot },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use inspectah_core::snapshot::InspectionSnapshot;

    fn collected(schema: u32) -> Pipeline<Collected> {
        let snap = InspectionSnapshot {
            schema_version: schema,
            ..Default::default()
        };
        Pipeline {
            state: Collected { snapshot: snap },
        }
    }

    #[test]
    fn test_validate_current_version() {
        let p = collected(SCHEMA_VERSION);
        let result = validate(p);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().state.snapshot.schema_version, SCHEMA_VERSION);
    }

    #[test]
    fn test_validate_v12_migrates() {
        let p = collected(12);
        let result = validate(p);
        assert!(result.is_ok());
        // After migration, version should be bumped to current
        assert_eq!(result.unwrap().state.snapshot.schema_version, SCHEMA_VERSION);
    }

    #[test]
    fn test_validate_v13_migrates() {
        let p = collected(13);
        let result = validate(p);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().state.snapshot.schema_version, SCHEMA_VERSION);
    }

    #[test]
    fn test_validate_rejects_v11() {
        let p = collected(11);
        let result = validate(p);
        assert!(result.is_err());
        match result.unwrap_err() {
            ValidationError::UnsupportedVersion { version } => {
                assert_eq!(version, 11);
            }
            other => panic!("expected UnsupportedVersion, got {other:?}"),
        }
    }

    #[test]
    fn test_validate_rejects_future_version() {
        let p = collected(99);
        let result = validate(p);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_returns_validated_state() {
        let p = collected(SCHEMA_VERSION);
        let validated = validate(p).unwrap();
        // Validated state carries the snapshot
        assert_eq!(validated.state.snapshot.schema_version, SCHEMA_VERSION);
    }
}
