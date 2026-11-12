use std::{env, ffi::OsString, fs, path::Path, process::ExitCode};

use base64::{engine::general_purpose::STANDARD, Engine};
use minisign_verify::{PublicKey, Signature};

fn decode_wrapped(value: &str) -> Result<String, String> {
    let bytes = STANDARD.decode(value).map_err(|e| e.to_string())?;
    String::from_utf8(bytes).map_err(|e| e.to_string())
}

fn load_key(path: &Path) -> Result<PublicKey, String> {
    let config = fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let config: serde_json::Value = serde_json::from_slice(&config)
        .map_err(|e| format!("{}: invalid JSON: {e}", path.display()))?;
    let key = config
        .pointer("/plugins/updater/pubkey")
        .and_then(|v| v.as_str())
        .ok_or_else(|| format!("{}: missing updater public key", path.display()))?;
    let decoded = decode_wrapped(key).map_err(|e| {
        format!(
            "{}: invalid updater public key encoding: {e}",
            path.display()
        )
    })?;
    PublicKey::decode(&decoded)
        .map_err(|e| format!("{}: invalid updater public key: {e}", path.display()))
}

fn verify_asset(key: &PublicKey, asset: &Path, signature_path: &Path) -> Result<(), String> {
    let data = fs::read(asset).map_err(|e| format!("{}: {e}", asset.display()))?;
    let wrapped = fs::read_to_string(signature_path).map_err(|e| {
        format!(
            "{} (signature {}): {e}",
            asset.display(),
            signature_path.display()
        )
    })?;
    let decoded = decode_wrapped(wrapped.trim()).map_err(|e| {
        format!(
            "{} (signature {}): invalid encoding: {e}",
            asset.display(),
            signature_path.display()
        )
    })?;
    let signature = Signature::decode(&decoded).map_err(|e| {
        format!(
            "{} (signature {}): invalid signature: {e}",
            asset.display(),
            signature_path.display()
        )
    })?;
    key.verify(&data, &signature, true).map_err(|e| {
        format!(
            "{} (signature {}): verification failed: {e}",
            asset.display(),
            signature_path.display()
        )
    })
}

fn run(args: &[OsString]) -> Result<(), String> {
    if args.len() < 3 || (args.len() - 1) % 2 != 0 {
        return Err(
            "usage: verify-updater CONFIG.json ASSET SIGNATURE [ASSET SIGNATURE ...]".into(),
        );
    }
    let key = load_key(Path::new(&args[0]))?;
    for pair in args[1..].chunks_exact(2) {
        let asset = Path::new(&pair[0]);
        verify_asset(&key, asset, Path::new(&pair[1]))?;
        println!("verified: {}", asset.display());
    }
    Ok(())
}

fn main() -> ExitCode {
    match run(&env::args_os().skip(1).collect::<Vec<_>>()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    // Public test vectors from minisign-verify 0.2.5, not release credentials.
    const KEY: &str = "untrusted comment: minisign public key E7620F1842B4E81F\nRWQf6LRCGA9i53mlYecO4IzT51TGPpvWucNSCh1CBM0QTaLn73Y7GFO3";
    const LEGACY: &str = "untrusted comment: signature from minisign secret key\nRWQf6LRCGA9i59SLOFxz6NxvASXDJeRtuZykwQepbDEGt87ig1BNpWaVWuNrm73YiIiJbq71Wi+dP9eKL8OC351vwIasSSbXxwA=\ntrusted comment: timestamp:1555779966\tfile:test\nQtKMXWyYcwdpZAlPF7tE2ENJkRd1ujvKjlj1m9RtHTBnZPa5WKU5uWRs5GoP5M/VqE81QFuMKI5k/SfNQUaOAA==";
    const PREHASHED: &str = "untrusted comment: signature from minisign secret key\nRUQf6LRCGA9i559r3g7V1qNyJDApGip8MfqcadIgT9CuhV3EMhHoN1mGTkUidF/z7SrlQgXdy8ofjb7bNJJylDOocrCo8KLzZwo=\ntrusted comment: timestamp:1556193335\tfile:test\ny/rUw2y8/hOUYjZU71eHp/Wo1KZ40fGy2VJEDl34XMJM+TX48Ss/17u3IvIfbVR1FkZZSNCisQbuQY+bHwhEBg==";

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let path =
                env::temp_dir().join(format!("verify-updater-test-{}", uuid::Uuid::new_v4()));
            fs::create_dir(&path).unwrap();
            let fixture = Self(path);
            fixture.write(
                "config",
                &serde_json::json!({"plugins":{"updater":{"pubkey":STANDARD.encode(KEY)}}})
                    .to_string(),
            );
            fixture.write("asset", "test");
            fixture.write("sig", &STANDARD.encode(PREHASHED));
            fixture
        }
        fn path(&self, name: &str) -> PathBuf {
            self.0.join(name)
        }
        fn write(&self, name: &str, value: &str) {
            fs::write(self.path(name), value).unwrap();
        }
        fn args(&self) -> Vec<OsString> {
            ["config", "asset", "sig"]
                .iter()
                .map(|name| self.path(name).into_os_string())
                .collect()
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }

    #[test]
    fn verifies_multiple_assets_and_both_signature_algorithms() {
        let f = Fixture::new();
        f.write("other", "test");
        f.write("legacy", &format!("{}\n", STANDARD.encode(LEGACY)));
        let mut args = f.args();
        args.extend([
            f.path("other").into_os_string(),
            f.path("legacy").into_os_string(),
        ]);
        assert!(run(&args).is_ok());
    }

    #[test]
    fn rejects_tampered_asset_and_identifies_it() {
        let f = Fixture::new();
        f.write("asset", "Test");
        assert!(run(&f.args())
            .unwrap_err()
            .contains(&f.path("asset").display().to_string()));
    }

    #[test]
    fn rejects_wrong_public_key() {
        let f = Fixture::new();
        let mut key = STANDARD.decode(KEY.lines().nth(1).unwrap()).unwrap();
        for byte in [2, 10] {
            key[byte] ^= 1;
            let public_key = format!("untrusted comment: test key\n{}", STANDARD.encode(&key));
            f.write(
                "config",
                &serde_json::json!({"plugins":{"updater":{"pubkey":STANDARD.encode(public_key)}}})
                    .to_string(),
            );
            assert!(run(&f.args()).unwrap_err().contains("verification failed"));
            key[byte] ^= 1;
        }
    }

    #[test]
    fn rejects_corrupt_and_unwrapped_signatures() {
        let f = Fixture::new();
        for value in [
            "!",
            LEGACY,
            &STANDARD.encode("bad"),
            &STANDARD.encode([0xff]),
        ] {
            f.write("sig", value);
            let error = run(&f.args()).unwrap_err();
            assert!(
                error.contains(&f.path("asset").display().to_string()),
                "{error}"
            );
            assert!(
                error.contains(&f.path("sig").display().to_string()),
                "{error}"
            );
        }
        for corrupted in [
            PREHASHED.replace("559r", "55Ar"),
            PREHASHED.replace("file:test", "file:evil"),
        ] {
            f.write("sig", &STANDARD.encode(corrupted));
            assert!(run(&f.args()).is_err());
        }
    }

    #[test]
    fn rejects_invalid_configurations() {
        let f = Fixture::new();
        for value in [
            "{",
            "{}",
            r#"{"plugins":{"updater":{"pubkey":2}}}"#,
            r#"{"plugins":{"updater":{"pubkey":"!"}}}"#,
            &serde_json::json!({"plugins":{"updater":{"pubkey":STANDARD.encode([0xff])}}})
                .to_string(),
            &serde_json::json!({"plugins":{"updater":{"pubkey":STANDARD.encode("bad")}}})
                .to_string(),
        ] {
            f.write("config", value);
            assert!(run(&f.args())
                .unwrap_err()
                .contains(&f.path("config").display().to_string()));
        }
        assert!(load_key(&f.path("missing"))
            .err()
            .unwrap()
            .contains("missing"));
    }

    #[test]
    fn rejects_bad_arguments_and_missing_files() {
        let f = Fixture::new();
        assert!(run(&[]).unwrap_err().contains("usage"));
        assert!(run(&f.args()[..2]).unwrap_err().contains("usage"));
        let mut args = f.args();
        args[1] = f.path("missing-asset").into_os_string();
        assert!(run(&args).unwrap_err().contains("missing-asset"));
        args[1] = f.path("asset").into_os_string();
        args[2] = f.path("missing-signature").into_os_string();
        assert!(run(&args).unwrap_err().contains("missing-signature"));
    }
}
