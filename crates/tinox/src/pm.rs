use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Package {
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub description: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Dependency {
    pub group: String,
    #[serde(rename = "artifactId")]
    pub artifact_id: String,
    pub version: String,
    pub url: String,
    /// Expected SHA-256 of the downloaded artifact, lowercase hex. Optional
    /// for backward compatibility with existing tinox.yaml files, but
    /// strongly recommended: without it, `tinox install` only pins against
    /// whatever tinox.lock happens to have recorded (see verify_checksum).
    #[serde(default)]
    pub sha256: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct TinoxManifest {
    pub package: Option<Package>,
    #[serde(default)]
    pub dependencies: Vec<Dependency>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct LockEntry {
    pub group: String,
    #[serde(rename = "artifactId")]
    pub artifact_id: String,
    pub version: String,
    pub url: String,
    pub sha256: String,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct TinoxLock {
    #[serde(default)]
    pub dependencies: Vec<LockEntry>,
}

pub fn read_lock(root: &Path) -> Result<TinoxLock, String> {
    let lock_path = root.join("tinox.lock");
    if !lock_path.exists() {
        return Ok(TinoxLock::default());
    }
    let content = fs::read_to_string(&lock_path)
        .map_err(|e| format!("Cannot read tinox.lock: {}", e))?;
    serde_yaml::from_str(&content).map_err(|e| format!("Invalid tinox.lock: {}", e))
}

pub fn write_lock(root: &Path, lock: &TinoxLock) -> Result<(), String> {
    let lock_path = root.join("tinox.lock");
    let content =
        serde_yaml::to_string(lock).map_err(|e| format!("Cannot serialize tinox.lock: {}", e))?;
    fs::write(&lock_path, content).map_err(|e| format!("Cannot write tinox.lock: {}", e))
}

fn lock_entry_for<'a>(lock: &'a TinoxLock, dep: &Dependency) -> Option<&'a LockEntry> {
    lock.dependencies.iter().find(|e| {
        e.group == dep.group && e.artifact_id == dep.artifact_id && e.version == dep.version
    })
}

fn upsert_lock_entry(lock: &mut TinoxLock, entry: LockEntry) {
    lock.dependencies.retain(|e| {
        !(e.group == entry.group && e.artifact_id == entry.artifact_id && e.version == entry.version)
    });
    lock.dependencies.push(entry);
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(bytes);
    digest.iter().map(|b| format!("{:02x}", b)).collect()
}

pub fn find_project_root() -> Option<PathBuf> {
    let mut dir = std::env::current_dir().ok()?;
    loop {
        if dir.join("tinox.yaml").exists() || dir.join("tinox.toml").exists() {
            return Some(dir);
        }
        if !dir.pop() {
            break;
        }
    }
    None
}

pub fn read_manifest(root: &Path) -> Result<TinoxManifest, String> {
    let yaml_path = root.join("tinox.yaml");
    if !yaml_path.exists() {
        return Ok(TinoxManifest::default());
    }
    let content = fs::read_to_string(&yaml_path)
        .map_err(|e| format!("Cannot read tinox.yaml: {}", e))?;
    serde_yaml::from_str(&content).map_err(|e| format!("Invalid tinox.yaml: {}", e))
}

pub fn write_manifest(root: &Path, manifest: &TinoxManifest) -> Result<(), String> {
    let yaml_path = root.join("tinox.yaml");
    let content =
        serde_yaml::to_string(manifest).map_err(|e| format!("Cannot serialize manifest: {}", e))?;
    fs::write(&yaml_path, content).map_err(|e| format!("Cannot write tinox.yaml: {}", e))
}

/// Rejects anything that isn't a single, plain path segment: empty, ".", "..",
/// or containing a path separator would let a dependency's group/artifactId/version
/// escape `.tinox/deps` (e.g. via an absolute path or a `..` segment).
fn sanitize_path_component(value: &str, field: &str) -> Result<(), String> {
    if value.is_empty()
        || value == "."
        || value == ".."
        || value.contains('/')
        || value.contains('\\')
        || value.contains('\0')
    {
        return Err(format!(
            "invalid dependency {}: {:?} is not a valid path segment",
            field, value
        ));
    }
    Ok(())
}

pub fn dep_install_dir(root: &Path, dep: &Dependency) -> Result<PathBuf, String> {
    sanitize_path_component(&dep.group, "group")?;
    sanitize_path_component(&dep.artifact_id, "artifactId")?;
    sanitize_path_component(&dep.version, "version")?;
    Ok(root
        .join(".tinox")
        .join("deps")
        .join(&dep.group)
        .join(&dep.artifact_id)
        .join(&dep.version))
}

pub fn installed_dep_dirs(root: &Path, manifest: &TinoxManifest) -> Vec<PathBuf> {
    manifest
        .dependencies
        .iter()
        .filter_map(|d| dep_install_dir(root, d).ok())
        .filter(|p| p.exists())
        .collect()
}

/// Resolves the expected checksum for `dep` per the priority described on
/// `install_dep`: an explicit `dep.sha256` always wins; otherwise, unless
/// `update` is set, a `tinox.lock` entry for the same coordinates *and*
/// URL pins it (a changed URL for the same version has no comparable
/// baseline, so it's treated as an unpinned first install rather than a
/// mismatch).
fn expected_checksum<'a>(dep: &'a Dependency, lock: &'a TinoxLock, update: bool) -> Option<&'a str> {
    dep.sha256.as_deref().or_else(|| {
        if update {
            None
        } else {
            lock_entry_for(lock, dep)
                .filter(|e| e.url == dep.url)
                .map(|e| e.sha256.as_str())
        }
    })
}

fn verify_checksum(dep: &Dependency, lock: &TinoxLock, update: bool, actual_sha256: &str) -> Result<(), String> {
    if let Some(expected) = expected_checksum(dep, lock, update) {
        if !expected.eq_ignore_ascii_case(actual_sha256) {
            return Err(format!(
                "checksum mismatch for {}:{} {} ({}): expected sha256 {}, got {} — refusing to install a dependency whose content doesn't match what was pinned (tinox.yaml/tinox.lock). Pass --update to re-pin if this URL's content legitimately changed.",
                dep.group, dep.artifact_id, dep.version, dep.url, expected, actual_sha256
            ));
        }
    }
    Ok(())
}

/// Installs one dependency, verifying the downloaded bytes against an
/// expected SHA-256 when one is available (`dep.sha256` from tinox.yaml
/// takes priority; otherwise a matching `tinox.lock` entry for the same
/// group/artifactId/version/url pins it). A mismatch is a hard error —
/// no silent fallback to "install anyway" — the same "no silent garbage"
/// principle the rest of this project follows (see CLAUDE.md). Callers
/// are responsible for persisting the resulting hash back into the lock
/// (see `cmd_install`/`cmd_add`) since this function only downloads.
fn install_dep(root: &Path, dep: &Dependency, lock: &TinoxLock, update: bool) -> Result<Option<String>, String> {
    let install_dir = dep_install_dir(root, dep)?;
    if install_dir.exists() {
        println!(
            "  already installed: {}:{} {}",
            dep.group, dep.artifact_id, dep.version
        );
        return Ok(None);
    }

    println!(
        "  downloading {}:{} {} ...",
        dep.group, dep.artifact_id, dep.version
    );

    let response = ureq::get(&dep.url)
        .call()
        .map_err(|e| format!("Download failed ({}): {}", dep.url, e))?;

    let mut bytes: Vec<u8> = Vec::new();
    response
        .into_reader()
        .read_to_end(&mut bytes)
        .map_err(|e| format!("Read failed: {}", e))?;

    let actual_sha256 = sha256_hex(&bytes);
    verify_checksum(dep, lock, update, &actual_sha256)?;

    fs::create_dir_all(&install_dir)
        .map_err(|e| format!("Cannot create install dir: {}", e))?;

    let url_lower = dep.url.to_lowercase();
    if url_lower.ends_with(".tar.gz") || url_lower.ends_with(".tgz") {
        extract_tar_gz(&bytes, &install_dir)?;
    } else if url_lower.ends_with(".zip") {
        extract_zip(&bytes, &install_dir)?;
    } else {
        // Single .tnx file — save directly
        let filename = dep.url.split('/').next_back().unwrap_or("lib.tnx");
        let filename = if filename.ends_with(".tnx") {
            filename.to_string()
        } else {
            format!("{}.tnx", filename)
        };
        fs::write(install_dir.join(filename), &bytes)
            .map_err(|e| format!("Cannot write file: {}", e))?;
    }

    println!(
        "  installed: {}:{} {} (sha256 {})",
        dep.group, dep.artifact_id, dep.version, actual_sha256
    );
    Ok(Some(actual_sha256))
}

fn extract_tar_gz(bytes: &[u8], dest: &Path) -> Result<(), String> {
    use flate2::read::GzDecoder;
    use std::io::Cursor;
    use tar::Archive;

    let gz = GzDecoder::new(Cursor::new(bytes));
    let mut archive = Archive::new(gz);
    archive
        .unpack(dest)
        .map_err(|e| format!("Cannot extract tar.gz: {}", e))
}

fn extract_zip(bytes: &[u8], dest: &Path) -> Result<(), String> {
    use std::io::Cursor;

    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|e| format!("Cannot open zip: {}", e))?;
    archive
        .extract(dest)
        .map_err(|e| format!("Cannot extract zip: {}", e))
}

/// `tinox install [--update]`. Without `--update`, a dependency already
/// pinned in tinox.lock must download to the exact same sha256 or the
/// install fails (catches a dependency URL's content silently changing
/// underneath a pinned version — see #112). `--update` re-pins instead of
/// verifying, for when that change is intentional.
pub fn cmd_install(args: &[String]) {
    let update = args.iter().any(|a| a == "--update");
    let root = match find_project_root() {
        Some(r) => r,
        None => {
            eprintln!("error: no tinox.yaml found");
            return;
        }
    };
    let manifest = match read_manifest(&root) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("error: {}", e);
            return;
        }
    };
    if manifest.dependencies.is_empty() {
        println!("No dependencies to install.");
        return;
    }
    let mut lock = match read_lock(&root) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("error: {}", e);
            return;
        }
    };
    println!(
        "Installing {} dependenc{} ...",
        manifest.dependencies.len(),
        if manifest.dependencies.len() == 1 {
            "y"
        } else {
            "ies"
        }
    );
    let mut ok = 0usize;
    let mut fail = 0usize;
    let mut lock_changed = false;
    for dep in &manifest.dependencies {
        match install_dep(&root, dep, &lock, update) {
            Ok(Some(sha256)) => {
                upsert_lock_entry(
                    &mut lock,
                    LockEntry {
                        group: dep.group.clone(),
                        artifact_id: dep.artifact_id.clone(),
                        version: dep.version.clone(),
                        url: dep.url.clone(),
                        sha256,
                    },
                );
                lock_changed = true;
                ok += 1;
            }
            Ok(None) => ok += 1, // already installed, nothing new to pin
            Err(e) => {
                eprintln!("  error: {}", e);
                fail += 1;
            }
        }
    }
    if lock_changed {
        if let Err(e) = write_lock(&root, &lock) {
            eprintln!("warning: failed to update tinox.lock: {}", e);
        }
    }
    println!("{} installed, {} failed", ok, fail);
}

pub fn cmd_package() {
    let root = match find_project_root() {
        Some(r) => r,
        None => {
            eprintln!("error: no tinox.yaml found");
            return;
        }
    };
    let manifest = match read_manifest(&root) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("error: {}", e);
            return;
        }
    };

    let pkg = match &manifest.package {
        Some(p) => p.clone(),
        None => {
            eprintln!("error: tinox.yaml is missing [package] section");
            return;
        }
    };

    let src_dir = root.join("src");
    if !src_dir.exists() {
        eprintln!("error: src/ directory not found");
        return;
    }

    // Collect all .tnx files from src/
    let mut tnx_files: Vec<PathBuf> = Vec::new();
    collect_tnx_files(&src_dir, &mut tnx_files);

    if tnx_files.is_empty() {
        eprintln!("error: no .tnx source files found in src/");
        return;
    }

    let archive_name = format!("{}-{}.tar.gz", pkg.name, pkg.version);
    let archive_path = root.join(&archive_name);

    match build_tar_gz(&archive_path, &root, &tnx_files) {
        Ok(_) => println!("Packaged: {}", archive_name),
        Err(e) => eprintln!("error: {}", e),
    }
}

fn collect_tnx_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_tnx_files(&path, out);
        } else if path.extension().map(|e| e == "tnx").unwrap_or(false) {
            out.push(path);
        }
    }
}

fn build_tar_gz(archive_path: &Path, root: &Path, files: &[PathBuf]) -> Result<(), String> {
    use flate2::{write::GzEncoder, Compression};
    use tar::Builder;

    let file = fs::File::create(archive_path)
        .map_err(|e| format!("Cannot create archive: {}", e))?;
    let gz = GzEncoder::new(file, Compression::default());
    let mut builder = Builder::new(gz);

    for file_path in files {
        let rel = file_path
            .strip_prefix(root)
            .map_err(|_| format!("Path error: {}", file_path.display()))?;
        builder
            .append_path_with_name(file_path, rel)
            .map_err(|e| format!("Cannot add {}: {}", rel.display(), e))?;
    }

    builder
        .finish()
        .map_err(|e| format!("Cannot finalize archive: {}", e))?;

    Ok(())
}

pub fn cmd_add(args: &[String]) {
    if args.len() < 4 {
        eprintln!("Usage: tinox add <group> <artifactId> <version> <url>");
        return;
    }
    let dep = Dependency {
        group: args[0].clone(),
        artifact_id: args[1].clone(),
        version: args[2].clone(),
        url: args[3].clone(),
        sha256: None,
    };
    let root = match find_project_root() {
        Some(r) => r,
        None => {
            eprintln!("error: no tinox.yaml found");
            return;
        }
    };
    let mut manifest = match read_manifest(&root) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("error: {}", e);
            return;
        }
    };
    manifest
        .dependencies
        .retain(|d| !(d.group == dep.group && d.artifact_id == dep.artifact_id));
    manifest.dependencies.push(dep.clone());
    if let Err(e) = write_manifest(&root, &manifest) {
        eprintln!("error: {}", e);
        return;
    }
    println!(
        "Added {}:{} {} to tinox.yaml",
        dep.group, dep.artifact_id, dep.version
    );
    // A fresh `add` has nothing pinned yet to verify against — this is the
    // first resolution of this coordinate, so an empty lock is correct
    // here (there is by definition no prior hash to compare against).
    let lock = TinoxLock::default();
    match install_dep(&root, &dep, &lock, false) {
        Ok(Some(sha256)) => {
            let mut lock = match read_lock(&root) {
                Ok(l) => l,
                Err(e) => {
                    eprintln!("warning: failed to update tinox.lock: {}", e);
                    return;
                }
            };
            upsert_lock_entry(
                &mut lock,
                LockEntry {
                    group: dep.group.clone(),
                    artifact_id: dep.artifact_id.clone(),
                    version: dep.version.clone(),
                    url: dep.url.clone(),
                    sha256,
                },
            );
            if let Err(e) = write_lock(&root, &lock) {
                eprintln!("warning: failed to update tinox.lock: {}", e);
            }
        }
        Ok(None) => {}
        Err(e) => eprintln!("warning: install failed: {}", e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dep(group: &str, artifact_id: &str, version: &str) -> Dependency {
        Dependency {
            group: group.to_string(),
            artifact_id: artifact_id.to_string(),
            version: version.to_string(),
            url: "https://example.com/lib.tnx".to_string(),
            sha256: None,
        }
    }

    fn lock_entry(d: &Dependency, sha256: &str) -> LockEntry {
        LockEntry {
            group: d.group.clone(),
            artifact_id: d.artifact_id.clone(),
            version: d.version.clone(),
            url: d.url.clone(),
            sha256: sha256.to_string(),
        }
    }

    #[test]
    fn rejects_dotdot_traversal_in_any_field() {
        let root = Path::new("/project");
        assert!(dep_install_dir(root, &dep("../../etc", "x", "1.0")).is_err());
        assert!(dep_install_dir(root, &dep("x", "../../etc", "1.0")).is_err());
        assert!(dep_install_dir(root, &dep("x", "y", "..")).is_err());
        assert!(dep_install_dir(root, &dep("x", "y", "../../../tmp/evil")).is_err());
    }

    #[test]
    fn rejects_absolute_and_separator_segments() {
        let root = Path::new("/project");
        assert!(dep_install_dir(root, &dep("/etc", "x", "1.0")).is_err());
        assert!(dep_install_dir(root, &dep("x", "a/b", "1.0")).is_err());
        assert!(dep_install_dir(root, &dep("x", "a\\b", "1.0")).is_err());
        assert!(dep_install_dir(root, &dep("", "x", "1.0")).is_err());
        assert!(dep_install_dir(root, &dep("x", ".", "1.0")).is_err());
    }

    #[test]
    fn accepts_normal_coordinates_and_stays_under_deps() {
        let root = Path::new("/project");
        let dir = dep_install_dir(root, &dep("com.example", "mylib", "1.2.3")).unwrap();
        assert!(dir.starts_with(root.join(".tinox").join("deps")));
        assert_eq!(dir, root.join(".tinox/deps/com.example/mylib/1.2.3"));
    }

    #[test]
    fn sha256_hex_matches_known_vector() {
        // sha256("") and sha256("abc") — canonical FIPS 180-4 test vectors.
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn verify_checksum_with_no_pin_accepts_anything() {
        let d = dep("g", "a", "1.0");
        let lock = TinoxLock::default();
        assert!(verify_checksum(&d, &lock, false, "deadbeef").is_ok());
    }

    #[test]
    fn verify_checksum_explicit_sha256_takes_priority_over_lock() {
        let mut d = dep("g", "a", "1.0");
        d.sha256 = Some("AAAA".to_string()); // uppercase — comparison must be case-insensitive
        let mut lock = TinoxLock::default();
        lock.dependencies.push(lock_entry(&d, "bbbb")); // would mismatch if consulted
        assert!(verify_checksum(&d, &lock, false, "aaaa").is_ok());
        assert!(verify_checksum(&d, &lock, false, "cccc").is_err());
    }

    #[test]
    fn verify_checksum_falls_back_to_lock_entry() {
        let d = dep("g", "a", "1.0");
        let mut lock = TinoxLock::default();
        lock.dependencies.push(lock_entry(&d, "cafebabe"));
        assert!(verify_checksum(&d, &lock, false, "cafebabe").is_ok());
        let err = verify_checksum(&d, &lock, false, "00000000").unwrap_err();
        assert!(err.contains("checksum mismatch"), "unexpected message: {err}");
        assert!(err.contains("--update"), "should mention the escape hatch: {err}");
    }

    #[test]
    fn verify_checksum_lock_entry_for_different_url_is_not_a_pin() {
        let mut d = dep("g", "a", "1.0");
        let mut lock = TinoxLock::default();
        lock.dependencies.push(lock_entry(&d, "cafebabe")); // pinned for the original URL
        d.url = "https://example.com/moved.tnx".to_string(); // same coordinates, different source
        // No comparable baseline for the new URL — anything is accepted rather than
        // spuriously failing against a hash that describes a different download.
        assert!(verify_checksum(&d, &lock, false, "anything").is_ok());
    }

    #[test]
    fn verify_checksum_update_flag_bypasses_lock_pin() {
        let d = dep("g", "a", "1.0");
        let mut lock = TinoxLock::default();
        lock.dependencies.push(lock_entry(&d, "cafebabe"));
        // Without --update this would fail; with it, re-pinning is allowed.
        assert!(verify_checksum(&d, &lock, true, "brand-new-hash").is_ok());
    }

    #[test]
    fn lock_roundtrips_through_yaml() {
        let dir = std::env::temp_dir().join(format!(
            "tinox-pm-test-{}-{}",
            std::process::id(),
            "lock_roundtrips_through_yaml"
        ));
        fs::create_dir_all(&dir).unwrap();
        let d = dep("g", "a", "1.0");
        let mut lock = TinoxLock::default();
        upsert_lock_entry(&mut lock, lock_entry(&d, "aaaa"));
        write_lock(&dir, &lock).unwrap();

        let read_back = read_lock(&dir).unwrap();
        assert_eq!(read_back.dependencies.len(), 1);
        assert_eq!(read_back.dependencies[0].sha256, "aaaa");

        // upsert replaces rather than duplicates an entry for the same coordinates
        upsert_lock_entry(&mut lock, lock_entry(&d, "bbbb"));
        assert_eq!(lock.dependencies.len(), 1);
        assert_eq!(lock.dependencies[0].sha256, "bbbb");

        fs::remove_dir_all(&dir).ok();
    }
}
