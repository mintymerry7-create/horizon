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
        .or_else(|| keep(&bundled_archive()?, &docs).ok())
        .unwrap_or_else(|| docs.join("resources"));
    args.extend(["--resources".to_owned(), resources.display().to_string()]);
    // The Microsoft account signed in on an earlier run (Options > Online), from the keychain.
    horizon_client::microsoft::restore();
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

/// The prepared resources inside the app bundle, next to the executable: one
/// zip, since signing and installing an app checks every file of the bundle
/// and 28,000 of them stall the installer.
fn bundled_archive() -> Option<PathBuf> {
    let zip = std::env::current_exe()
        .ok()?
        .parent()?
        .join("resources.zip");
    zip.is_file().then_some(zip)
}

/// Unzips the bundled resources `zip` to `docs/resources`, whole or not at all.
fn keep(zip: &Path, docs: &Path) -> std::io::Result<PathBuf> {
    let started = std::time::Instant::now();
    let partial = docs.join("resources.partial");
    let _ = std::fs::remove_dir_all(&partial);
    zip::ZipArchive::new(std::fs::File::open(zip)?)
        .and_then(|mut archive| archive.extract(&partial))
        .map_err(std::io::Error::other)?;
    let to = docs.join("resources");
    let _ = std::fs::remove_dir_all(&to);
    std::fs::rename(&partial, &to)?;
    // Before the game's log starts: stderr.
    eprintln!(
        "Horizon: unzipped the bundled resources in {:?}",
        started.elapsed()
    );
    Ok(to)
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
    fn unzips_the_bundled_resources_into_documents() {
        use std::io::Write;
        let root = std::env::temp_dir().join(format!("horizon-ios-keep-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let bundle = root.join("resources.zip");
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&bundle).unwrap());
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file("horizon-resources.json", options).unwrap();
        zip.write_all(b"{}").unwrap();
        zip.start_file("assets/a.bin", options).unwrap();
        zip.write_all(b"a").unwrap();
        zip.finish().unwrap();
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
