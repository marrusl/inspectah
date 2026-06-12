/// Extract a package section stat from a view JSON response.
fn pkg_stat(view: &serde_json::Value, field: &str) -> i64 {
    view["stats"]["sections"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["kind"] == "package")
        .and_then(|s| s[field].as_i64())
        .unwrap_or(0)
}

/// Create a minimal test tarball with a FullyRedacted snapshot.
fn create_test_tarball(dir: &std::path::Path) -> std::path::PathBuf {
    let snap = serde_json::json!({
        "schema_version": 19,
        "rpm": {
            "packages_added": [
                {
                    "name": "httpd",
                    "arch": "x86_64",
                    "state": "added",
                    "include": true,
                    "source_repo": "appstream"
                }
            ],
            "base_image_only": [],
            "rpm_va": [],
            "repo_files": [],
            "gpg_keys": [],
            "dnf_history_removed": [],
            "version_changes": [],
            "module_streams": [],
            "version_locks": [],
            "module_stream_conflicts": [],
            "multiarch_packages": [],
            "duplicate_packages": [],
            "repo_providing_packages": [],
            "ostree_overrides": [],
            "ostree_removals": [],
            "file_ownership": [],
            "no_baseline": true
        },
        "config": {
            "files": [
                {
                    "path": "/etc/httpd/conf/httpd.conf",
                    "kind": "rpm_owned_modified",
                    "category": "other",
                    "include": true
                }
            ]
        },
        "redaction_state": {
            "state": "fully_redacted",
            "redacted_by": "inspectah 0.8.0",
            "config_hash": "abc123"
        }
    });

    let snap_path = dir.join("inspection-snapshot.json");
    std::fs::write(&snap_path, serde_json::to_string_pretty(&snap).unwrap()).unwrap();

    let tarball_path = dir.join("test-scan.tar.gz");
    let f = std::fs::File::create(&tarball_path).unwrap();
    let gz = flate2::write::GzEncoder::new(f, flate2::Compression::default());
    let mut tar = tar::Builder::new(gz);
    tar.append_path_with_name(&snap_path, "inspection-snapshot.json")
        .unwrap();
    tar.finish().unwrap();
    drop(tar);
    tarball_path
}

#[tokio::test]
async fn refine_server_lifecycle() {
    let tempdir = tempfile::tempdir().unwrap();
    let tarball = create_test_tarball(tempdir.path());

    // Load tarball and create session
    let session = inspectah_refine::tarball::from_tarball(&tarball).unwrap();
    let state = std::sync::Arc::new(inspectah_web::handlers::AppState {
        session: std::sync::Arc::new(std::sync::Mutex::new(session)),
        sections_cache: std::sync::OnceLock::new(),
    });

    // Bind to ephemeral port
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let origin = format!("http://{addr}");

    let app = inspectah_web::router(state.clone(), &origin);

    // Spawn the server
    let server_handle = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let client = reqwest::Client::new();
    let base = format!("http://{addr}");

    // 1. Health check
    let resp = client
        .get(format!("{base}/api/health"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["status"], "ok");

    // 2. Initial view
    let resp = client.get(format!("{base}/api/view")).send().await.unwrap();
    assert_eq!(resp.status(), 200);
    let view: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(view["generation"], 0);
    assert_eq!(pkg_stat(&view, "total"), 1);

    // 3. Apply an operation (include httpd — normalization sets it to
    //    exclude by default because there's no baseline, so including
    //    it is a real state change that bumps generation)
    let resp = client
        .post(format!("{base}/api/op"))
        .json(&serde_json::json!({
            "op": "SetInclude",
            "target": {
                "item_id": {"kind": "Package", "key": {"name": "httpd", "arch": "x86_64"}},
                "include": true
            }
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let view: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(view["generation"], 1);
    assert_eq!(pkg_stat(&view, "included"), 1);

    // 4. Undo (requires JSON body)
    let resp = client
        .post(format!("{base}/api/undo"))
        .json(&serde_json::json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let view: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(view["generation"], 2);
    assert_eq!(pkg_stat(&view, "included"), 0);

    // 5. Redo (requires JSON body)
    let resp = client
        .post(format!("{base}/api/redo"))
        .json(&serde_json::json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    // 6. Changes
    let resp = client
        .get(format!("{base}/api/changes"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let changes: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(changes["is_dirty"], true);

    // 7. Ops history
    let resp = client.get(format!("{base}/api/ops")).send().await.unwrap();
    assert_eq!(resp.status(), 200);
    let ops: Vec<serde_json::Value> = resp.json().await.unwrap();
    assert_eq!(ops.len(), 1);

    // 8. Export with matching generation
    let current_gen = {
        let s = state.session.lock().unwrap();
        s.generation()
    };
    let resp = client
        .post(format!("{base}/api/tarball"))
        .json(&serde_json::json!({"generation": current_gen}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    assert_eq!(
        resp.headers().get("content-type").unwrap(),
        "application/gzip"
    );
    let tarball_bytes = resp.bytes().await.unwrap();
    assert!(!tarball_bytes.is_empty());

    // 9. Export with stale generation -> 409
    let resp = client
        .post(format!("{base}/api/tarball"))
        .json(&serde_json::json!({"generation": 999}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 409);

    // 10. Invalid op -> 422
    let resp = client
        .post(format!("{base}/api/op"))
        .json(&serde_json::json!({
            "op": "SetInclude",
            "target": {
                "item_id": {"kind": "Package", "key": {"name": "nonexistent", "arch": "x86_64"}},
                "include": false
            }
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 422);

    // Clean shutdown
    server_handle.abort();
}
