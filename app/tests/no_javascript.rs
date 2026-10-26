//! The app is Rust only: no JavaScript of its own in the source (Dioxus's
//! internal JavaScript is not ours) and no .js files in the repository.

use std::path::{Path, PathBuf};

fn files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy();
        if name.starts_with('.') {
            continue;
        }
        if path.is_dir() {
            files(&path, out);
        } else {
            out.push(path);
        }
    }
}

#[test]
fn the_app_source_has_no_javascript() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut all = Vec::new();
    files(&src, &mut all);
    assert!(!all.is_empty());
    let banned = [
        concat!("eval", "("),
        concat!("<scr", "ipt"),
        concat!(".js", "\""),
    ];
    for file in all {
        let text = std::fs::read_to_string(&file).unwrap();
        for b in banned {
            assert!(!text.contains(b), "{} contains {b}", file.display());
        }
    }
}

#[test]
fn the_repository_has_no_js_files() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let listed = std::process::Command::new("git")
        .args([
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
        ])
        .current_dir(root)
        .output()
        .expect("git lists the repository's files");
    assert!(listed.status.success());
    let names = String::from_utf8(listed.stdout).unwrap();
    let js: Vec<_> = names
        .split('\0')
        .filter(|n| n.ends_with(".js") || n.ends_with(".mjs"))
        .collect();
    assert!(js.is_empty(), "JavaScript files: {js:?}");
}
