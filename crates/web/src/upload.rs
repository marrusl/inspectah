use axum::extract::multipart::{Field, MultipartError};
use axum::extract::{Multipart, Path as UrlPath, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json, Response};
use inspectah_refine::rpm_filename::{RpmFilename, StagedName};
use inspectah_refine::session::{
    RefineSession, UPLOAD_PART_PREFIX, UPLOAD_PART_RAND_BYTES, UPLOAD_PART_SUFFIX, UploadMatch,
};
use inspectah_refine::types::RefineError;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::error::AppError;
use crate::handlers::AppState;

/// The one multipart field an upload may carry (`useRpmUpload.ts:113`).
const UPLOAD_FIELD_NAME: &str = "file";

/// Files staged by one request. Exactly one field is accepted, so this is
/// always 1; the client's modal counts files with it.
const FILES_PER_REQUEST: u32 = 1;

/// An upload failure and the status the client sees.
#[derive(Debug)]
pub enum UploadError {
    /// The request itself is wrong, and the status says how.
    Client(StatusCode, String),
    /// The session refused, or the filesystem did.
    Session(RefineError),
}

impl From<MultipartError> for UploadError {
    fn from(e: MultipartError) -> Self {
        // Every MultipartError carries the status it deserves: a
        // `DefaultBodyLimit` overrun arrives as `StreamReadFailed` wrapping
        // `LengthLimitError`, which `status()` maps to 413, while malformed
        // framing is 400. The blanket 400 this replaces reported an
        // oversize body as a syntax error the client could fix by retrying.
        UploadError::Client(e.status(), e.body_text())
    }
}

impl From<RefineError> for UploadError {
    fn from(e: RefineError) -> Self {
        UploadError::Session(e)
    }
}

impl IntoResponse for UploadError {
    fn into_response(self) -> Response {
        match self {
            UploadError::Client(status, message) => {
                (status, Json(serde_json::json!({ "error": message }))).into_response()
            }
            UploadError::Session(e) => AppError(e).into_response(),
        }
    }
}

/// A fully written, flushed part file in the upload directory, not yet
/// under its real name.
///
/// Dropping it removes the file, including when axum drops the handler
/// future on a client disconnect, so an abandoned transfer leaves nothing
/// behind.
pub struct StagedPart {
    file: tempfile::NamedTempFile,
}

/// What the upload did, as the response reports it.
pub struct UploadOutcome {
    /// The package the filename matched, if any.
    pub matched: Option<UploadMatch>,
    /// The filename decomposed, for the client to display. Parsing is
    /// matching, not a safety claim; `staged_as` is the validated name.
    pub parsed: Option<RpmFilename>,
    /// The name the file is staged under, which is the name the client
    /// sent.
    pub staged_as: String,
}

impl UploadOutcome {
    fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "status": if self.matched.is_some() { "matched" } else { "unmatched" },
            "matched": self.matched.as_ref().map(|m| &m.canonical),
            "match_kind": self.matched.as_ref().map(|m| m.match_kind),
            "host_version": self.matched.as_ref().map(|m| &m.host_version),
            "uploaded_version": self.matched.as_ref().and_then(|m| m.uploaded_version.as_ref()),
            "uploaded": FILES_PER_REQUEST,
            "parsed": self.parsed,
            "staged_as": self.staged_as,
        })
    }
}

/// Stream one multipart field into a part file in `upload_dir`.
///
/// The `field.chunk()` loop is the only read of the body, so resident
/// memory per upload is one chunk whatever the file's size. The part file
/// is opened `O_EXCL` with mode 0600 and a name no staged RPM can wear
/// (the allowlist bans a leading dot), and `sync_all` runs here rather
/// than inside the caller's lock hold.
pub async fn stage(upload_dir: &Path, field: &mut Field<'_>) -> Result<StagedPart, UploadError> {
    let mut part = tempfile::Builder::new()
        .prefix(UPLOAD_PART_PREFIX)
        .suffix(UPLOAD_PART_SUFFIX)
        .rand_bytes(UPLOAD_PART_RAND_BYTES)
        .tempfile_in(upload_dir)
        .map_err(|e| RefineError::TarballError(format!("create upload part file: {e}")))?;

    while let Some(chunk) = field.chunk().await? {
        part.write_all(&chunk)
            .map_err(|e| RefineError::TarballError(format!("write upload part file: {e}")))?;
    }
    part.as_file()
        .sync_all()
        .map_err(|e| RefineError::TarballError(format!("flush upload part file: {e}")))?;

    Ok(StagedPart { file: part })
}

/// Place a staged part under its real name and record it, in one hold of
/// the session lock.
///
/// In order: match, the 409 check, the rename, the record, then the
/// deletion of a file this upload superseded. Taking a [`StagedName`]
/// rather than a `&str` is what keeps an unvalidated name out of the
/// rename: there is no other way into this function.
pub fn commit(
    session: &mut RefineSession,
    part: StagedPart,
    name: &StagedName,
) -> Result<UploadOutcome, UploadError> {
    let dest = session.ensure_upload_dir()?.join(name.as_str());

    // Under the same hold as the rename below, so the match cannot go
    // stale between deciding and placing.
    let matched = session.match_uploaded_rpm(name.as_str());
    let matched_canonical = matched.as_ref().map(|m| m.canonical.as_str());

    // No rename ever replaces a file that is another package's
    // `cache_path`, matched or unmatched.
    if let Some(owner) = other_package_staged_at(session, &dest, matched_canonical) {
        return Err(UploadError::Client(
            StatusCode::CONFLICT,
            format!("{} is already staged for {owner}", name.as_str()),
        ));
    }

    // Read before the record step rewrites `cache_path`.
    let superseded = matched_canonical.and_then(|c| superseded_upload(session, c, &dest));

    part.file.persist(&dest).map_err(|e| {
        RefineError::TarballError(format!("stage upload as {}: {}", dest.display(), e.error))
    })?;

    if let Some(m) = &matched {
        session.set_uploaded(&m.canonical, dest, m.uploaded_version.clone());
    }

    if let Some(old) = superseded {
        // The record step has already succeeded and cannot be undone, so a
        // failed unlink leaves an unreferenced file rather than a wrong
        // state; the next startup is not going to remove it either, since
        // it wears a staged name, but nothing reads it.
        let _ = std::fs::remove_file(old);
    }

    Ok(UploadOutcome {
        parsed: RpmFilename::parse_untrusted(name.as_str()),
        matched,
        staged_as: name.as_str().to_string(),
    })
}

/// The `name.arch` of a package other than `exclude` whose `cache_path` is
/// already `dest`.
fn other_package_staged_at(
    session: &RefineSession,
    dest: &Path,
    exclude: Option<&str>,
) -> Option<String> {
    let rpm = session.snapshot().rpm.as_ref()?;
    rpm.packages_added
        .iter()
        .filter(|pkg| pkg.cache_path.as_deref().map(Path::new) == Some(dest))
        .map(|pkg| format!("{}.{}", pkg.name, pkg.arch))
        .find(|canonical| Some(canonical.as_str()) != exclude)
}

/// The staged file a new upload for `canonical` replaces: its current
/// `cache_path`, when it holds an upload under a different name.
fn superseded_upload(session: &RefineSession, canonical: &str, dest: &Path) -> Option<PathBuf> {
    let rpm = session.snapshot().rpm.as_ref()?;
    rpm.packages_added
        .iter()
        .find(|pkg| format!("{}.{}", pkg.name, pkg.arch) == canonical && pkg.uploaded)
        .and_then(|pkg| pkg.cache_path.as_deref())
        .map(PathBuf::from)
        .filter(|previous| previous != dest)
}

/// Accept an uploaded RPM file and stage it for export.
///
/// Exactly one multipart field, named `file`, carrying a filename that
/// passes the staged-name allowlist. The file is streamed into a part file
/// in the session's upload directory, then placed under its real name and
/// recorded in one hold of the session lock. Export merges staged files
/// into repoless-packages/ alongside cached RPMs from the source tarball.
///
/// Returns match status: `"matched"` with the canonical `name.arch` when
/// the upload matched a repo-less package, or `"unmatched"` when no match
/// was found (the file is still staged for manual use).
pub async fn upload_rpm(
    State(state): State<Arc<AppState>>,
    mut multipart: Multipart,
) -> Result<Json<serde_json::Value>, UploadError> {
    let upload_dir = {
        let mut session = state.session.lock().unwrap();
        session.ensure_upload_dir()?.to_path_buf()
    };

    let mut field = multipart.next_field().await?.ok_or_else(|| {
        UploadError::Client(
            StatusCode::BAD_REQUEST,
            format!("expected one multipart field named \"{UPLOAD_FIELD_NAME}\""),
        )
    })?;
    if field.name() != Some(UPLOAD_FIELD_NAME) {
        return Err(UploadError::Client(
            StatusCode::BAD_REQUEST,
            format!("expected one multipart field named \"{UPLOAD_FIELD_NAME}\""),
        ));
    }
    let raw_filename = field
        .file_name()
        .ok_or_else(|| {
            UploadError::Client(
                StatusCode::BAD_REQUEST,
                format!("the \"{UPLOAD_FIELD_NAME}\" field carries no filename"),
            )
        })?
        .to_string();
    // Rejection is rejection: the name that reaches the filesystem is the
    // one the client sent, never a stripped version of it.
    let staged_name = StagedName::parse(&raw_filename).map_err(|e| {
        UploadError::Client(StatusCode::BAD_REQUEST, format!("filename rejected: {e}"))
    })?;

    let part = stage(&upload_dir, &mut field).await?;
    drop(field);

    // A second field is 400 and nothing is placed. Multipart is
    // sequential, so a second field is knowable only after the first has
    // been read; the part file this drops was never under a staged name
    // and no package ever pointed at it.
    if multipart.next_field().await?.is_some() {
        return Err(UploadError::Client(
            StatusCode::BAD_REQUEST,
            format!("expected exactly one multipart field, named \"{UPLOAD_FIELD_NAME}\""),
        ));
    }

    let outcome = {
        let mut session = state.session.lock().unwrap();
        commit(&mut session, part, &staged_name)?
    };

    Ok(Json(outcome.to_json()))
}

/// Remove a previously uploaded RPM file.
///
/// Accepts `name_arch` in canonical `name.arch` format (e.g. `nginx.x86_64`).
/// Reverses the effect of `upload_rpm`: deletes the staged file and clears
/// the `repoless_cached` / `cache_path` fields on the matching PackageEntry.
pub async fn delete_uploaded_rpm(
    State(state): State<Arc<AppState>>,
    UrlPath(name_arch): UrlPath<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    // Split "name.arch" on the last dot to handle package names containing dots
    let (name, arch) = name_arch.rsplit_once('.').ok_or_else(|| {
        AppError(inspectah_refine::types::RefineError::BadRequest(format!(
            "invalid name.arch format: {name_arch}"
        )))
    })?;

    let mut session = state.session.lock().unwrap();
    let found = session.unmark_uploaded_rpm(name, arch).map_err(AppError)?;

    if !found {
        return Err(AppError(inspectah_refine::types::RefineError::NotFound(
            format!("no uploaded RPM for {name_arch}"),
        )));
    }

    Ok(Json(serde_json::json!({
        "removed": name_arch,
        "status": "removed"
    })))
}
