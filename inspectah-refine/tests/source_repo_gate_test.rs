use inspectah_core::types::rpm::RpmSection;
use std::fs;
use std::path::PathBuf;

#[test]
fn test_source_repo_populated_in_golden_data() {
    // Path is relative to workspace root when running cargo test
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.pop(); // Go up from inspectah-refine to workspace root
    path.push("testdata/golden/go-v13-rpm-section.json");
    let content = fs::read_to_string(&path)
        .expect("golden test data must exist");
    let rpm: RpmSection = serde_json::from_str(&content)
        .expect("golden data must deserialize");
    let packages_with_repo: Vec<_> = rpm.packages_added.iter()
        .filter(|p| !p.source_repo.is_empty()).collect();
    assert!(!packages_with_repo.is_empty(),
        "golden data must have packages with non-empty source_repo");
    let known_repos: Vec<_> = packages_with_repo.iter()
        .map(|p| p.source_repo.as_str())
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();
    assert!(known_repos.len() >= 2,
        "golden data should have multiple distinct repos, got: {:?}", known_repos);
}
