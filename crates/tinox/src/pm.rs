use std::collections::HashSet;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// `[package]`'s `name`/`version`/`description` — parsed/written by
/// `parse_manifest`/`write_manifest` (hand-rolled, see there for why).
#[derive(Debug, Clone)]
pub struct Package {
    pub name: String,
    pub version: String,
    pub description: String,
}

/// One `[[dependencies]]` table — parsed/written by
/// `parse_manifest`/`write_manifest` (hand-rolled, see there for why).
#[derive(Debug, Clone)]
pub struct Dependency {
    pub group: String,
    pub artifact_id: String,
    pub version: String,
    pub url: String,
    /// Expected SHA-256 of the downloaded artifact, lowercase hex. Optional
    /// for backward compatibility with existing manifests, but strongly
    /// recommended: without it, `tinox install` only pins against whatever
    /// tinox.lock happens to have recorded (see verify_checksum).
    pub sha256: Option<String>,
}

#[derive(Debug, Default)]
pub struct TinoxManifest {
    pub package: Option<Package>,
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
        if dir.join("tinox.toml").exists() {
            return Some(dir);
        }
        if !dir.pop() {
            break;
        }
    }
    None
}

#[derive(PartialEq, Clone, Copy)]
enum ManifestSection {
    None,
    Package,
    Dependency,
    Other,
}

fn manifest_section_for(header_line: &str) -> ManifestSection {
    if header_line == "[[dependencies]]" {
        ManifestSection::Dependency
    } else if header_line == "[package]" {
        ManifestSection::Package
    } else {
        ManifestSection::Other
    }
}

/// `key = "value"` (or bare `key = value`) → `(key, unquoted value)`, the
/// same convention every other tinox.toml reader in this codebase already
/// uses (see `read_project_entry`/`read_metrics_section` in `main.rs`).
fn parse_toml_kv(line: &str) -> Option<(&str, String)> {
    let (key, value) = line.split_once('=')?;
    Some((key.trim(), value.trim().trim_matches('"').to_string()))
}

/// Parses `[package]` (name/version/description) and every `[[dependencies]]`
/// table from `tinox.toml`'s content — hand-rolled rather than a TOML crate
/// dependency, matching every other tinox.toml reader in this codebase.
/// Unknown keys (`[package] entry`/`output`, `[build]`, `[metrics]`, …) are
/// simply skipped here, not lost — `write_manifest` below only ever
/// rewrites the keys THIS function understands, leaving the rest of the
/// file untouched (see #154 — a prior version of this used a completely
/// separate `tinox.yaml` file/format that the rest of the CLI never read).
fn parse_manifest(content: &str) -> TinoxManifest {
    let mut section = ManifestSection::None;
    let mut pkg_name = String::new();
    let mut pkg_version = String::new();
    let mut pkg_description = String::new();
    let mut have_package = false;

    let mut dependencies: Vec<Dependency> = Vec::new();
    let mut cur = Dependency { group: String::new(), artifact_id: String::new(), version: String::new(), url: String::new(), sha256: None };
    let mut have_cur = false;

    for raw_line in content.lines() {
        let line = raw_line.trim();
        if line.starts_with('[') {
            if have_cur {
                dependencies.push(std::mem::replace(&mut cur, Dependency { group: String::new(), artifact_id: String::new(), version: String::new(), url: String::new(), sha256: None }));
                have_cur = false;
            }
            section = manifest_section_for(line);
            if section == ManifestSection::Dependency {
                have_cur = true;
            } else if section == ManifestSection::Package {
                have_package = true;
            }
            continue;
        }
        let Some((key, value)) = parse_toml_kv(line) else { continue };
        match section {
            ManifestSection::Package => match key {
                "name" => pkg_name = value,
                "version" => pkg_version = value,
                "description" => pkg_description = value,
                _ => {}
            },
            ManifestSection::Dependency => match key {
                "group" => cur.group = value,
                "artifactId" => cur.artifact_id = value,
                "version" => cur.version = value,
                "url" => cur.url = value,
                "sha256" if !value.is_empty() => cur.sha256 = Some(value),
                _ => {}
            },
            ManifestSection::None | ManifestSection::Other => {}
        }
    }
    if have_cur {
        dependencies.push(cur);
    }

    let package = have_package.then_some(Package { name: pkg_name, version: pkg_version, description: pkg_description });
    TinoxManifest { package, dependencies }
}

pub fn read_manifest(root: &Path) -> Result<TinoxManifest, String> {
    let toml_path = root.join("tinox.toml");
    if !toml_path.exists() {
        return Ok(TinoxManifest::default());
    }
    let content = fs::read_to_string(&toml_path)
        .map_err(|e| format!("Cannot read tinox.toml: {}", e))?;
    Ok(parse_manifest(&content))
}

fn format_dependency(dep: &Dependency) -> String {
    let mut s = format!(
        "[[dependencies]]\ngroup = \"{}\"\nartifactId = \"{}\"\nversion = \"{}\"\nurl = \"{}\"\n",
        dep.group, dep.artifact_id, dep.version, dep.url
    );
    if let Some(sha256) = &dep.sha256 {
        s.push_str(&format!("sha256 = \"{}\"\n", sha256));
    }
    s
}

/// Surgically rewrites `tinox.toml`'s `name`/`version`/`description` keys
/// (inside `[package]`) and every `[[dependencies]]` table, leaving every
/// OTHER line untouched — `entry`/`output` inside `[package]`, `[build]`,
/// `[metrics]`, `[database]`, comments, … all round-trip byte-for-byte.
/// A blind whole-file rewrite from just the `TinoxManifest` struct (which
/// doesn't model those other keys/sections at all) would silently drop
/// them — exactly the failure mode #154 was filed over, just moved one
/// layer deeper if done carelessly.
pub fn write_manifest(root: &Path, manifest: &TinoxManifest) -> Result<(), String> {
    let toml_path = root.join("tinox.toml");
    let existing = if toml_path.exists() {
        fs::read_to_string(&toml_path).map_err(|e| format!("Cannot read tinox.toml: {}", e))?
    } else {
        String::new()
    };

    let mut out: Vec<String> = Vec::new();
    let mut section = ManifestSection::None;
    let mut saw_package_header = false;

    for raw_line in existing.lines() {
        let line = raw_line.trim();
        if line.starts_with('[') {
            section = manifest_section_for(line);
            if section == ManifestSection::Dependency {
                continue; // dropped — every [[dependencies]] table is rebuilt below
            }
            out.push(raw_line.to_string());
            if section == ManifestSection::Package {
                saw_package_header = true;
                // Fresh name/version/description right after the header;
                // any OLD copies of these three keys further down this
                // section are skipped below, everything else (entry,
                // output, …) round-trips untouched.
                if let Some(pkg) = &manifest.package {
                    out.push(format!("name = \"{}\"", pkg.name));
                    out.push(format!("version = \"{}\"", pkg.version));
                    out.push(format!("description = \"{}\"", pkg.description));
                }
            }
            continue;
        }
        match section {
            ManifestSection::Dependency => {} // dropped — rebuilt below
            ManifestSection::Package => {
                let key = parse_toml_kv(line).map(|(k, _)| k);
                if !matches!(key, Some("name" | "version" | "description")) {
                    out.push(raw_line.to_string());
                }
            }
            ManifestSection::None | ManifestSection::Other => out.push(raw_line.to_string()),
        }
    }

    let mut content = out.join("\n");
    if !content.is_empty() && !content.ends_with('\n') {
        content.push('\n');
    }
    if !saw_package_header {
        if let Some(pkg) = &manifest.package {
            content.push_str(&format!(
                "[package]\nname = \"{}\"\nversion = \"{}\"\ndescription = \"{}\"\n",
                pkg.name, pkg.version, pkg.description
            ));
        }
    }
    if !manifest.dependencies.is_empty() {
        content.push('\n');
        for dep in &manifest.dependencies {
            content.push_str(&format_dependency(dep));
            content.push('\n');
        }
        // Drop the one trailing blank line left after the last dependency block.
        content.pop();
    }

    fs::write(&toml_path, content).map_err(|e| format!("Cannot write tinox.toml: {}", e))
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
                "checksum mismatch for {}:{} {} ({}): expected sha256 {}, got {} — refusing to install a dependency whose content doesn't match what was pinned (tinox.toml/tinox.lock). Pass --update to re-pin if this URL's content legitimately changed.",
                dep.group, dep.artifact_id, dep.version, dep.url, expected, actual_sha256
            ));
        }
    }
    Ok(())
}

/// Installs one dependency, verifying the downloaded bytes against an
/// expected SHA-256 when one is available (`dep.sha256` from tinox.toml
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

    let mut raw_bytes: Vec<u8> = Vec::new();
    response
        .into_reader()
        .read_to_end(&mut raw_bytes)
        .map_err(|e| format!("Read failed: {}", e))?;

    // A registry API (e.g. tinox-central) can't return artifact bytes as a
    // raw response body — tinox's `String` is NUL-terminated at the
    // runtime level (see tinox-central's PLAN.md §7.1), so any server
    // built with tinox.core.http_server has to wrap the artifact in a
    // `{"filename": "...", "contentBase64": "..."}` JSON envelope instead
    // of streaming octets directly. Detect and unwrap that shape before
    // falling back to "response body IS the artifact", so a dependency
    // URL pointing at such a registry (whose path rarely carries a
    // .tar.gz/.zip suffix) still resolves to the right filename/bytes.
    let (bytes, filename_hint): (Vec<u8>, Option<String>) =
        match parse_registry_envelope(&raw_bytes) {
            Some((filename, decoded)) => (decoded, Some(filename)),
            None => (raw_bytes, None),
        };

    let actual_sha256 = sha256_hex(&bytes);
    verify_checksum(dep, lock, update, &actual_sha256)?;

    fs::create_dir_all(&install_dir)
        .map_err(|e| format!("Cannot create install dir: {}", e))?;

    // Prefer the artifact's own filename (from a registry envelope) for
    // the tar.gz/zip/single-file dispatch — the dependency URL itself
    // (e.g. a registry API path with no file extension at all) isn't a
    // reliable signal in that case.
    let name_for_dispatch = filename_hint
        .clone()
        .unwrap_or_else(|| dep.url.split('/').next_back().unwrap_or("lib.tnx").to_string());
    let name_lower = name_for_dispatch.to_lowercase();
    if name_lower.ends_with(".tar.gz") || name_lower.ends_with(".tgz") {
        extract_tar_gz(&bytes, &install_dir)?;
    } else if name_lower.ends_with(".zip") {
        extract_zip(&bytes, &install_dir)?;
    } else {
        // Single .tnx file — save directly
        let filename = if name_for_dispatch.ends_with(".tnx") {
            name_for_dispatch
        } else {
            format!("{}.tnx", name_for_dispatch)
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

/// Recognizes a tinox-central-shaped download response — a JSON object
/// with (at least) `filename` and `contentBase64` string fields — and
/// returns the decoded artifact bytes plus its reported filename. `None`
/// for anything else (a plain tar.gz/zip/.tnx response body, which is
/// the common case for a dependency hosted as a static file), so callers
/// fall back to treating the raw bytes as the artifact itself.
///
/// Hand-rolled rather than pulling in a JSON crate, matching this file's
/// existing hand-rolled TOML manifest parser — the shape needed here is
/// two fixed string fields, not general JSON.
fn parse_registry_envelope(bytes: &[u8]) -> Option<(String, Vec<u8>)> {
    let text = std::str::from_utf8(bytes).ok()?;
    let trimmed = text.trim_start();
    if !trimmed.starts_with('{') {
        return None;
    }
    let filename = extract_json_string_field(trimmed, "filename")?;
    let content_base64 = extract_json_string_field(trimmed, "contentBase64")?;
    let decoded = base64_decode(&content_base64)?;
    Some((filename, decoded))
}

/// Finds `"field": "value"` in a (trusted, server-controlled) JSON object
/// string and returns `value`, unescaping `\"` and `\\` only (the only
/// escapes tinox-central's `Json::serialize` needs for a filename/base64
/// payload — both are otherwise plain-ASCII fields).
fn extract_json_string_field(json: &str, field: &str) -> Option<String> {
    let needle = format!("\"{}\"", field);
    let key_pos = json.find(&needle)?;
    let after_key = &json[key_pos + needle.len()..];
    let colon_pos = after_key.find(':')?;
    let after_colon = after_key[colon_pos + 1..].trim_start();
    let mut chars = after_colon.char_indices();
    let (_, first) = chars.next()?;
    if first != '"' {
        return None;
    }
    let mut result = String::new();
    let mut escaped = false;
    for (i, c) in chars {
        if escaped {
            match c {
                '"' => result.push('"'),
                '\\' => result.push('\\'),
                other => result.push(other),
            }
            escaped = false;
            continue;
        }
        if c == '\\' {
            escaped = true;
            continue;
        }
        if c == '"' {
            let _ = i;
            return Some(result);
        }
        result.push(c);
    }
    None
}

/// Standard base64 (RFC 4648, with `=` padding) decoder — the alphabet
/// `tinox.core.base64`'s `Base64::encodeBytes` uses on the server side.
/// Hand-rolled for the same reason as `parse_registry_envelope`: one
/// fixed, well-known format, not worth a new crate dependency for.
fn base64_decode(input: &str) -> Option<Vec<u8>> {
    fn val(c: u8) -> Option<u8> {
        match c {
            b'A'..=b'Z' => Some(c - b'A'),
            b'a'..=b'z' => Some(c - b'a' + 26),
            b'0'..=b'9' => Some(c - b'0' + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }
    let clean: Vec<u8> = input.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
    let mut out = Vec::with_capacity(clean.len() / 4 * 3);
    let mut i = 0;
    while i < clean.len() {
        let b0 = *clean.get(i)?;
        let c0 = val(b0)?;
        let b1 = *clean.get(i + 1)?;
        let c1 = val(b1)?;
        let b2 = clean.get(i + 2).copied();
        let b3 = clean.get(i + 3).copied();

        out.push((c0 << 2) | (c1 >> 4));

        match (b2, b3) {
            (Some(b'='), _) | (None, _) => break,
            (Some(b2v), Some(b'=')) | (Some(b2v), None) => {
                let c2 = val(b2v)?;
                out.push((c1 << 4) | (c2 >> 2));
                break;
            }
            (Some(b2v), Some(b3v)) => {
                let c2 = val(b2v)?;
                out.push((c1 << 4) | (c2 >> 2));
                let c3 = val(b3v)?;
                out.push((c2 << 6) | c3);
            }
        }
        i += 4;
    }
    Some(out)
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

/// Installs `dep`, then — if its own install directory contains a
/// `tinox.toml` declaring further dependencies — recursively installs
/// those too (#157). Flat resolution: transitive dependencies land in
/// the SAME project-level `.tinox/deps/` tree as direct ones, not nested
/// per-dependency, matching the flat namespace `resolve_imports`/
/// `resolve_in_dep_dirs` already assumes. `visited` is shared across the
/// whole call tree for one `install`/`add` run: it guards against a
/// dependency cycle (A depends on B depends on A) and against re-walking
/// a coordinate reached twice (a diamond — two dependencies both
/// depending on the same third one at the same version).
///
/// Diamond dependencies at DIFFERENT versions of the same
/// group:artifactId are deliberately not specially handled here — they
/// install into two different version-suffixed directories without
/// conflict, and if that ever results in an import genuinely resolving
/// against both, #156's ambiguous-import hard error already catches it;
/// no separate version-conflict detection is needed on top of that.
///
/// Returns `(installed_ok, failed)`, aggregated over `dep` and everything
/// transitively reached from it.
fn install_dep_transitively(
    root: &Path,
    dep: &Dependency,
    lock: &mut TinoxLock,
    update: bool,
    visited: &mut HashSet<(String, String, String)>,
    lock_changed: &mut bool,
) -> (usize, usize) {
    let coord = (dep.group.clone(), dep.artifact_id.clone(), dep.version.clone());
    if !visited.insert(coord) {
        return (0, 0);
    }

    let mut ok = 0usize;
    let mut fail = 0usize;
    match install_dep(root, dep, lock, update) {
        Ok(Some(sha256)) => {
            upsert_lock_entry(
                lock,
                LockEntry {
                    group: dep.group.clone(),
                    artifact_id: dep.artifact_id.clone(),
                    version: dep.version.clone(),
                    url: dep.url.clone(),
                    sha256,
                },
            );
            *lock_changed = true;
            ok += 1;
        }
        Ok(None) => ok += 1, // already installed, nothing new to pin
        Err(e) => {
            eprintln!("  error: {}", e);
            // A dependency we couldn't install has no readable manifest of
            // its own to walk — nothing transitive to attempt.
            return (ok, fail + 1);
        }
    }

    if let Ok(install_dir) = dep_install_dir(root, dep) {
        // read_manifest returns an empty manifest (not an error) when the
        // dependency doesn't ship its own tinox.toml — the common case,
        // handled the same as "no transitive dependencies" below.
        if let Ok(sub_manifest) = read_manifest(&install_dir) {
            for sub_dep in &sub_manifest.dependencies {
                let (sub_ok, sub_fail) =
                    install_dep_transitively(root, sub_dep, lock, update, visited, lock_changed);
                ok += sub_ok;
                fail += sub_fail;
            }
        }
    }

    (ok, fail)
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
            eprintln!("error: no tinox.toml found");
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
    let mut visited: HashSet<(String, String, String)> = HashSet::new();
    for dep in &manifest.dependencies {
        let (dep_ok, dep_fail) =
            install_dep_transitively(&root, dep, &mut lock, update, &mut visited, &mut lock_changed);
        ok += dep_ok;
        fail += dep_fail;
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
            eprintln!("error: no tinox.toml found");
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
            eprintln!("error: tinox.toml is missing [package] section");
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

    // Archive entries are relative to src/, not to the project root: a
    // consumer's `tinox install` extracts this archive directly into
    // .tinox/deps/<group>/<artifactId>/<version>/, and import resolution
    // (resolve_in_dep_dirs in main.rs) looks for the imported module path
    // right under THAT directory — a leading "src/" in the archive would
    // put every file one level too deep and break every import of this
    // package (confirmed by hand: a consumer importing `foo.Bar` expects
    // <dep-dir>/foo/Bar.tnx, not <dep-dir>/src/foo/Bar.tnx).
    match build_tar_gz(&archive_path, &src_dir, &tnx_files) {
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
            eprintln!("error: no tinox.toml found");
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
        "Added {}:{} {} to tinox.toml",
        dep.group, dep.artifact_id, dep.version
    );
    // The real on-disk lock, not a fresh empty one: `dep` itself has
    // nothing pinned yet either way (a brand-new coordinate can't be in
    // it), but any TRANSITIVE dependency reached from it (#157) might
    // already be pinned from an earlier `install`/`add`, and should still
    // be checksum-verified against that, not treated as unpinned.
    let mut lock = match read_lock(&root) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("warning: failed to read tinox.lock: {}", e);
            TinoxLock::default()
        }
    };
    let mut lock_changed = false;
    let mut visited: HashSet<(String, String, String)> = HashSet::new();
    install_dep_transitively(&root, &dep, &mut lock, false, &mut visited, &mut lock_changed);
    if lock_changed {
        if let Err(e) = write_lock(&root, &lock) {
            eprintln!("warning: failed to update tinox.lock: {}", e);
        }
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

    #[test]
    fn parse_manifest_reads_package_and_dependencies() {
        let content = "[package]\nname = \"demo\"\nversion = \"0.1.0\"\ndescription = \"x\"\nentry = \"src/main.tnx\"\n\n[[dependencies]]\ngroup = \"com.example\"\nartifactId = \"mylib\"\nversion = \"1.0.0\"\nurl = \"https://example.com/mylib.tar.gz\"\nsha256 = \"abc123\"\n";
        let m = parse_manifest(content);
        let pkg = m.package.expect("package");
        assert_eq!(pkg.name, "demo");
        assert_eq!(pkg.version, "0.1.0");
        assert_eq!(pkg.description, "x");
        assert_eq!(m.dependencies.len(), 1);
        assert_eq!(m.dependencies[0].group, "com.example");
        assert_eq!(m.dependencies[0].artifact_id, "mylib");
        assert_eq!(m.dependencies[0].sha256.as_deref(), Some("abc123"));
    }

    #[test]
    fn parse_manifest_missing_file_content_has_no_package() {
        let m = parse_manifest("");
        assert!(m.package.is_none());
        assert!(m.dependencies.is_empty());
    }

    #[test]
    fn write_manifest_preserves_unrelated_toml_sections_and_keys() {
        let dir = std::env::temp_dir().join(format!(
            "tinox-pm-test-{}-{}",
            std::process::id(),
            "write_manifest_preserves"
        ));
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("tinox.toml"),
            "[package]\nname = \"demo\"\nversion = \"0.1.0\"\ndescription = \"\"\nentry = \"src/main.tnx\"\n\n[build]\noutput = \"demo_bin\"\n\n[metrics]\nenabled = true\n",
        )
        .unwrap();

        let mut manifest = read_manifest(&dir).unwrap();
        manifest.dependencies.push(dep("com.example", "mylib", "1.0.0"));
        write_manifest(&dir, &manifest).unwrap();

        let rewritten = fs::read_to_string(dir.join("tinox.toml")).unwrap();
        assert!(rewritten.contains("entry = \"src/main.tnx\""), "{rewritten}");
        assert!(rewritten.contains("[build]"), "{rewritten}");
        assert!(rewritten.contains("output = \"demo_bin\""), "{rewritten}");
        assert!(rewritten.contains("[metrics]"), "{rewritten}");
        assert!(rewritten.contains("enabled = true"), "{rewritten}");
        assert!(rewritten.contains("[[dependencies]]"), "{rewritten}");
        assert!(rewritten.contains("artifactId = \"mylib\""), "{rewritten}");

        // Round-trips cleanly through read_manifest again, and the
        // preserved [package] section still parses correctly.
        let reread = read_manifest(&dir).unwrap();
        assert_eq!(reread.package.unwrap().name, "demo");
        assert_eq!(reread.dependencies.len(), 1);

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn write_manifest_dedup_replaces_existing_coordinate() {
        let dir = std::env::temp_dir().join(format!(
            "tinox-pm-test-{}-{}",
            std::process::id(),
            "write_manifest_dedup"
        ));
        fs::create_dir_all(&dir).unwrap();
        let mut manifest = TinoxManifest {
            package: Some(Package { name: "demo".to_string(), version: "0.1.0".to_string(), description: String::new() }),
            dependencies: vec![dep("com.example", "mylib", "1.0.0")],
        };
        write_manifest(&dir, &manifest).unwrap();

        manifest.dependencies.retain(|d| d.artifact_id != "mylib");
        manifest.dependencies.push(dep("com.example", "mylib", "2.0.0"));
        write_manifest(&dir, &manifest).unwrap();

        let reread = read_manifest(&dir).unwrap();
        assert_eq!(reread.dependencies.len(), 1);
        assert_eq!(reread.dependencies[0].version, "2.0.0");

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn base64_decode_matches_known_vectors() {
        // RFC 4648 test vectors.
        assert_eq!(base64_decode("").unwrap(), b"".to_vec());
        assert_eq!(base64_decode("Zg==").unwrap(), b"f".to_vec());
        assert_eq!(base64_decode("Zm8=").unwrap(), b"fo".to_vec());
        assert_eq!(base64_decode("Zm9v").unwrap(), b"foo".to_vec());
        assert_eq!(base64_decode("Zm9vYg==").unwrap(), b"foob".to_vec());
        assert_eq!(base64_decode("Zm9vYmE=").unwrap(), b"fooba".to_vec());
        assert_eq!(base64_decode("Zm9vYmFy").unwrap(), b"foobar".to_vec());
    }

    #[test]
    fn base64_decode_roundtrips_binary_with_embedded_nul() {
        // The whole point of this codepath: bytes a plain tinox `String`
        // can't represent (embedded 0x00) must still decode correctly.
        let raw: Vec<u8> = vec![0x00, 0x01, 0xff, 0x00, b'A', 0xfe];
        // Precomputed standard base64 of the bytes above.
        let encoded = "AAH/AEH+";
        assert_eq!(base64_decode(encoded).unwrap(), raw);
    }

    #[test]
    fn parse_registry_envelope_extracts_filename_and_decodes_content() {
        let json = br#"{"filename":"websocket-1.0.0.tar.gz","sha256":"abc","sizeBytes":3,"contentBase64":"Zm9v"}"#;
        let (filename, decoded) = parse_registry_envelope(json).expect("envelope should parse");
        assert_eq!(filename, "websocket-1.0.0.tar.gz");
        assert_eq!(decoded, b"foo".to_vec());
    }

    #[test]
    fn parse_registry_envelope_handles_escaped_filename() {
        let json = br#"{"filename":"weird\"name\\.tnx","contentBase64":"Zg=="}"#;
        let (filename, decoded) = parse_registry_envelope(json).expect("envelope should parse");
        assert_eq!(filename, "weird\"name\\.tnx");
        assert_eq!(decoded, b"f".to_vec());
    }

    #[test]
    fn build_tar_gz_entries_have_no_src_prefix() {
        // #172 follow-on: `tinox package` archives must extract directly
        // into a dependency install dir with the module path at the top
        // level (matching resolve_in_dep_dirs' expectations), not nested
        // under an extra "src/" the consumer never asked for.
        let dir = std::env::temp_dir().join(format!(
            "tinox-pm-test-{}-{}",
            std::process::id(),
            "build_tar_gz_no_src_prefix"
        ));
        let src_dir = dir.join("src");
        fs::create_dir_all(src_dir.join("foo")).unwrap();
        fs::write(src_dir.join("foo").join("Bar.tnx"), "class Bar {}").unwrap();

        let files = vec![src_dir.join("foo").join("Bar.tnx")];
        let archive_path = dir.join("out.tar.gz");
        build_tar_gz(&archive_path, &src_dir, &files).unwrap();

        let extract_dir = dir.join("extracted");
        let bytes = fs::read(&archive_path).unwrap();
        extract_tar_gz(&bytes, &extract_dir).unwrap();

        assert!(
            extract_dir.join("foo").join("Bar.tnx").exists(),
            "expected foo/Bar.tnx directly under the extract dir, found: {:?}",
            fs::read_dir(&extract_dir).ok().map(|e| e.filter_map(|x| x.ok().map(|x| x.path())).collect::<Vec<_>>())
        );
        assert!(!extract_dir.join("src").exists(), "archive must not carry a leading src/ path segment");

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn parse_registry_envelope_returns_none_for_non_json_bytes() {
        // A real tar.gz/zip response body never starts with '{' as valid
        // UTF-8 text -- must fall through untouched, not be misdetected.
        let gzip_magic: &[u8] = &[0x1f, 0x8b, 0x08, 0x00, 0x00, 0x00];
        assert!(parse_registry_envelope(gzip_magic).is_none());
        assert!(parse_registry_envelope(b"not json at all").is_none());
    }
}
