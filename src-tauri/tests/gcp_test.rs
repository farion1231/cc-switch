use gcp_auth::TokenProvider;

#[tokio::test]
async fn test_gcp_auth() -> anyhow::Result<()> {
    // let provider = gcp_auth::provider().await?;
    let creds = gcp_auth::CustomServiceAccount::from_file("path/to/acct.json")?;
    let scopes = &["https://www.googleapis.com/auth/cloud-platform"];
    let token = creds.token(scopes).await?;

    println!("Access Token: {token:#?}",);
    println!("Access Token as_str: {}", token.as_str());
    println!("Access Token expires_at: {}", token.expires_at());
    println!("Access Token has_expired: {}", token.has_expired());
    Ok(())
}
