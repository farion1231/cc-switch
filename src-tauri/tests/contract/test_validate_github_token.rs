#[cfg(test)]
mod tests {
    use serde_json::json;

    #[test]
    fn test_validate_github_token_valid() {
        // This test MUST fail initially (TDD approach)
        let input = json!({
            "github_token": "ghp_validtoken123456789"
        });

        // Expected output for valid token
        let expected = json!({
            "valid": true,
            "username": "testuser",
            "scopes": ["gist", "user:email"],
            "has_gist_permission": true,
            "error_message": null
        });

        // TODO: Invoke validate_github_token command
        // let result = validate_github_token(input);

        panic!("validate_github_token command not implemented");
    }

    #[test]
    fn test_validate_github_token_invalid() {
        let input = json!({
            "github_token": "invalid_token"
        });

        // Expected output for invalid token
        let expected = json!({
            "valid": false,
            "username": null,
            "scopes": [],
            "has_gist_permission": false,
            "error_message": "Bad credentials"
        });

        panic!("validate_github_token command not implemented");
    }

    #[test]
    fn test_validate_github_token_insufficient_scopes() {
        let input = json!({
            "github_token": "ghp_noscopes123456789"
        });

        // Expected output for token without gist scope
        let expected = json!({
            "valid": true,
            "username": "testuser",
            "scopes": ["repo"],
            "has_gist_permission": false,
            "error_message": "Token lacks gist permission"
        });

        panic!("validate_github_token command not implemented");
    }
}