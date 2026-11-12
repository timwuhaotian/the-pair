use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

use base64::{engine::general_purpose::STANDARD, Engine};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("verify-updater-cli-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn cli_verifies_multiple_assets_and_reports_failure_with_nonzero_exit() {
    let temp = TempDir::new();
    let dir = &temp.0;
    let key = "untrusted comment: public test key\nRWQf6LRCGA9i53mlYecO4IzT51TGPpvWucNSCh1CBM0QTaLn73Y7GFO3";
    let signature = "untrusted comment: public test signature\nRUQf6LRCGA9i559r3g7V1qNyJDApGip8MfqcadIgT9CuhV3EMhHoN1mGTkUidF/z7SrlQgXdy8ofjb7bNJJylDOocrCo8KLzZwo=\ntrusted comment: timestamp:1556193335\tfile:test\ny/rUw2y8/hOUYjZU71eHp/Wo1KZ40fGy2VJEDl34XMJM+TX48Ss/17u3IvIfbVR1FkZZSNCisQbuQY+bHwhEBg==";
    let config = dir.join("config.json");
    let sig = dir.join("asset.sig");
    let first = dir.join("first");
    let second = dir.join("second");
    fs::write(
        &config,
        serde_json::json!({"plugins":{"updater":{"pubkey":STANDARD.encode(key)}}}).to_string(),
    )
    .unwrap();
    fs::write(&sig, STANDARD.encode(signature)).unwrap();
    fs::write(&first, "test").unwrap();
    fs::write(&second, "test").unwrap();
    let invoke = || {
        Command::new(env!("CARGO_BIN_EXE_verify-updater"))
            .arg(&config)
            .arg(&first)
            .arg(&sig)
            .arg(&second)
            .arg(&sig)
            .output()
            .unwrap()
    };
    let output = invoke();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout).lines().count(), 2);
    fs::write(&second, "tampered").unwrap();
    let output = invoke();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains(&second.display().to_string()));
    let usage = Command::new(env!("CARGO_BIN_EXE_verify-updater"))
        .output()
        .unwrap();
    assert!(!usage.status.success());
    assert!(String::from_utf8_lossy(&usage.stderr).contains("usage:"));
}

fn assert_success(output: Output) {
    assert!(
        output.status.success(),
        "command failed: {}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn node_script(name: &str, cwd: &Path, args: &[&str]) {
    let script = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("scripts")
        .join(name);
    let output = Command::new("node")
        .arg(script)
        .args(args)
        .current_dir(cwd)
        .env_remove("NODE_OPTIONS")
        .env_remove("NODE_PATH")
        .output()
        .unwrap();
    assert_success(output);
}

fn write_config(path: &Path, key: &str) {
    fs::write(
        path,
        serde_json::json!({"plugins": {"updater": {"pubkey": STANDARD.encode(key)}}}).to_string(),
    )
    .unwrap();
}

#[test]
fn offline_release_pipeline_verifies_assets_and_manifest() {
    // Public minisign-verify 0.2.5 vectors; these are not release credentials.
    let key = "untrusted comment: public test key\nRWQf6LRCGA9i53mlYecO4IzT51TGPpvWucNSCh1CBM0QTaLn73Y7GFO3";
    let prehashed = "untrusted comment: public test signature\nRUQf6LRCGA9i559r3g7V1qNyJDApGip8MfqcadIgT9CuhV3EMhHoN1mGTkUidF/z7SrlQgXdy8ofjb7bNJJylDOocrCo8KLzZwo=\ntrusted comment: timestamp:1556193335\tfile:test\ny/rUw2y8/hOUYjZU71eHp/Wo1KZ40fGy2VJEDl34XMJM+TX48Ss/17u3IvIfbVR1FkZZSNCisQbuQY+bHwhEBg==";
    let legacy = "untrusted comment: public legacy signature\nRWQf6LRCGA9i59SLOFxz6NxvASXDJeRtuZykwQepbDEGt87ig1BNpWaVWuNrm73YiIiJbq71Wi+dP9eKL8OC351vwIasSSbXxwA=\ntrusted comment: timestamp:1555779966\tfile:test\nQtKMXWyYcwdpZAlPF7tE2ENJkRd1ujvKjlj1m9RtHTBnZPa5WKU5uWRs5GoP5M/VqE81QFuMKI5k/SfNQUaOAA==";
    let linux_signature = prehashed.replace("public test signature", "public linux test signature");
    let platforms = [
        (
            "macos",
            "macos",
            "sample.app.tar.gz",
            "the-pair-1.2.3.app.tar.gz",
            prehashed,
        ),
        (
            "windows",
            "nsis",
            "sample-setup.exe",
            "the-pair-1.2.3-setup.exe",
            legacy,
        ),
        (
            "linux",
            "appimage",
            "sample.AppImage.tar.gz",
            "the-pair-1.2.3.AppImage.tar.gz",
            linux_signature.as_str(),
        ),
    ];
    let temp = TempDir::new();
    let root = &temp.0;
    let bundle = root.join("bundle");
    let dist = root.join("dist");
    let config = root.join("config.json");
    write_config(&config, key);

    for (platform, subdir, source, asset, signature) in platforms {
        let dir = bundle.join(subdir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(source), b"test").unwrap();
        fs::write(
            dir.join(format!("{source}.sig")),
            STANDARD.encode(signature),
        )
        .unwrap();
        node_script(
            "release-updater-assets.mjs",
            root,
            &[
                "--platform",
                platform,
                "--bundle-root",
                bundle.to_str().unwrap(),
                "--out-dir",
                dist.to_str().unwrap(),
                "--version",
                "1.2.3",
            ],
        );
        assert_eq!(fs::read(dist.join(asset)).unwrap(), b"test");
        assert_eq!(
            fs::read_to_string(dist.join(format!("{asset}.sig"))).unwrap(),
            STANDARD.encode(signature)
        );
    }

    let verify = |config: &Path| {
        let mut command = Command::new(env!("CARGO_BIN_EXE_verify-updater"));
        command.current_dir(root).arg(config);
        for (_, _, _, asset, _) in platforms {
            command
                .arg(dist.join(asset))
                .arg(dist.join(format!("{asset}.sig")));
        }
        command.output().unwrap()
    };
    let output = verify(&config);
    assert_eq!(String::from_utf8_lossy(&output.stdout).lines().count(), 3);
    assert_success(output);

    let base_url = "https://release.invalid/download/v1.2.3";
    let manifest_path = root.join("latest.json");
    node_script(
        "generate-updater-manifest.mjs",
        root,
        &[
            "--version",
            "1.2.3",
            "--base-url",
            base_url,
            "--dist-dir",
            dist.to_str().unwrap(),
            "--output",
            manifest_path.to_str().unwrap(),
            "--pub-date",
            "2026-01-01T00:00:00Z",
        ],
    );
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(manifest_path).unwrap()).unwrap();
    assert_eq!(manifest["version"], "1.2.3");
    let entries = manifest["platforms"].as_object().unwrap();
    assert_eq!(entries.len(), 4);
    for (target, asset) in [
        ("darwin-x86_64", platforms[0].3),
        ("darwin-aarch64", platforms[0].3),
        ("windows-x86_64", platforms[1].3),
        ("linux-x86_64", platforms[2].3),
    ] {
        let entry = &entries[target];
        assert_eq!(entry["url"], format!("{base_url}/{asset}"));
        let signature = entry["signature"].as_str().unwrap();
        assert_eq!(
            signature,
            fs::read_to_string(dist.join(format!("{asset}.sig"))).unwrap()
        );
        let manifest_signature = root.join(format!("manifest-{target}.sig"));
        fs::write(&manifest_signature, signature).unwrap();
        assert_success(
            Command::new(env!("CARGO_BIN_EXE_verify-updater"))
                .current_dir(root)
                .arg(&config)
                .arg(dist.join(asset))
                .arg(manifest_signature)
                .output()
                .unwrap(),
        );
    }

    for (_, _, _, asset, _) in platforms {
        let path = dist.join(asset);
        fs::write(&path, b"tampered").unwrap();
        let output = verify(&config);
        assert!(!output.status.success());
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(
            error.contains("verification failed") && error.contains(&path.display().to_string()),
            "{error}"
        );
        fs::write(path, b"test").unwrap();
    }

    let mut wrong_key = STANDARD.decode(key.lines().nth(1).unwrap()).unwrap();
    wrong_key[10] ^= 1;
    let wrong_key = format!(
        "untrusted comment: wrong test key\n{}",
        STANDARD.encode(wrong_key)
    );
    write_config(&config, &wrong_key);
    let output = verify(&config);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("verification failed"));
}
