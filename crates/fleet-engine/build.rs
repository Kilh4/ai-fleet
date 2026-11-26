//! Embeds curated skills from `../../skills/elite` into `fleet-engine` so
//! `ai-fleet.exe` carries all elite skills as a standalone binary in any repo.

use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

fn collect_skill_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    let mut paths: Vec<PathBuf> = entries.filter_map(|e| e.ok().map(|e| e.path())).collect();
    paths.sort();
    for p in paths {
        if p.is_dir() {
            collect_skill_files(&p, out);
        } else if p.file_name().and_then(|n| n.to_str()) == Some("SKILL.md") {
            out.push(p);
        }
    }
}

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let elite_dir = manifest_dir.join("..").join("..").join("skills").join("elite");
    println!("cargo:rerun-if-changed={}", elite_dir.display());

    let mut files = Vec::new();
    if elite_dir.is_dir() {
        collect_skill_files(&elite_dir, &mut files);
    }

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));
    let dest = out_dir.join("embedded_skills.rs");
    let mut f = fs::File::create(&dest).expect("create embedded_skills.rs");

    writeln!(f, "pub static EMBEDDED_SKILL_RAW: &[(&str, &str)] = &[").unwrap();
    for path in &files {
        println!("cargo:rerun-if-changed={}", path.display());
        let rel = path
            .strip_prefix(&elite_dir)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        let abs = fs::canonicalize(path).unwrap_or_else(|_| path.clone());
        let abs_str = abs.to_string_lossy().replace('\\', "/");
        // Strip Windows verbatim prefix `//?/` if present so `include_str!` stays portable.
        let clean_abs = abs_str.strip_prefix("//?/").unwrap_or(&abs_str);
        writeln!(f, "    ({rel:?}, include_str!({clean_abs:?})),").unwrap();
    }
    writeln!(f, "];").unwrap();
}
