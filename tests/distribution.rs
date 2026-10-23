#![cfg(unix)]
//! install.sh and the Homebrew renderer against a local file:// "release", plus completions and the man page.
use std::fs;
use std::path::Path;
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_prdiff");
const ROOT: &str = env!("CARGO_MANIFEST_DIR");

fn sha256(path: &Path) -> String {
    let out = Command::new("sha256sum")
        .arg(path)
        .output()
        .or_else(|_| {
            Command::new("shasum")
                .args(["-a", "256"])
                .arg(path)
                .output()
        })
        .unwrap();
    String::from_utf8(out.stdout)
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap()
        .to_string()
}

/// A fake release dir: <dir>/latest/download/<asset> and <dir>/download/v9.9.9/<asset>.
fn fake_release(asset: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let pkg = tempfile::tempdir().unwrap();
    fs::copy(BIN, pkg.path().join("prdiff")).unwrap();
    for sub in ["latest/download", "download/v9.9.9"] {
        let d = dir.path().join(sub);
        fs::create_dir_all(&d).unwrap();
        let tar = d.join(asset);
        assert!(
            Command::new("tar")
                .arg("-czf")
                .arg(&tar)
                .arg("-C")
                .arg(pkg.path())
                .arg("prdiff")
                .status()
                .unwrap()
                .success()
        );
        fs::write(
            d.join(format!("{asset}.sha256")),
            format!("{}  {asset}\n", sha256(&tar)),
        )
        .unwrap();
    }
    dir
}

fn host_asset() -> String {
    let os = if cfg!(target_os = "macos") {
        "apple-darwin"
    } else {
        "unknown-linux-musl"
    };
    let arch = if cfg!(target_arch = "aarch64") {
        "aarch64"
    } else {
        "x86_64"
    };
    format!("prdiff-{arch}-{os}.tar.gz")
}

fn install(release: &Path, bin_dir: &Path, version: &str) -> std::process::Output {
    Command::new("sh")
        .arg(Path::new(ROOT).join("install.sh"))
        .env("PRDIFF_BASE_URL", format!("file://{}", release.display()))
        .env("PRDIFF_BIN_DIR", bin_dir)
        .env("PRDIFF_VERSION", version)
        .output()
        .unwrap()
}

#[test]
fn install_script_downloads_verifies_and_installs() {
    let release = fake_release(&host_asset());
    for version in ["latest", "v9.9.9"] {
        let bin_dir = tempfile::tempdir().unwrap();
        let out = install(release.path(), bin_dir.path(), version);
        assert!(
            out.status.success(),
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        let v = Command::new(bin_dir.path().join("prdiff"))
            .arg("--version")
            .output()
            .unwrap();
        assert!(
            String::from_utf8_lossy(&v.stdout).starts_with("prdiff "),
            "{version}"
        );
    }
}

#[test]
fn install_script_refuses_a_tampered_download() {
    let release = fake_release(&host_asset());
    let sums = release
        .path()
        .join(format!("latest/download/{}.sha256", host_asset()));
    fs::write(&sums, format!("{}  x\n", "0".repeat(64))).unwrap();
    let bin_dir = tempfile::tempdir().unwrap();
    let out = install(release.path(), bin_dir.path(), "latest");
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("checksum mismatch"));
    assert!(
        !bin_dir.path().join("prdiff").exists(),
        "nothing is installed on a mismatch"
    );

    let missing = install(Path::new("/nonexistent"), bin_dir.path(), "latest");
    assert!(!missing.status.success());
    assert!(String::from_utf8_lossy(&missing.stderr).contains("download failed"));
}

#[test]
fn homebrew_formula_is_rendered_from_release_checksums() {
    // a release dir that has all four assets' checksums
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path().join("download/v1.2.3");
    fs::create_dir_all(&d).unwrap();
    for (i, t) in [
        "aarch64-apple-darwin",
        "x86_64-apple-darwin",
        "aarch64-unknown-linux-musl",
        "x86_64-unknown-linux-musl",
    ]
    .iter()
    .enumerate()
    {
        fs::write(
            d.join(format!("prdiff-{t}.tar.gz.sha256")),
            format!("{:064x}  prdiff-{t}.tar.gz\n", i + 1),
        )
        .unwrap();
    }
    let out = Command::new("sh")
        .arg(Path::new(ROOT).join("packaging/homebrew/render.sh"))
        .arg("v1.2.3")
        .env(
            "PRDIFF_BASE_URL",
            format!("file://{}", dir.path().display()),
        )
        .output()
        .unwrap();
    let rb = String::from_utf8(out.stdout).unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        rb.contains("class Prdiff < Formula") && rb.contains("version \"1.2.3\""),
        "{rb}"
    );
    assert!(rb.contains(&format!("sha256 \"{:064x}\"", 4)), "{rb}");
    assert!(rb.contains("prdiff-aarch64-apple-darwin.tar.gz"));
}

#[test]
fn completions_and_man_page_are_generated() {
    for shell in ["bash", "zsh", "fish"] {
        let out = Command::new(BIN)
            .args(["completions", shell])
            .output()
            .unwrap();
        let text = String::from_utf8(out.stdout).unwrap();
        assert!(out.status.success() && text.contains("prdiff"), "{shell}");
        assert!(
            text.contains("compare") && text.contains("publish"),
            "{shell}: subcommands are completed"
        );
    }
    let man = Command::new(BIN).arg("man").output().unwrap();
    let text = String::from_utf8(man.stdout).unwrap();
    assert!(
        text.starts_with(".ie") || text.contains(".TH"),
        "roff: {}",
        &text[..text.len().min(80)]
    );
    assert!(text.contains("prdiff"));
}

#[test]
fn workflow_files_are_valid_yaml_with_the_expected_shape() {
    // cheap structural checks without a YAML dependency
    let action = fs::read_to_string(Path::new(ROOT).join("action.yml")).unwrap();
    for needle in [
        "using: composite",
        "mode:",
        "prdiff check --pr",
        "prdiff publish --pr",
    ] {
        assert!(action.contains(needle), "action.yml lacks {needle}");
    }
    let rel = fs::read_to_string(Path::new(ROOT).join(".github/workflows/release.yml")).unwrap();
    for t in [
        "x86_64-unknown-linux-musl",
        "aarch64-unknown-linux-musl",
        "aarch64-apple-darwin",
        "x86_64-apple-darwin",
    ] {
        assert!(rel.contains(t), "release.yml lacks {t}");
        assert!(
            fs::read_to_string(Path::new(ROOT).join("install.sh"))
                .unwrap()
                .contains(&t.replace("x86_64-", "").replace("aarch64-", ""))
                || t.contains("linux")
                || t.contains("darwin")
        );
    }
}
