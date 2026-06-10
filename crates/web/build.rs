use std::env;
use std::path::Path;
use std::process::Command;

fn main() {
    // Allow skipping UI build entirely for Rust-only development
    if env::var("INSPECTAH_SKIP_UI").is_ok() {
        return;
    }

    let ui_dir = Path::new("ui");
    println!("cargo:rerun-if-changed=ui/src");
    println!("cargo:rerun-if-changed=ui/package.json");
    println!("cargo:rerun-if-changed=ui/package-lock.json");
    println!("cargo:rerun-if-changed=ui/vite.config.ts");
    println!("cargo:rerun-if-changed=ui/tsconfig.json");
    println!("cargo:rerun-if-changed=ui/index.html");

    let dist_dir = ui_dir.join("dist");
    let has_npm = Command::new("npm").arg("--version").output().is_ok();

    if has_npm {
        let status = Command::new("npm")
            .arg("ci")
            .current_dir(ui_dir)
            .status()
            .expect("failed to run npm ci");
        assert!(status.success(), "npm ci failed");

        let status = Command::new("npm")
            .args(["run", "build"])
            .current_dir(ui_dir)
            .status()
            .expect("failed to run npm run build");
        assert!(status.success(), "npm run build failed");
    } else if dist_dir.exists() {
        // No Node — use pre-built assets if available
    } else {
        panic!(
            "No Node.js found and no pre-built ui/dist/ exists. \
             Install Node.js or provide pre-built assets."
        );
    }
}
