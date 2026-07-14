pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    use super::VERSION;

    #[test]
    fn exposes_workspace_version() {
        assert_eq!(VERSION, env!("CARGO_PKG_VERSION"));
    }
}
