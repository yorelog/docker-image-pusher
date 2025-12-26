use std::env;
use std::path::PathBuf;
use assert_cmd::Command;

// End-to-end tests that exercise both containerd bundle and docker-save tar flows.
// Both tests are ignored by default and require real artifacts + registry access.
// This requires a real containerd root with the requested image present, and
// network access to push the image. To run locally:
// E2E_IMAGE=registry.cn-beijing.aliyuncs.com/yoce/alpine:3.21 \
// E2E_CONTAINERD_ROOT=~/.local/share/containerd \
// E2E_NAMESPACE=default \
// cargo test e2e_export_and_push -- --ignored
// Optional: E2E_TARGET to override the push destination, E2E_USERNAME/E2E_PASSWORD for auth.
#[test]
fn e2e_export_and_push() {
    let image = match env::var("E2E_IMAGE") {
        Ok(v) => v,
        Err(_) => {
            eprintln!("skipping: set E2E_IMAGE to run");
            return;
        }
    };
    let root = match env::var("E2E_CONTAINERD_ROOT") {
        Ok(v) => v,
        Err(_) => {
            eprintln!("skipping: set E2E_CONTAINERD_ROOT to run");
            return;
        }
    };
    let namespace = env::var("E2E_NAMESPACE").unwrap_or_else(|_| "default".to_string());
    let target = env::var("E2E_TARGET").ok();
    let username = env::var("E2E_USERNAME").ok();
    let password = env::var("E2E_PASSWORD").ok();

    let tmp = tempfile::tempdir().expect("tempdir");
    let out_dir = tmp.path().join("bundle");

    // Export the image(s) into a self-contained bundle (meta.db + blobs).
    let mut export_cmd = Command::cargo_bin("docker-image-pusher").expect("bin");
    export_cmd
        .arg("save")
        .arg("--root")
        .arg(&root)
        .arg("--namespace")
        .arg(&namespace)
        .arg("--out")
        .arg(&out_dir)
        .arg(&image);
    export_cmd.assert().success();

    // Push back from the exported bundle using the same image reference unless overridden.
    let mut push_cmd = Command::cargo_bin("docker-image-pusher").expect("bin");
    push_cmd
        .arg("push")
        .arg("--root")
        .arg(&out_dir)
        .arg("--namespace")
        .arg(&namespace)
        .arg("--image")
        .arg(&image);
    if let Some(t) = target {
        push_cmd.arg("--target").arg(t);
    }
    if let Some(u) = username.as_ref() {
        push_cmd.arg("--username").arg(u);
    }
    if let Some(p) = password.as_ref() {
        push_cmd.arg("--password").arg(p);
    }

    let push_out = push_cmd.assert();
    push_out.success();

    // Ensure we produced some blobs in the bundle as a sanity check.
    let blobs_root = PathBuf::from(&out_dir).join("io.containerd.content.v1.content/blobs");
    assert!(blobs_root.exists(), "blobs root missing: {:?}", blobs_root);
}

// End-to-end test that pushes directly from a docker-save tarball.
// Requires E2E_TAR to point to a docker save archive on disk (e.g., produced by `docker save`).
// Optional: E2E_TARGET to override destination, E2E_USERNAME/E2E_PASSWORD for auth.
#[test]
fn e2e_push_tar() {
    let tar_path = match env::var("E2E_TAR") {
        Ok(v) => v,
        Err(_) => {
            eprintln!("skipping: set E2E_TAR to run");
            return;
        }
    };
    if !PathBuf::from(&tar_path).is_file() {
        eprintln!("skipping: E2E_TAR does not point to a file: {}", tar_path);
        return;
    }

    let target = env::var("E2E_TARGET").ok();
    let username = env::var("E2E_USERNAME").ok();
    let password = env::var("E2E_PASSWORD").ok();

    let mut push_cmd = Command::cargo_bin("docker-image-pusher").expect("bin");
    push_cmd.arg("push").arg("--tar").arg(&tar_path);
    if let Some(t) = target {
        push_cmd.arg("--target").arg(t);
    }
    if let Some(u) = username.as_ref() {
        push_cmd.arg("--username").arg(u);
    }
    if let Some(p) = password.as_ref() {
        push_cmd.arg("--password").arg(p);
    }

    push_cmd.assert().success();
}
