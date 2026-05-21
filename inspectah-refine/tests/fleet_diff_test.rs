use inspectah_refine::fleet::diff::{compute_batch_diff, compute_diff, ChangeKind, DiffError};
use inspectah_refine::types::ContentHash;

#[test]
fn identical_content_empty_hunks() {
    let result = compute_diff("hello\nworld\n", "hello\nworld\n", 3).unwrap();
    assert!(result.hunks.is_empty());
    assert_eq!(result.stats.total_changes, 0);
    assert_eq!(result.stats.insertions, 0);
    assert_eq!(result.stats.deletions, 0);
}

#[test]
fn single_line_change_produces_hunk() {
    let result = compute_diff("a\nb\nc\n", "a\nB\nc\n", 3).unwrap();
    assert!(!result.hunks.is_empty());
    assert_eq!(result.stats.insertions, 1);
    assert_eq!(result.stats.deletions, 1);
}

#[test]
fn empty_base_all_inserts() {
    let result = compute_diff("", "line1\nline2\n", 3).unwrap();
    assert_eq!(result.stats.insertions, 2);
    assert_eq!(result.stats.deletions, 0);
}

#[test]
fn binary_content_rejected() {
    let result = compute_diff("hello\0world", "other", 3);
    assert!(matches!(result, Err(DiffError::BinaryContent)));
}

#[test]
fn binary_in_target_rejected() {
    let result = compute_diff("clean text", "has\0null", 3);
    assert!(matches!(result, Err(DiffError::BinaryContent)));
}

#[test]
fn input_too_large_rejected() {
    let large = "x\n".repeat(60_000); // >100KB
    let result = compute_diff(&large, "small", 3);
    assert!(matches!(result, Err(DiffError::InputTooLarge)));
}

#[test]
fn context_lines_trims_equal_runs() {
    let base = (0..100).map(|i| format!("line{i}\n")).collect::<String>();
    let target = base.replace("line50\n", "CHANGED\n");
    let result = compute_diff(&base, &target, 3).unwrap();
    let equal_count: usize = result
        .hunks
        .iter()
        .flat_map(|h| &h.changes)
        .filter(|c| c.kind == ChangeKind::Equal)
        .count();
    assert!(
        equal_count <= 7,
        "at most 3+3 context + boundary, got {equal_count}"
    );
}

#[test]
fn batch_diff_multiple_targets() {
    let t1 = ContentHash::from_content(b"a\nB\nc\n");
    let t2 = ContentHash::from_content(b"a\nb\nC\n");
    let results = compute_batch_diff(
        "a\nb\nc\n",
        &[(t1.clone(), "a\nB\nc\n"), (t2.clone(), "a\nb\nC\n")],
        3,
    );
    assert_eq!(results.len(), 2);
    assert!(results[&t1].is_ok());
    assert!(results[&t2].is_ok());
}

#[test]
fn batch_diff_per_target_error() {
    let good = ContentHash::from_content(b"clean");
    let bad = ContentHash::from_content(b"has\0null");
    let results = compute_batch_diff(
        "base text",
        &[(good.clone(), "clean"), (bad.clone(), "has\0null")],
        3,
    );
    assert!(results[&good].is_ok());
    assert!(results[&bad].is_err());
}
