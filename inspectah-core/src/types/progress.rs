use crate::types::completeness::InspectorId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepId {
    // RPM sub-steps
    QueryingPackages,
    ClassifyingPackages,
    ResolvingSourceRepos,
    ResolvingDepTree,
    VerifyingIntegrity,
    MappingFileOwnership,
    // Config sub-steps
    ApplyingRpmVerification,
    WalkingFilesystem,
    ClassifyingConfigs,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProbeId {
    ElfBinaries,
    PythonVenvs,
    PipPackages,
    NpmPackages,
    GemPackages,
    EnvFiles,
    GitRepos,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MetricKind {
    PackagesFound,
    ReposMapped,
    ConfigsModified,
    UnitsFound,
    ContainersFound,
    TimersFound,
}

#[derive(Debug, Clone)]
pub enum InspectorOutcome {
    Complete,
    Degraded { reason: String },
    Skipped { reason: String },
    Failed { reason: String },
    Interrupted,
}

#[derive(Debug, Clone)]
pub enum StepOutcome {
    Complete,
    Degraded { reason: String },
    Failed { reason: String },
    Skipped { reason: String },
    Interrupted,
}

#[derive(Debug, Clone)]
pub enum ProbeOutcome {
    Found { count: usize },
    Empty,
}

#[derive(Debug, Clone)]
pub enum ProgressEvent {
    InspectorStarted(InspectorId),
    InspectorFinished {
        id: InspectorId,
        outcome: InspectorOutcome,
    },
    StepStarted {
        inspector: InspectorId,
        step: StepId,
    },
    StepFinished {
        inspector: InspectorId,
        step: StepId,
        outcome: StepOutcome,
    },
    Metric {
        inspector: InspectorId,
        kind: MetricKind,
        value: usize,
    },
    ProbeStarted {
        inspector: InspectorId,
        probe: ProbeId,
    },
    ProbeFinished {
        inspector: InspectorId,
        probe: ProbeId,
        outcome: ProbeOutcome,
    },
}
