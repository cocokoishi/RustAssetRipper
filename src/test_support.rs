//! Test-only filesystem helpers shared by the unit-test modules.
//!
//! Tests that need real files must not assume the process-wide temporary directory is
//! writable. Sandboxed hosts (including this project's Windows gate session) can deny
//! directory creation outside the checkout while still allowing writes inside it, which
//! turned every filesystem-backed test into a spurious `PermissionDenied` failure. The
//! helpers below resolve exactly one writable root per test process - preferring
//! `RUSTASSETRIPPER_TEST_TMP`, then the OS temporary directory, then
//! `<checkout>/target/test-tmp` - and hand out unique paths below it.
//!
//! The assertions in the tests themselves are unchanged; only the location of their
//! scratch files becomes deterministic and sandbox-safe.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;

static ROOT: OnceLock<PathBuf> = OnceLock::new();
static NEXT_PATH_ID: AtomicU64 = AtomicU64::new(0);

/// The writable root every filesystem-backed test places its artifacts under.
pub(crate) fn test_temp_root() -> &'static Path {
    ROOT.get_or_init(resolve_test_temp_root).as_path()
}

/// A unique path below [`test_temp_root`] named `assetripper-{tag}-{pid}-{id}.{extension}`.
pub(crate) fn unique_test_path(tag: &str, extension: &str) -> PathBuf {
    let id = NEXT_PATH_ID.fetch_add(1, Ordering::Relaxed);
    test_temp_root().join(format!(
        "assetripper-{tag}-{}-{id}.{extension}",
        std::process::id()
    ))
}

/// Create a unique directory below [`test_temp_root`] and return its path.
pub(crate) fn unique_test_directory(tag: &str) -> PathBuf {
    let path = unique_test_path(tag, "dir");
    std::fs::create_dir(&path).expect("the resolved test temporary root accepts a new directory");
    path
}

fn resolve_test_temp_root() -> PathBuf {
    if let Some(configured) = std::env::var_os("RUSTASSETRIPPER_TEST_TMP") {
        let configured = PathBuf::from(configured);
        if ensure_writable(&configured) {
            return configured;
        }
        panic!(
            "RUSTASSETRIPPER_TEST_TMP={} is not writable",
            configured.display()
        );
    }

    let candidates = [
        std::env::temp_dir(),
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test-tmp"),
    ];
    for candidate in candidates {
        if ensure_writable(&candidate) {
            return candidate;
        }
    }

    panic!("no writable test temporary directory; set RUSTASSETRIPPER_TEST_TMP to a writable path");
}

fn ensure_writable(directory: &Path) -> bool {
    if std::fs::create_dir_all(directory).is_err() {
        return false;
    }
    let probe = directory.join(format!(
        "assetripper-write-probe-{}.tmp",
        std::process::id()
    ));
    match std::fs::write(&probe, b"") {
        Ok(()) => {
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::{test_temp_root, unique_test_directory, unique_test_path};

    #[test]
    fn the_test_temp_root_is_writable_and_stable() {
        let root = test_temp_root();
        assert!(root.is_absolute(), "{} is not absolute", root.display());
        assert_eq!(root, test_temp_root());
        let probe = root.join(format!("assetripper-root-probe-{}.tmp", std::process::id()));
        std::fs::write(&probe, b"probe").unwrap();
        assert_eq!(std::fs::read(&probe).unwrap(), b"probe");
        std::fs::remove_file(&probe).unwrap();
    }

    #[test]
    fn unique_paths_and_directories_do_not_collide() {
        let first = unique_test_path("collision", "bin");
        let second = unique_test_path("collision", "bin");
        assert_ne!(first, second);
        let name = first.file_name().unwrap().to_string_lossy().into_owned();
        assert!(name.starts_with(&format!("assetripper-collision-{}-", std::process::id())));
        assert!(name.ends_with(".bin"));

        let directory = unique_test_directory("collision-dir");
        assert!(directory.is_dir());
        std::fs::write(directory.join("child"), b"x").unwrap();
        std::fs::remove_dir_all(&directory).unwrap();
    }
}
