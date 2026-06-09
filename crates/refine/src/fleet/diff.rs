use std::collections::BTreeMap;

use similar::{ChangeTag, TextDiff};

use crate::types::ContentHash;

const MAX_INPUT_BYTES: usize = 100_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChangeKind {
    Equal,
    Insert,
    Delete,
}

#[derive(Debug, Clone)]
pub struct DiffChange {
    pub kind: ChangeKind,
    pub content: String,
}

#[derive(Debug, Clone)]
pub struct LineRange {
    pub start: usize,
    pub count: usize,
}

#[derive(Debug, Clone)]
pub struct DiffHunk {
    pub base_range: LineRange,
    pub target_range: LineRange,
    pub changes: Vec<DiffChange>,
}

#[derive(Debug, Clone)]
pub struct DiffStats {
    pub total_changes: usize,
    pub insertions: usize,
    pub deletions: usize,
}

#[derive(Debug, Clone)]
pub struct DiffResult {
    pub hunks: Vec<DiffHunk>,
    pub stats: DiffStats,
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum DiffError {
    #[error("binary content detected")]
    BinaryContent,
    #[error("input exceeds 100KB limit")]
    InputTooLarge,
}

/// Compute a line-level diff between `base` and `target`, returning hunks
/// with up to `context_lines` of surrounding equal lines.
///
/// Guards:
/// - Rejects inputs containing null bytes (`DiffError::BinaryContent`).
/// - Rejects inputs larger than 100KB (`DiffError::InputTooLarge`).
pub fn compute_diff(
    base: &str,
    target: &str,
    context_lines: usize,
) -> Result<DiffResult, DiffError> {
    if base.contains('\0') || target.contains('\0') {
        return Err(DiffError::BinaryContent);
    }
    if base.len() > MAX_INPUT_BYTES || target.len() > MAX_INPUT_BYTES {
        return Err(DiffError::InputTooLarge);
    }

    let text_diff = TextDiff::from_lines(base, target);
    let grouped = text_diff.grouped_ops(context_lines);

    let mut hunks = Vec::new();
    let mut insertions = 0usize;
    let mut deletions = 0usize;

    for group in &grouped {
        let mut changes = Vec::new();
        let mut base_start = usize::MAX;
        let mut base_end = 0usize;
        let mut target_start = usize::MAX;
        let mut target_end = 0usize;

        for op in group {
            let (tag, old_range, new_range) = match op {
                similar::DiffOp::Equal {
                    old_index,
                    new_index,
                    len,
                } => (
                    ChangeTag::Equal,
                    *old_index..*old_index + len,
                    *new_index..*new_index + len,
                ),
                similar::DiffOp::Delete {
                    old_index,
                    old_len,
                    new_index,
                } => (
                    ChangeTag::Delete,
                    *old_index..*old_index + old_len,
                    *new_index..*new_index,
                ),
                similar::DiffOp::Insert {
                    old_index,
                    new_index,
                    new_len,
                } => (
                    ChangeTag::Insert,
                    *old_index..*old_index,
                    *new_index..*new_index + new_len,
                ),
                similar::DiffOp::Replace {
                    old_index,
                    old_len,
                    new_index,
                    new_len,
                } => {
                    // Replace is delete + insert. Process deletions first.
                    let or = *old_index..*old_index + old_len;
                    let nr = *new_index..*new_index + new_len;

                    if or.start < base_start {
                        base_start = or.start;
                    }
                    if or.end > base_end {
                        base_end = or.end;
                    }
                    if nr.start < target_start {
                        target_start = nr.start;
                    }
                    if nr.end > target_end {
                        target_end = nr.end;
                    }

                    for idx in or {
                        let value = text_diff.old_slices()[idx];
                        changes.push(DiffChange {
                            kind: ChangeKind::Delete,
                            content: value.to_string(),
                        });
                        deletions += 1;
                    }
                    for idx in nr {
                        let value = text_diff.new_slices()[idx];
                        changes.push(DiffChange {
                            kind: ChangeKind::Insert,
                            content: value.to_string(),
                        });
                        insertions += 1;
                    }
                    continue;
                }
            };

            if old_range.start < base_start {
                base_start = old_range.start;
            }
            if old_range.end > base_end {
                base_end = old_range.end;
            }
            if new_range.start < target_start {
                target_start = new_range.start;
            }
            if new_range.end > target_end {
                target_end = new_range.end;
            }

            match tag {
                ChangeTag::Equal => {
                    for idx in old_range {
                        let value = text_diff.old_slices()[idx];
                        changes.push(DiffChange {
                            kind: ChangeKind::Equal,
                            content: value.to_string(),
                        });
                    }
                }
                ChangeTag::Delete => {
                    for idx in old_range {
                        let value = text_diff.old_slices()[idx];
                        changes.push(DiffChange {
                            kind: ChangeKind::Delete,
                            content: value.to_string(),
                        });
                        deletions += 1;
                    }
                }
                ChangeTag::Insert => {
                    for idx in new_range {
                        let value = text_diff.new_slices()[idx];
                        changes.push(DiffChange {
                            kind: ChangeKind::Insert,
                            content: value.to_string(),
                        });
                        insertions += 1;
                    }
                }
            }
        }

        if base_start == usize::MAX {
            base_start = 0;
        }
        if target_start == usize::MAX {
            target_start = 0;
        }

        hunks.push(DiffHunk {
            base_range: LineRange {
                start: base_start,
                count: base_end.saturating_sub(base_start),
            },
            target_range: LineRange {
                start: target_start,
                count: target_end.saturating_sub(target_start),
            },
            changes,
        });
    }

    let total_changes = insertions + deletions;

    Ok(DiffResult {
        hunks,
        stats: DiffStats {
            total_changes,
            insertions,
            deletions,
        },
    })
}

/// Compute diffs for multiple targets against the same base.
///
/// Each target is keyed by its `ContentHash`. Errors are captured per-target
/// (e.g., one target may have binary content while others diff cleanly).
pub fn compute_batch_diff(
    base: &str,
    targets: &[(ContentHash, &str)],
    context_lines: usize,
) -> BTreeMap<ContentHash, Result<DiffResult, DiffError>> {
    targets
        .iter()
        .map(|(hash, target)| (hash.clone(), compute_diff(base, target, context_lines)))
        .collect()
}
