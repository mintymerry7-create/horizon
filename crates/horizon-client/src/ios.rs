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
    // Without resources the game says where to put them (no_resources.rs).
    let resources = find_resources(&docs, 4).unwrap_or_else(|| docs.join("resources"));
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
}
