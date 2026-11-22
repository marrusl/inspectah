//! The one way a file enters the Configuration section.
//!
//! Every branch of the config inspector (rpm -Va, `%ghost`, the unowned
//! walk, orphans, the ostree path, the authselect advisories) hands its
//! candidates to [`ConfigCollector::admit`], which reads them through the
//! machine-state read gate and records the class: a withheld member as a
//! [`WithheldConfigEntry`], with no content; a carriable member as a
//! `MachineState` entry, excluded by default; anything else as before. A
//! branch keeps its own skips (DHCP, dev artifacts, rpm ownership, the
//! silent lists), but it cannot add a file any other way, so a new branch
//! cannot carry a member's bytes.

use super::admit;
use super::classify::classify_config_path;
use super::{CaptureFailure, read_config_content};
use inspectah_core::machine_state::{
    RuleTarget, Screened, ScreenedExecutor, classify_path, normal_path,
};
use inspectah_core::traits::executor::Executor;
use inspectah_core::types::config::{ConfigFileEntry, ConfigFileKind, ConfigSection};
use inspectah_core::types::machine_state::{MachineStateMember, WithheldConfigEntry};
use inspectah_core::types::{AdvisoryType, FindingKind};
use std::collections::HashSet;
use std::path::Path;

/// A file a branch found and wants in the section.
#[derive(Debug, Clone)]
pub(super) struct Candidate<'a> {
    /// The path the entry is recorded under.
    pub path: &'a str,
    /// The path the content is read from: `path`, or for the `%ghost`
    /// branch the file's physical spelling under a directory link.
    pub read: &'a str,
    pub kind: ConfigFileKind,
    pub package: Option<&'a str>,
    pub rpm_va_flags: Option<&'a str>,
}

impl<'a> Candidate<'a> {
    /// A candidate read from the path it is recorded under.
    pub fn new(path: &'a str, kind: ConfigFileKind) -> Self {
        Self {
            path,
            read: path,
            kind,
            package: None,
            rpm_va_flags: None,
        }
    }
}

/// Owns the Configuration section while the inspector runs.
pub(super) struct ConfigCollector<'e> {
    exec: &'e ScreenedExecutor<'e>,
    section: ConfigSection,
    /// Normalized paths in `files` or `withheld`.
    seen: HashSet<String>,
    degraded: Vec<String>,
}

impl<'e> ConfigCollector<'e> {
    pub fn new(exec: &'e ScreenedExecutor<'e>) -> Self {
        Self {
            exec,
            section: ConfigSection::default(),
            seen: HashSet::new(),
            degraded: Vec::new(),
        }
    }

    /// The degraded reasons so far, for branches that add their own.
    pub fn degraded(&mut self) -> &mut Vec<String> {
        &mut self.degraded
    }

    /// Whether a path is already in the section, under either list.
    pub fn contains(&self, path: &str) -> bool {
        self.seen.contains(&key(path))
    }

    /// The ordinary or class entry recorded at `path`, to change its
    /// disposition (the modernization and authselect passes).
    pub fn entry_mut(&mut self, path: &str) -> Option<&mut ConfigFileEntry> {
        self.section.files.iter_mut().find(|f| f.path == path)
    }

    /// The ordinary entries collected so far.
    pub fn files(&self) -> &[ConfigFileEntry] {
        &self.section.files
    }

    /// Whether `path` is a withheld row.
    pub fn is_withheld(&self, path: &str) -> bool {
        let k = key(path);
        self.section.withheld.iter().any(|w| key(&w.path) == k)
    }

    /// Read the candidate through the gate and record it: a withheld row
    /// when the gate refuses it by path, alias, inode or content; a
    /// `MachineState` entry for a carriable member; otherwise the entry as
    /// the inspector always recorded it, a `ContentNotCaptured` advisory
    /// when the read failed. A carriable member whose read failed is
    /// withheld, since there is nothing to ship.
    ///
    /// Returns the ordinary or class entry it pushed, so a caller can
    /// adjust its disposition, or `None` when the file was withheld or
    /// already in the section. An [`admit::Unreadable`] is returned for
    /// the caller to handle as it always has (a FIFO, a link leaving
    /// `/etc`, ...): nothing is recorded for it here.
    pub fn admit(
        &mut self,
        c: Candidate<'_>,
    ) -> Result<Option<&mut ConfigFileEntry>, admit::Unreadable> {
        if self.contains(c.path) {
            return Ok(None);
        }
        if let Some(member) = self.withheld_by_screen(&c) {
            self.withhold(&c, member);
            return Ok(None);
        }
        let carriable = carriable_member(c.path);
        let readable = admit::readable(self.exec, c.read)?;
        let read = if readable == admit::Readable::DevNullMask {
            Ok(String::new())
        } else {
            read_config_content(self.exec, c.read, &mut self.degraded)
        };
        let (content, disposition) = match read {
            Ok(content) => (
                content,
                match carriable {
                    Some(member) => FindingKind::MachineState {
                        member,
                        include: false,
                    },
                    None => FindingKind::default(),
                },
            ),
            Err(CaptureFailure::Refused(member)) => {
                self.withhold(&c, member);
                return Ok(None);
            }
            Err(failure) => {
                if let Some(member) = carriable {
                    self.withhold(&c, MachineStateMember::Carriable(member));
                    return Ok(None);
                }
                (
                    String::new(),
                    FindingKind::advisory(AdvisoryType::ContentNotCaptured, failure.rationale()),
                )
            }
        };
        Ok(Some(self.push(c, content, disposition)))
    }

    /// For entries that carry no content by construction: cross-tree
    /// symlink advisories and modernization advisories for absent files.
    /// Still classifies, so a member path never becomes an advisory: a
    /// withheld member is a withheld row, and a carriable member, having
    /// no content here, is withheld as well.
    pub fn admit_advisory(&mut self, c: Candidate<'_>, disposition: FindingKind) {
        if self.contains(c.path) {
            return;
        }
        if let Some(member) = self.withheld_by_screen(&c) {
            self.withhold(&c, member);
            return;
        }
        if let Some(member) = carriable_member(c.path) {
            self.withhold(&c, MachineStateMember::Carriable(member));
            return;
        }
        self.push(c, String::new(), disposition);
    }

    /// The section and the degraded reasons, sorted by path.
    pub fn into_parts(mut self) -> (ConfigSection, Vec<String>) {
        self.section.files.sort_by(|a, b| a.path.cmp(&b.path));
        self.section.withheld.sort_by(|a, b| a.path.cmp(&b.path));
        (self.section, self.degraded)
    }

    /// The withheld member either spelling of the candidate is, by path,
    /// resolved path or inode.
    fn withheld_by_screen(&self, c: &Candidate<'_>) -> Option<MachineStateMember> {
        [c.path, c.read]
            .into_iter()
            .find_map(|p| match self.exec.screen(p) {
                Screened::Withheld(member) => Some(member),
                _ => None,
            })
    }

    fn withhold(&mut self, c: &Candidate<'_>, member: MachineStateMember) {
        let meta = self.exec.file_metadata(Path::new(c.read)).ok();
        let path = normal_path(c.path).unwrap_or_else(|| c.path.to_string());
        self.seen.insert(path.clone());
        self.section.withheld.push(WithheldConfigEntry {
            path,
            kind: c.kind.clone(),
            package: c.package.map(str::to_string),
            member,
            mode: meta.map(|m| m.permissions()),
            size: meta.map(|m| m.size),
            aggregate: None,
        });
    }

    fn push(
        &mut self,
        c: Candidate<'_>,
        content: String,
        disposition: FindingKind,
    ) -> &mut ConfigFileEntry {
        self.seen.insert(key(c.path));
        self.section.files.push(ConfigFileEntry {
            path: c.path.to_string(),
            kind: c.kind,
            category: classify_config_path(c.read),
            content,
            rpm_va_flags: c.rpm_va_flags.map(str::to_string),
            package: c.package.map(str::to_string),
            diff_against_rpm: None,
            disposition,
            ..Default::default()
        });
        self.section
            .files
            .last_mut()
            .expect("an entry was just pushed")
    }
}

/// The carriable member a path is by its own spelling.
fn carriable_member(path: &str) -> Option<inspectah_core::types::machine_state::CarriableMember> {
    match classify_path(path) {
        Some(RuleTarget::Carriable(member)) => Some(member),
        _ => None,
    }
}

/// The key `seen` holds a path under.
fn key(path: &str) -> String {
    normal_path(path).unwrap_or_else(|| path.to_string())
}
