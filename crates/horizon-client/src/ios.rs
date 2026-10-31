//! iOS: apps start with no command line and no writable working directory.
//! The game's folder is the app's Documents, which the Files app shows
//! (Info.plist's UIFileSharingEnabled): saves, options.txt, logs and the
//! prepared resources, which the player copies there from a computer
//! (the preparer runs Mojang's Java code, which iOS can't).
use std::path::{Path, PathBuf};

/// The game's arguments: the resources found in Documents, Documents as the
/// game folder and home, fullscreen, and the launcher's session when one was
/// copied there.
pub fn launch_args() -> Vec<String> {
    let docs = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default()
        .join("Documents");
    let _ = std::fs::create_dir_all(&docs);
    let mut args = vec![
        "--game-dir".to_owned(),
        docs.display().to_string(),
        "--home".to_owned(),
        docs.display().to_string(),
        "--fullscreen".to_owned(),
    ];
    // The resources put in Documents, else those built into the app (the owner's IPA,
    // ios.yml), copied to Documents so that later IPAs without them find them still.
    // Without either the game says where to put them (no_resources.rs).
    let resources = find_resources(&docs, 4)
        .or_else(|| bundled_resources().map(|b| keep(&b, &docs).unwrap_or(b)))
        .unwrap_or_else(|| docs.join("resources"));
    args.extend(["--resources".to_owned(), resources.display().to_string()]);
    // A session the launcher wrote (online-mode servers); the game reads it once and deletes it.
    let session = docs.join("session.json");
    if session.is_file() {
        args.extend(["--session-file".to_owned(), session.display().to_string()]);
    }
    // More options for a test run in the Simulator (ios/sim-test.sh passes them through simctl).
    if let Ok(extra) = std::env::var("HORIZON_IOS_ARGS") {
        args.extend(extra.split_whitespace().map(str::to_owned));
    }
    args
}

/// The prepared resources inside the app bundle, next to the executable.
fn bundled_resources() -> Option<PathBuf> {
    let dir = std::env::current_exe().ok()?.parent()?.join("resources");
    dir.join("horizon-resources.json").is_file().then_some(dir)
}

/// Copies the bundled resources `from` to `docs/resources`, whole or not at all.
/// On APFS each file is a clone of the bundle's, so this takes no time or space.
fn keep(from: &Path, docs: &Path) -> std::io::Result<PathBuf> {
    let partial = docs.join("resources.partial");
    let _ = std::fs::remove_dir_all(&partial);
    copy_dir(from, &partial)?;
    let to = docs.join("resources");
    let _ = std::fs::remove_dir_all(&to);
    std::fs::rename(&partial, &to)?;
    Ok(to)
}

fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

/// The first folder under `dir`, `depth` levels down at most, holding a
/// `horizon-resources.json`: wherever the Files app unzipped it.
fn find_resources(dir: &Path, depth: u32) -> Option<PathBuf> {
    if dir.join("horizon-resources.json").is_file() {
        return Some(dir.to_path_buf());
    }
    if depth == 0 {
        return None;
    }
    let mut subdirs: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .map(|e| e.path())
        // The game's own folders never hold it.
        .filter(|p| {
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
            !matches!(
                name,
                "saves" | "logs" | "crash-reports" | "screenshots" | "resourcepacks" | "__MACOSX"
            )
        })
        .collect();
    subdirs.sort();
    subdirs.iter().find_map(|d| find_resources(d, depth - 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_an_unzipped_folder() {
        let root = std::env::temp_dir().join(format!("horizon-ios-{}", std::process::id()));
        let deep = root.join("horizon-resources/26.3");
        std::fs::create_dir_all(&deep).unwrap();
        std::fs::create_dir_all(root.join("saves/World")).unwrap();
        assert_eq!(find_resources(&root, 4), None);
        std::fs::write(deep.join("horizon-resources.json"), "{}").unwrap();
        assert_eq!(find_resources(&root, 4), Some(deep.clone()));
        assert_eq!(find_resources(&root, 1), None);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn keeps_a_copy_in_documents() {
        let root = std::env::temp_dir().join(format!("horizon-ios-keep-{}", std::process::id()));
        let bundle = root.join("Horizon.app/resources");
        std::fs::create_dir_all(bundle.join("assets")).unwrap();
        std::fs::write(bundle.join("horizon-resources.json"), "{}").unwrap();
        std::fs::write(bundle.join("assets/a.bin"), "a").unwrap();
        let docs = root.join("Documents");
        std::fs::create_dir_all(docs.join("resources.partial")).unwrap();
        let kept = keep(&bundle, &docs).unwrap();
        assert_eq!(kept, docs.join("resources"));
        assert_eq!(find_resources(&docs, 4), Some(kept.clone()));
        assert_eq!(
            std::fs::read_to_string(kept.join("assets/a.bin")).unwrap(),
            "a"
        );
        assert!(!docs.join("resources.partial").exists());
        std::fs::remove_dir_all(&root).unwrap();
    }
}
