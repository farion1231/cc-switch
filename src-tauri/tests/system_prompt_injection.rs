/// 测试 System Prompt 注入的拼接逻辑
/// 模拟 forwarder 中 injection_extra 的计算过程
#[cfg(test)]
mod system_prompt_injection_tests {
    use std::io::Write;

    /// 模拟 forwarder 中的注入拼接逻辑
    fn build_injection_content(
        file_content: &str,
        shared_content: &str,
        enabled: bool,
        receive_shared: bool,
    ) -> Option<String> {
        if !enabled {
            return None;
        }
        let mut parts = Vec::new();
        let trimmed = file_content.trim();
        if !trimmed.is_empty() {
            parts.push(trimmed.to_string());
        }
        if receive_shared {
            let t = shared_content.trim();
            if !t.is_empty() {
                parts.push(t.to_string());
            }
        }
        if parts.is_empty() {
            None
        } else {
            Some(parts.join("\n\n---\n\n"))
        }
    }

    #[test]
    fn injection_combines_file_and_shared() {
        let result = build_injection_content(
            "测试数据：111",
            "测试数据：222",
            true,
            true,
        );
        assert_eq!(
            result.as_deref(),
            Some("测试数据：111\n\n---\n\n测试数据：222")
        );
    }

    #[test]
    fn injection_disabled_returns_none() {
        let result = build_injection_content("111", "222", false, true);
        assert_eq!(result, None);
    }

    #[test]
    fn injection_file_only_when_shared_disabled() {
        let result = build_injection_content("111", "222", true, false);
        assert_eq!(result.as_deref(), Some("111"));
    }

    #[test]
    fn injection_file_only_when_shared_empty() {
        let result = build_injection_content("111", "", true, true);
        assert_eq!(result.as_deref(), Some("111"));
    }

    #[test]
    fn injection_shared_only_when_file_empty() {
        let result = build_injection_content("", "222", true, true);
        assert_eq!(result.as_deref(), Some("222"));
    }

    #[test]
    fn injection_both_empty_returns_none() {
        let result = build_injection_content("", "", true, true);
        assert_eq!(result, None);
    }

    #[test]
    fn injection_none_when_disabled_even_with_content() {
        let result = build_injection_content("111", "222", false, true);
        assert_eq!(result, None);
    }
}
