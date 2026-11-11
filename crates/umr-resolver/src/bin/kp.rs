//! `kp` — Knowledge Pack maintenance tool (maintainers only).
//!
//! ```text
//! kp keygen <secret-key-file> <public-key-file>   create a signing key pair
//! kp sign <pack.json> <secret-key-file>            write <pack.json>.sig
//! kp verify <pack.json>                            check signature + validation
//! ```
//! The secret key file is written with owner-only permissions and must never be committed.

use std::path::Path;
use std::process::ExitCode;

use umr_resolver::pack::{generate_keypair, sign, Pack};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let result = match args.as_slice() {
        ["keygen", secret, public] => keygen(Path::new(secret), Path::new(public)),
        ["sign", pack, secret] => sign_pack(Path::new(pack), Path::new(secret)),
        ["verify", pack] => verify(Path::new(pack)),
        _ => Err("usage: kp keygen <secret> <public> | kp sign <pack.json> <secret> | kp verify <pack.json>".into()),
    };
    match result {
        Ok(msg) => {
            println!("{msg}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("kp: {e}");
            ExitCode::FAILURE
        }
    }
}

fn keygen(secret: &Path, public: &Path) -> Result<String, String> {
    if secret.exists() {
        return Err(format!(
            "{} already exists; refusing to overwrite a signing key",
            secret.display()
        ));
    }
    let (s, p) = generate_keypair().map_err(|e| e.to_string())?;
    if let Some(dir) = secret.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    #[cfg(windows)]
    umr_platform_write(secret, s.as_bytes())?;
    #[cfg(not(windows))]
    std::fs::write(secret, s.as_bytes()).map_err(|e| e.to_string())?;
    std::fs::write(public, format!("{p}\n")).map_err(|e| e.to_string())?;
    Ok(format!(
        "secret key: {} (owner-only)\npublic key: {} ({p})",
        secret.display(),
        public.display()
    ))
}

#[cfg(windows)]
fn umr_platform_write(path: &Path, data: &[u8]) -> Result<(), String> {
    umr_platform::secure_file::write_private(path, data).map_err(|e| e.to_string())
}

fn sign_pack(pack: &Path, secret: &Path) -> Result<String, String> {
    let data = std::fs::read(pack).map_err(|e| format!("{}: {e}", pack.display()))?;
    // Never sign an invalid pack.
    let parsed: Pack =
        serde_json::from_slice(&data).map_err(|e| format!("pack is not valid: {e}"))?;
    let errors = parsed.validate();
    if !errors.is_empty() {
        return Err(format!("pack has errors:\n  {}", errors.join("\n  ")));
    }
    let key = std::fs::read_to_string(secret).map_err(|e| format!("{}: {e}", secret.display()))?;
    let sig = sign(&data, &key)?;
    let sig_path = format!("{}.sig", pack.display());
    std::fs::write(&sig_path, format!("{sig}\n")).map_err(|e| e.to_string())?;
    Ok(format!(
        "signed {} (pack_version {}) -> {sig_path}",
        pack.display(),
        parsed.pack_version
    ))
}

fn verify(pack: &Path) -> Result<String, String> {
    let json = std::fs::read_to_string(pack).map_err(|e| format!("{}: {e}", pack.display()))?;
    let sig = std::fs::read_to_string(format!("{}.sig", pack.display()))
        .map_err(|e| format!("signature: {e}"))?;
    let p = Pack::load_verified(&json, &sig).map_err(|e| e.to_string())?;
    Ok(format!(
        "OK: pack_version {}, {} AGP rules, {} Gradle/Java rules, {} sources",
        p.pack_version,
        p.agp.len(),
        p.gradle_java.len(),
        p.sources.len()
    ))
}
