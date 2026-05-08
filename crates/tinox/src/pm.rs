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
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct TinoxManifest {
    pub package: Option<Package>,
    #[serde(default)]
    pub dependencies: Vec<Dependency>,
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

pub fn dep_install_dir(root: &Path, dep: &Dependency) -> PathBuf {
    root.join(".tinox")
        .join("deps")
        .join(&dep.group)
        .join(&dep.artifact_id)
        .join(&dep.version)
}

pub fn installed_dep_dirs(root: &Path, manifest: &TinoxManifest) -> Vec<PathBuf> {
    manifest
        .dependencies
        .iter()
        .map(|d| dep_install_dir(root, d))
        .filter(|p| p.exists())
        .collect()
}

fn install_dep(root: &Path, dep: &Dependency) -> Result<(), String> {
    let install_dir = dep_install_dir(root, dep);
    if install_dir.exists() {
        println!(
            "  already installed: {}:{} {}",
            dep.group, dep.artifact_id, dep.version
        );
        return Ok(());
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
        "  installed: {}:{} {}",
        dep.group, dep.artifact_id, dep.version
    );
    Ok(())
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

pub fn cmd_install() {
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
    for dep in &manifest.dependencies {
        match install_dep(&root, dep) {
            Ok(_) => ok += 1,
            Err(e) => {
                eprintln!("  error: {}", e);
                fail += 1;
            }
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
    match install_dep(&root, &dep) {
        Ok(_) => {}
        Err(e) => eprintln!("warning: install failed: {}", e),
    }
}
