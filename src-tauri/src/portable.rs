use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(windows)]
use std::os::windows::fs::MetadataExt;

const PORTABLE_MARKER: &str = "portable.ini";
const EXTERNAL_WRITE_ERROR_PREFIX: &str = "portable mode blocks external write:";
const PROBE_ATTEMPTS: usize = 8;
#[cfg(windows)]
const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;

static PORTABLE_PATHS: OnceLock<Option<PortablePaths>> = OnceLock::new();
static INITIALIZATION_RESULT: OnceLock<Result<(), String>> = OnceLock::new();
static PROBE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PortablePaths {
    root: PathBuf,
    data: PathBuf,
    app: PathBuf,
    cache: PathBuf,
    temp: PathBuf,
    tauri: PathBuf,
    webview2: PathBuf,
}

impl PortablePaths {
    fn from_executable(executable: &Path) -> Result<Self, String> {
        let root = executable
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .ok_or_else(|| {
                format!(
                    "portable initialization failed: executable has no parent: {}",
                    executable.display()
                )
            })?
            .to_path_buf();
        let data = root.join("data");

        Ok(Self {
            app: data.join("app"),
            cache: data.join("cache"),
            temp: data.join("temp"),
            tauri: data.join("tauri"),
            webview2: data.join("webview2"),
            root,
            data,
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn data_dir(&self) -> &Path {
        &self.data
    }

    pub fn app_dir(&self) -> &Path {
        &self.app
    }

    pub fn cache_dir(&self) -> &Path {
        &self.cache
    }

    pub fn temp_dir(&self) -> &Path {
        &self.temp
    }

    pub fn tauri_dir(&self) -> &Path {
        &self.tauri
    }

    pub fn webview2_dir(&self) -> &Path {
        &self.webview2
    }

    fn write_directories(&self) -> [&Path; 6] {
        [
            self.data_dir(),
            self.app_dir(),
            self.cache_dir(),
            self.temp_dir(),
            self.tauri_dir(),
            self.webview2_dir(),
        ]
    }

    fn data_subdirectories(&self) -> [&Path; 5] {
        [
            self.app_dir(),
            self.cache_dir(),
            self.temp_dir(),
            self.tauri_dir(),
            self.webview2_dir(),
        ]
    }
}

pub fn initialize() -> Result<(), String> {
    INITIALIZATION_RESULT.get_or_init(initialize_once).clone()
}

fn initialize_once() -> Result<(), String> {
    let executable = std::env::current_exe()
        .map_err(|error| format!("portable initialization failed: current_exe: {error}"))?;
    let candidate = PortablePaths::from_executable(&executable)?;
    let marker_path = candidate.root().join(PORTABLE_MARKER);

    match detect_portable_marker(&marker_path) {
        Ok(false) => {
            let _ = PORTABLE_PATHS.set(None);
            return Ok(());
        }
        Ok(true) => {
            let _ = PORTABLE_PATHS.set(Some(candidate));
        }
        Err(error) => {
            let _ = PORTABLE_PATHS.set(Some(candidate));
            return Err(error);
        }
    }

    if let Some(paths) = paths() {
        prepare_portable_directories(paths)?;
        std::env::set_var("TEMP", paths.temp_dir());
        std::env::set_var("TMP", paths.temp_dir());
    }

    Ok(())
}

fn detect_portable_marker(marker_path: &Path) -> Result<bool, String> {
    match fs::symlink_metadata(marker_path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || platform_metadata_is_reparse_point(&metadata) {
                Err(format!(
                    "portable initialization failed: marker is a link or reparse point: {}",
                    marker_path.display()
                ))
            } else if metadata.is_file() {
                Ok(true)
            } else {
                Err(format!(
                    "portable initialization failed: marker is not a regular file: {}",
                    marker_path.display()
                ))
            }
        }
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(false),
        Err(error) => Err(format!(
            "portable initialization failed: inspect marker {}: {error}",
            marker_path.display()
        )),
    }
}

pub fn is_portable() -> bool {
    paths().is_some()
}

pub fn paths() -> Option<&'static PortablePaths> {
    PORTABLE_PATHS.get().and_then(Option::as_ref)
}

pub fn require_external_write(operation: &str) -> Result<(), String> {
    require_external_write_for_mode(is_portable(), operation)
}

fn require_external_write_for_mode(portable: bool, operation: &str) -> Result<(), String> {
    if portable {
        Err(format!("{EXTERNAL_WRITE_ERROR_PREFIX} {operation}"))
    } else {
        Ok(())
    }
}

fn prepare_portable_directories(paths: &PortablePaths) -> Result<(), String> {
    let directories = paths.write_directories();

    reject_managed_reparse_points(paths)?;

    for directory in directories {
        fs::create_dir_all(directory).map_err(|error| {
            format!(
                "portable initialization failed: create directory {}: {error}",
                directory.display()
            )
        })?;
    }

    reject_managed_reparse_points(paths)?;
    validate_canonical_layout(paths)?;

    for directory in directories {
        probe_directory_writable(directory)?;
    }

    Ok(())
}

fn reject_managed_reparse_points(paths: &PortablePaths) -> Result<(), String> {
    reject_reparse_point(paths.root())?;
    reject_reparse_points_in_tree(paths.data_dir())
}

fn reject_reparse_points_in_tree(root: &Path) -> Result<(), String> {
    let mut pending = vec![root.to_path_buf()];

    while let Some(path) = pending.pop() {
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(format!(
                    "portable initialization failed: inspect managed path {}: {error}",
                    path.display()
                ));
            }
        };
        if metadata.file_type().is_symlink() || platform_metadata_is_reparse_point(&metadata) {
            return Err(format!(
                "portable initialization failed: managed path is a link or reparse point: {}",
                path.display()
            ));
        }
        if !metadata.is_dir() {
            continue;
        }

        let entries = fs::read_dir(&path).map_err(|error| {
            format!(
                "portable initialization failed: read managed directory {}: {error}",
                path.display()
            )
        })?;
        for entry in entries {
            let entry = entry.map_err(|error| {
                format!(
                    "portable initialization failed: enumerate managed directory {}: {error}",
                    path.display()
                )
            })?;
            pending.push(entry.path());
        }
    }

    Ok(())
}

fn reject_reparse_point(path: &Path) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || platform_metadata_is_reparse_point(&metadata) {
                Err(format!(
                    "portable initialization failed: managed path is a link or reparse point: {}",
                    path.display()
                ))
            } else {
                Ok(())
            }
        }
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!(
            "portable initialization failed: inspect managed path {}: {error}",
            path.display()
        )),
    }
}

#[cfg(windows)]
fn platform_metadata_is_reparse_point(metadata: &fs::Metadata) -> bool {
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(windows))]
fn platform_metadata_is_reparse_point(_metadata: &fs::Metadata) -> bool {
    false
}

fn validate_canonical_layout(paths: &PortablePaths) -> Result<(), String> {
    let canonical_root = fs::canonicalize(paths.root()).map_err(|error| {
        format!(
            "portable initialization failed: resolve root {}: {error}",
            paths.root().display()
        )
    })?;
    let canonical_data = fs::canonicalize(paths.data_dir()).map_err(|error| {
        format!(
            "portable initialization failed: resolve data directory {}: {error}",
            paths.data_dir().display()
        )
    })?;

    if canonical_data.parent() != Some(canonical_root.as_path()) {
        return Err(format!(
            "portable initialization failed: data directory escaped portable root: {}",
            paths.data_dir().display()
        ));
    }

    for directory in paths.data_subdirectories() {
        let canonical_directory = fs::canonicalize(directory).map_err(|error| {
            format!(
                "portable initialization failed: resolve data subdirectory {}: {error}",
                directory.display()
            )
        })?;
        if canonical_directory.parent() != Some(canonical_data.as_path()) {
            return Err(format!(
                "portable initialization failed: data subdirectory escaped data root: {}",
                directory.display()
            ));
        }
    }

    Ok(())
}

fn probe_directory_writable(directory: &Path) -> Result<(), String> {
    for _ in 0..PROBE_ATTEMPTS {
        let probe_path = next_probe_path(directory);
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&probe_path)
        {
            Ok(mut probe) => {
                let write_result = probe.write_all(b"cc-switch portable write probe");
                drop(probe);
                let cleanup_result = fs::remove_file(&probe_path);

                return match (write_result, cleanup_result) {
                    (Ok(()), Ok(())) => Ok(()),
                    (Ok(()), Err(cleanup_error)) => Err(format!(
                        "portable initialization failed: remove write probe {}: {cleanup_error}",
                        probe_path.display()
                    )),
                    (Err(write_error), Ok(())) => Err(format!(
                        "portable initialization failed: directory is not writable ({}): {error}",
                        directory.display(),
                        error = write_error
                    )),
                    (Err(write_error), Err(cleanup_error)) => Err(format!(
                        "portable initialization failed: directory is not writable ({}): {write_error}; cleanup failed for {}: {cleanup_error}",
                        directory.display(),
                        probe_path.display()
                    )),
                };
            }
            Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(format!(
                    "portable initialization failed: directory is not writable ({}): {error}",
                    directory.display()
                ));
            }
        }
    }

    Err(format!(
        "portable initialization failed: could not create a unique write probe in {} after {PROBE_ATTEMPTS} attempts",
        directory.display()
    ))
}

fn next_probe_path(directory: &Path) -> PathBuf {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_else(|error| error.duration().as_nanos());
    let sequence = PROBE_SEQUENCE.fetch_add(1, Ordering::Relaxed);

    directory.join(format!(
        ".cc-switch-write-probe-{}-{timestamp}-{sequence}",
        std::process::id()
    ))
}

#[cfg(test)]
mod tests {
    use super::{
        detect_portable_marker, prepare_portable_directories, require_external_write_for_mode,
        PortablePaths,
    };
    use std::fs;
    use std::path::Path;

    #[cfg(windows)]
    #[test]
    fn derives_paths_from_windows_executable() {
        let paths =
            PortablePaths::from_executable(Path::new(r"D:\Portable\CC-Switch\cc-switch.exe"))
                .expect("the executable has a parent directory");

        assert_eq!(paths.root(), Path::new(r"D:\Portable\CC-Switch"));
        assert_eq!(paths.data_dir(), Path::new(r"D:\Portable\CC-Switch\data"));
        assert_eq!(paths.app_dir(), paths.data_dir().join("app"));
        assert_eq!(paths.cache_dir(), paths.data_dir().join("cache"));
        assert_eq!(paths.temp_dir(), paths.data_dir().join("temp"));
        assert_eq!(paths.tauri_dir(), paths.data_dir().join("tauri"));
        assert_eq!(paths.webview2_dir(), paths.data_dir().join("webview2"));
    }

    #[test]
    fn external_write_guard_only_blocks_portable_mode() {
        assert!(require_external_write_for_mode(false, "test").is_ok());

        let error = require_external_write_for_mode(true, "test")
            .expect_err("portable mode must reject external writes");
        assert!(error.starts_with("portable mode blocks external write:"));
    }

    #[test]
    fn missing_marker_selects_normal_mode() {
        let directory = tempfile::tempdir().expect("create test directory");
        let marker = directory.path().join("portable.ini");

        assert!(!detect_portable_marker(&marker).expect("missing marker is valid"));
    }

    #[test]
    fn marker_directory_is_rejected() {
        let directory = tempfile::tempdir().expect("create test directory");
        let marker = directory.path().join("portable.ini");
        fs::create_dir(&marker).expect("create marker directory");

        let error = detect_portable_marker(&marker)
            .expect_err("the portable marker must be a regular file");
        assert!(error.contains("not a regular file"));
    }

    #[cfg(unix)]
    #[test]
    fn dangling_marker_link_is_rejected_instead_of_selecting_normal_mode() {
        use std::os::unix::fs::symlink;

        let directory = tempfile::tempdir().expect("create test directory");
        let marker = directory.path().join("portable.ini");
        symlink(directory.path().join("missing.ini"), &marker)
            .expect("create dangling marker link");

        let error = detect_portable_marker(&marker)
            .expect_err("dangling marker link must not select normal mode");
        assert!(error.contains("link or reparse point"));
    }

    #[test]
    fn every_write_directory_stays_beneath_data() {
        let executable = Path::new("portable-root").join("cc-switch.exe");
        let paths = PortablePaths::from_executable(&executable)
            .expect("the executable has a parent directory");
        let directories = paths.write_directories();

        assert_eq!(directories.len(), 6);
        assert!(directories
            .iter()
            .all(|directory| directory.starts_with(paths.data_dir())));
    }

    #[test]
    fn successful_probe_leaves_no_file() {
        let directory = tempfile::tempdir().expect("create test directory");

        super::probe_directory_writable(directory.path()).expect("probe succeeds");

        let entries: Vec<_> = fs::read_dir(directory.path())
            .expect("read test directory")
            .collect::<Result<_, _>>()
            .expect("read directory entries");
        assert!(entries.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn portable_initialization_rejects_linked_data_directory_before_writing() {
        use std::os::unix::fs::symlink;

        let directory = tempfile::tempdir().expect("create test directory");
        let root = directory.path().join("portable-root");
        let outside = directory.path().join("outside");
        fs::create_dir_all(&root).expect("create portable root");
        fs::create_dir_all(&outside).expect("create outside directory");
        symlink(&outside, root.join("data")).expect("link data directory outside root");
        let paths = PortablePaths::from_executable(&root.join("cc-switch.exe"))
            .expect("derive portable paths");

        let error = prepare_portable_directories(&paths)
            .expect_err("linked data directory must be rejected");

        assert!(error.contains("link or reparse point"));
        assert!(fs::read_dir(&outside)
            .expect("read outside directory")
            .next()
            .is_none());
    }

    #[cfg(unix)]
    #[test]
    fn portable_initialization_rejects_nested_file_link_before_writing() {
        use std::os::unix::fs::symlink;

        let directory = tempfile::tempdir().expect("create test directory");
        let root = directory.path().join("portable-root");
        let app = root.join("data").join("app");
        let outside = directory.path().join("outside.log");
        fs::create_dir_all(&app).expect("create portable app directory");
        fs::write(&outside, "unchanged").expect("create outside file");
        symlink(&outside, app.join("crash.log")).expect("link managed file outside data");
        let paths = PortablePaths::from_executable(&root.join("cc-switch.exe"))
            .expect("derive portable paths");

        let error =
            prepare_portable_directories(&paths).expect_err("nested managed link must be rejected");

        assert!(error.contains("link or reparse point"));
        assert_eq!(
            fs::read_to_string(&outside).expect("read outside file"),
            "unchanged"
        );
    }
}
