use std::path::PathBuf;

pub fn get_pi_dir() -> PathBuf {
    crate::settings::get_pi_override_dir()
        .unwrap_or_else(|| crate::config::get_home_dir().join(".pi").join("agent"))
}

pub fn get_pi_sessions_dir() -> PathBuf {
    get_pi_dir().join("sessions")
}
