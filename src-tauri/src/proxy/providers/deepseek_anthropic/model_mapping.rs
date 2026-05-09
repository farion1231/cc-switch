pub fn map_claude_to_deepseek(claude_model: &str) -> &'static str {
    let lower = claude_model.to_ascii_lowercase();
    if lower.contains("opus") {
        return "deepseek-v4-pro";
    }
    if lower.contains("sonnet") {
        return "deepseek-v4-flash";
    }
    if lower.contains("haiku") {
        return "deepseek-v4-flash";
    }
    "deepseek-v4-flash"
}

pub fn is_reasoner_target(deepseek_model: &str) -> bool {
    deepseek_model.contains("pro") || deepseek_model.contains("reasoner")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_opus_maps_to_pro() {
        assert_eq!(map_claude_to_deepseek("claude-opus-4-7"), "deepseek-v4-pro");
    }

    #[test]
    fn test_sonnet_maps_to_flash() {
        assert_eq!(map_claude_to_deepseek("claude-sonnet-4-6"), "deepseek-v4-flash");
    }

    #[test]
    fn test_haiku_maps_to_flash() {
        assert_eq!(map_claude_to_deepseek("claude-haiku-3-5"), "deepseek-v4-flash");
    }

    #[test]
    fn test_unknown_maps_to_flash() {
        assert_eq!(map_claude_to_deepseek("some-unknown-model"), "deepseek-v4-flash");
    }

    #[test]
    fn test_case_insensitive_opus() {
        assert_eq!(map_claude_to_deepseek("CLAUDE-OPUS-4"), "deepseek-v4-pro");
    }

    #[test]
    fn test_is_reasoner_target_pro() {
        assert!(is_reasoner_target("deepseek-v4-pro"));
    }

    #[test]
    fn test_is_reasoner_target_reasoner() {
        assert!(is_reasoner_target("deepseek-reasoner"));
    }

    #[test]
    fn test_is_reasoner_target_flash() {
        assert!(!is_reasoner_target("deepseek-v4-flash"));
    }
}
