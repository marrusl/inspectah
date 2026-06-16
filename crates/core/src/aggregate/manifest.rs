use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Deserialize)]
pub struct AggregateManifest {
    pub label: Option<String>,
    pub target_image: Option<String>,
    pub sources: Vec<PathBuf>,
}

impl AggregateManifest {
    /// Parse a TOML string into an AggregateManifest.
    pub fn parse(content: &str) -> Result<AggregateManifest, toml::de::Error> {
        toml::from_str(content)
    }

    /// Load a manifest from a file, resolving source paths relative to the manifest's parent directory.
    pub fn load(path: &Path) -> Result<AggregateManifest, Box<dyn std::error::Error>> {
        let content = std::fs::read_to_string(path)?;
        let mut manifest = Self::parse(&content)?;

        // Resolve source paths relative to the manifest file's parent directory
        if let Some(parent) = path.parent() {
            manifest.sources = manifest
                .sources
                .into_iter()
                .map(|source| {
                    if source.is_relative() {
                        parent.join(source)
                    } else {
                        source
                    }
                })
                .collect();
        }

        Ok(manifest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_minimal() {
        let toml = r#"sources = ["a.tar.gz", "b.tar.gz"]"#;
        let m = AggregateManifest::parse(toml).unwrap();
        assert_eq!(m.sources.len(), 2);
        assert!(m.label.is_none());
        assert!(m.target_image.is_none());
    }

    #[test]
    fn test_parse_full() {
        let toml = r#"
label = "web-servers"
target_image = "host-a"
sources = ["scans/a.tar.gz", "scans/b.tar.gz"]
"#;
        let m = AggregateManifest::parse(toml).unwrap();
        assert_eq!(m.label.as_deref(), Some("web-servers"));
        assert_eq!(m.target_image.as_deref(), Some("host-a"));
    }

    #[test]
    fn test_load_resolves_paths() {
        // Create a temp dir with a manifest file
        let dir = tempfile::tempdir().unwrap();
        let manifest_path = dir.path().join("aggregate.toml");
        std::fs::write(&manifest_path, r#"sources = ["scans/a.tar.gz"]"#).unwrap();
        let m = AggregateManifest::load(&manifest_path).unwrap();
        assert_eq!(m.sources[0], dir.path().join("scans/a.tar.gz"));
    }
}
