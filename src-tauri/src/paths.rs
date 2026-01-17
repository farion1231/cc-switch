use std::path::PathBuf;

pub fn home_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        if let Some(home) = std::env::var_os("USERPROFILE").filter(|v| !v.is_empty()) {
            return Some(PathBuf::from(home));
        }

        match (std::env::var_os("HOMEDRIVE"), std::env::var_os("HOMEPATH")) {
            (Some(drive), Some(path)) if !drive.is_empty() && !path.is_empty() => {
                let mut out = PathBuf::from(drive);
                out.push(path);
                return Some(out);
            }
            _ => {}
        }
    }

    #[cfg(not(windows))]
    {
        if let Some(home) = std::env::var_os("HOME").filter(|v| !v.is_empty()) {
            return Some(PathBuf::from(home));
        }
    }

    dirs::home_dir()
}

pub fn home_dir_or_current() -> PathBuf {
    home_dir().unwrap_or_else(|| {
        log::warn!("Failed to resolve user home directory; falling back to current directory");
        PathBuf::from(".")
    })
}

