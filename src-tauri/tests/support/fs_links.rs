use std::{fs, io, path::Path};

/// Create a real link to an existing directory, including on unprivileged Windows.
/// Junctions preserve these directory-link fixtures, not file, dangling or relative symlinks.
pub fn symlink_dir(source: &Path, destination: &Path) -> io::Result<()> {
    #[cfg(unix)]
    std::os::unix::fs::symlink(source, destination)?;

    #[cfg(windows)]
    match std::os::windows::fs::symlink_dir(source, destination) {
        Ok(()) => {}
        Err(error) if error.raw_os_error() == Some(1314) => {
            use std::os::windows::process::CommandExt;
            use std::process::Command;

            // Preserve native symlink coverage where available. Only privilege failure
            // falls back; existing destinations and other errors must still fail.
            let output = Command::new("cmd.exe")
                .args(["/D", "/V:OFF", "/C"])
                .raw_arg(r#"mklink /J "%CC_SWITCH_TEST_LINK_DEST%" "%CC_SWITCH_TEST_LINK_SOURCE%""#)
                .env("CC_SWITCH_TEST_LINK_DEST", destination)
                .env("CC_SWITCH_TEST_LINK_SOURCE", source)
                .output()?;
            if !output.status.success() {
                return Err(io::Error::other(format!(
                    "create test directory junction failed ({}): {}",
                    output.status,
                    String::from_utf8_lossy(&output.stderr).trim()
                )));
            }
        }
        Err(error) => return Err(error),
    }

    assert!(fs::symlink_metadata(destination)?.file_type().is_symlink());
    assert_eq!(fs::canonicalize(destination)?, fs::canonicalize(source)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn directory_link_preserves_target_and_handles_special_paths() {
        let root = tempfile::tempdir().expect("tempdir");
        let source = root.path().join("source & %PATH% ! 中文");
        let destination = root.path().join("link & %PATH% ! 中文");
        fs::create_dir(&source).expect("create source");
        fs::write(source.join("sentinel"), "keep target").expect("write sentinel");

        symlink_dir(&source, &destination).expect("create directory link");
        assert_eq!(
            fs::canonicalize(fs::read_link(&destination).expect("read link"))
                .expect("resolve link"),
            fs::canonicalize(&source).expect("resolve source")
        );
        #[cfg(unix)]
        fs::remove_file(&destination).expect("unlink");
        #[cfg(windows)]
        fs::remove_dir(&destination).expect("unlink");
        assert_eq!(
            fs::read_to_string(source.join("sentinel")).expect("read sentinel"),
            "keep target"
        );
    }

    #[test]
    fn directory_link_does_not_replace_existing_destination() {
        let root = tempfile::tempdir().expect("tempdir");
        let source = root.path().join("source");
        let destination = root.path().join("existing");
        fs::create_dir(&source).expect("create source");
        fs::create_dir(&destination).expect("create destination");
        fs::write(destination.join("sentinel"), "keep existing").expect("write sentinel");

        assert!(symlink_dir(&source, &destination).is_err());
        assert!(!fs::symlink_metadata(&destination)
            .expect("destination metadata")
            .file_type()
            .is_symlink());
        assert_eq!(
            fs::read_to_string(destination.join("sentinel")).expect("read sentinel"),
            "keep existing"
        );
    }
}
