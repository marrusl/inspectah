//! A staged RPM upload the export has proved, and the map the renderer
//! reads.
//!
//! The snapshot's `uploaded` and `cache_path` fields are `#[serde(default)]`
//! and deserialize straight from a scanned host, so neither is evidence of
//! anything. [`VerifiedUpload`] is the opposite: the export builds one only
//! after proving the file is a regular file inside the prepared upload
//! directory under a name the allowlist admits, and the renderer emits a
//! `COPY` or a `RUN` for a package only when the map holds an entry for it.
//!
//! The basename is a [`StagedName`], not a `String`. That is the point of
//! the type: a rendered `RUN dnf localinstall -y /tmp/<basename>` runs as
//! root at image build time, and there is no constructor that puts an
//! unvalidated name in this struct for it to interpolate.

use std::collections::BTreeMap;

use crate::rpm_filename::StagedName;

/// A staged upload proved fit to export, keyed in [`VerifiedUploads`] by the
/// package's `name.arch`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedUpload {
    /// The `name.arch` of the package this upload belongs to.
    pub canonical: String,
    /// The proved basename, the only name the export writes and the
    /// renderer names.
    pub basename: StagedName,
    /// The version-release re-derived from `basename`, when it differs from
    /// the version the host reported. Never read from the snapshot's
    /// `uploaded_version` field, which a scan supplies.
    pub uploaded_version: Option<String>,
}

/// Every verified staged upload for one export, by `name.arch`.
///
/// The export and the Containerfile preview build this the same way, which
/// is what keeps the preview from promising a `COPY` the export will not
/// write.
pub type VerifiedUploads = BTreeMap<String, VerifiedUpload>;
