//! The authselect profile, rendered as the `authselect select` line an
//! administrator would write, never as copied files.
//!
//! authselect generates `nsswitch.conf` and the PAM stacks from the
//! selected profile and records the selection in
//! [`AUTHSELECT_CONF_PATH`]. Copying the generated files would give the
//! image files authselect then reports as changed outside it; selecting
//! the profile writes them again, from the profile the image's
//! authselect ships. So an included config entry at that path becomes
//! one `RUN authselect select` line, and the config tree does not stage
//! the file ([`super::configtree`]'s `owned_elsewhere`).
//!
//! The line runs after the Configuration Files `COPY`, so a custom
//! profile under `/etc/authselect/custom/` and `user-nsswitch.conf`,
//! which the config tree carries as ordinary files, are in place when it
//! runs. A custom profile is selected only when the export carries every
//! file of it the scan found, `README` included
//! ([`missing_profile_files`]); otherwise the section is a FIXME.

use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::config::AUTHSELECT_CONF_PATH;

use super::comment::escape_comment_text;
use super::configtree::{StagedWrites, is_staged, staged_writes};
use super::containerfile::section;
use super::safety::{BareWord, BareWordError};

/// The directory part of a custom profile's id (`custom/<name>`).
const CUSTOM_PROFILE_PREFIX: &str = "custom/";

/// Where a custom profile `custom/<name>` lives on the host and in the
/// image: this, then `<name>/`.
const CUSTOM_PROFILE_DIR: &str = "/etc/authselect/custom/";

/// The one file `authselect select` cannot do without: with no `README`
/// it fails with "Unable to find profile" (measured in rhel-bootc 9.8 and
/// 10.2), which fails the build.
const PROFILE_README: &str = "README";

/// The Containerfile section the line is written under.
const SECTION_HEADER: &str = "Authentication Profile (authselect)";

/// Why the line is there and what it does.
const SELECT_COMMENT: &str = "# Profile and features from /etc/authselect/authselect.conf; \
     authselect select writes nsswitch.conf and the PAM files from them";

/// Why `--force`: authselect refuses to select a profile over files it
/// did not write, and a base image, or a file this export copies, may
/// hold such a file. `--force` makes the profile the authority, which is
/// what carrying the selection means.
const FORCE_FLAG: &str = "--force";

/// A profile selection read from `authselect.conf`: every word proved a
/// bare word, so the line needs no quoting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selection {
    /// `sssd`, `local`, or `custom/<name>`.
    profile: String,
    features: Vec<BareWord>,
}

/// Why an `authselect.conf` cannot be rendered as a line.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SelectionRefusal {
    #[error("the file names no profile")]
    NoProfile,
    #[error("the profile id is not a plain name or custom/<name>: {0}")]
    Profile(BareWordError),
    #[error("a feature is not a plain name: {0}")]
    Feature(BareWordError),
}

impl Selection {
    /// Read `authselect.conf`: the profile id on the first line that is
    /// not blank or a `#` comment, then one feature per line.
    pub fn parse(content: &str) -> Result<Self, SelectionRefusal> {
        let mut words = content
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with('#'));
        let profile = words.next().ok_or(SelectionRefusal::NoProfile)?;
        let name = profile
            .strip_prefix(CUSTOM_PROFILE_PREFIX)
            .unwrap_or(profile);
        BareWord::parse_name(name).map_err(SelectionRefusal::Profile)?;
        if !name.starts_with(|c: char| c.is_ascii_alphanumeric()) {
            return Err(SelectionRefusal::Profile(BareWordError::BadStart));
        }
        let features = words
            .map(|feature| BareWord::parse_name(feature).map_err(SelectionRefusal::Feature))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            profile: profile.to_string(),
            features,
        })
    }

    /// The name of a custom profile (`corp` for `custom/corp`), `None`
    /// for one authselect ships.
    pub fn custom_name(&self) -> Option<&str> {
        self.profile.strip_prefix(CUSTOM_PROFILE_PREFIX)
    }

    /// The `RUN authselect select` line.
    pub fn run_line(&self) -> String {
        let mut line = format!("RUN authselect select {}", self.profile);
        for feature in &self.features {
            line.push(' ');
            line.push_str(&feature.to_string());
        }
        line.push(' ');
        line.push_str(FORCE_FLAG);
        line
    }
}

/// The files of the custom profile `name` that the export does not carry,
/// sorted: every config entry the scan found in the profile's directory
/// that `staged` does not write (excluded in refine, or not captured),
/// and its `README` if that is not written. `authselect select` fails
/// without the `README` and writes an empty PAM stack for a missing
/// template (measured in rhel-bootc 9.8 and 10.2), so the profile is not
/// selected while anything is missing.
fn missing_profile_files(
    snap: &InspectionSnapshot,
    staged: &StagedWrites<'_>,
    name: &str,
) -> Vec<String> {
    let dir = format!("{CUSTOM_PROFILE_DIR}{name}/");
    let readme = format!("{dir}{PROFILE_README}");
    let mut paths: Vec<String> = snap
        .config
        .iter()
        .flat_map(|config| config.files.iter())
        .map(|f| f.path.clone())
        .filter(|path| path.starts_with(&dir))
        .chain(std::iter::once(readme))
        .filter(|path| !is_staged(staged, path))
        .collect();
    paths.sort();
    paths.dedup();
    paths
}

/// The authselect section of the Containerfile: the `authselect select`
/// line when an included config entry records the selection, a FIXME
/// when that entry cannot be read as one or names a custom profile the
/// export does not carry whole, nothing otherwise.
pub(crate) fn authselect_lines(snap: &InspectionSnapshot) -> Vec<String> {
    let Some(entry) = snap.config.as_ref().and_then(|config| {
        config
            .files
            .iter()
            .find(|f| f.path == AUTHSELECT_CONF_PATH && f.disposition.is_included())
    }) else {
        return Vec::new();
    };
    let body = match Selection::parse(&entry.content) {
        Ok(selection) => {
            let missing = selection
                .custom_name()
                .map(|name| missing_profile_files(snap, &staged_writes(snap), name))
                .unwrap_or_default();
            if missing.is_empty() {
                vec![SELECT_COMMENT.to_string(), selection.run_line()]
            } else {
                vec![format!(
                    "# FIXME: authselect profile {} not selected: the export does not carry {}; \
                     authselect select fails without the profile's README and writes an empty \
                     PAM stack for a missing template. Put the profile in place and run: {}",
                    selection.profile,
                    escape_comment_text(&missing.join(", ")),
                    selection.run_line().trim_start_matches("RUN "),
                )]
            }
        }
        Err(refusal) => vec![format!(
            "# FIXME: authselect profile not selected: {refusal}; run authselect select by hand"
        )],
    };
    section(SECTION_HEADER, body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_profile_and_its_features_are_one_line() {
        let selection = Selection::parse("sssd\nwith-faillock\nwith-mkhomedir\n").unwrap();
        assert_eq!(
            selection.run_line(),
            "RUN authselect select sssd with-faillock with-mkhomedir --force"
        );
    }

    #[test]
    fn a_custom_profile_keeps_its_directory() {
        let selection = Selection::parse("custom/corp\n\nwith-sudo\n").unwrap();
        assert_eq!(
            selection.run_line(),
            "RUN authselect select custom/corp with-sudo --force"
        );
    }

    #[test]
    fn host_text_that_is_not_a_plain_word_is_refused() {
        for (content, refusal) in [
            ("", SelectionRefusal::NoProfile),
            ("# only a comment\n", SelectionRefusal::NoProfile),
        ] {
            assert_eq!(Selection::parse(content), Err(refusal));
        }
        for content in [
            "sssd; rm -rf /\n",
            "-sssd\n",
            "custom/../x\n",
            "custom/..\n",
            "sssd\nwith faillock\n",
            "sssd\n--force-bad\n",
            "sssd\n$(id)\n",
        ] {
            assert!(Selection::parse(content).is_err(), "{content:?}");
        }
    }
}
