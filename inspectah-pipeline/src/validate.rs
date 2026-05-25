use inspectah_core::pipeline::{Collected, Pipeline, Validated};
use inspectah_core::snapshot::SCHEMA_VERSION;

/// Errors from schema validation.
#[derive(Debug, thiserror::Error)]
pub enum ValidationError {
    #[error(
        "unsupported schema version: {version} (accepted: {max})",
        max = SCHEMA_VERSION
    )]
    UnsupportedVersion { version: u32 },

    #[error("missing required section: {section}")]
    MissingSection { section: String },
}

/// Validate a collected snapshot.
///
/// - Checks schema version matches SCHEMA_VERSION exactly.
/// - Returns `Pipeline<Validated>` on success.
pub fn validate(pipeline: Pipeline<Collected>) -> Result<Pipeline<Validated>, ValidationError> {
    let snapshot = pipeline.state.snapshot;

    if snapshot.schema_version != SCHEMA_VERSION {
        return Err(ValidationError::UnsupportedVersion {
            version: snapshot.schema_version,
        });
    }

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
        assert_eq!(
            result.unwrap().state.snapshot.schema_version,
            SCHEMA_VERSION
        );
    }

    #[test]
    fn test_validate_rejects_old_version() {
        let p = collected(16);
        let result = validate(p);
        assert!(result.is_err());
        match result.unwrap_err() {
            ValidationError::UnsupportedVersion { version } => {
                assert_eq!(version, 16);
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
