use crate::snapshot::InspectionSnapshot;

pub type RawSnapshot = InspectionSnapshot;

pub struct Pipeline<S> {
    pub state: S,
}

pub struct Collected {
    pub snapshot: RawSnapshot,
}

pub struct Validated {
    pub snapshot: InspectionSnapshot,
}

pub struct Redacted {
    pub snapshot: InspectionSnapshot,
}

pub struct Artifacts {
    pub output_dir: std::path::PathBuf,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pipeline_progression() {
        let raw = RawSnapshot::default();
        let p: Pipeline<Collected> = Pipeline { state: Collected { snapshot: raw } };
        // Collected -> Validated (skip_validation for test)
        let validated = p.state.snapshot; // access snapshot from Collected
        let p: Pipeline<Validated> = Pipeline {
            state: Validated { snapshot: validated },
        };
        // Validated -> Redacted
        let p: Pipeline<Redacted> = Pipeline {
            state: Redacted { snapshot: p.state.snapshot },
        };
        // Redacted can produce artifacts
        let _ = &p.state.snapshot; // prove Redacted state is reachable
    }
}
