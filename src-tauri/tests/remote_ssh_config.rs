use cc_switch_lib::remote::ssh_config::discover_ssh_targets_from_path;

#[test]
fn discovers_concrete_hosts_with_includes_defaults_and_overrides() {
    let temp = tempfile::tempdir().expect("temp dir");
    let ssh_dir = temp.path().join(".ssh");
    let include_dir = ssh_dir.join("conf.d");
    std::fs::create_dir_all(&include_dir).expect("create SSH config directory");

    // 该样例同时约束 Include 的就地展开顺序和 OpenSSH“首个值生效”语义，
    // 后续扩展解析器时不能简单改成后写覆盖前写。
    std::fs::write(
        ssh_dir.join("config"),
        r#"
Host production
    HostName 10.0.0.8
    User deploy
    Port 2222
    IdentityFile ~/.ssh/production

Include conf.d/*.conf

Host *
    User fallback-user
    Port 22
"#,
    )
    .expect("write main SSH config");
    std::fs::write(
        include_dir.join("staging.conf"),
        r#"
Host staging
    HostName staging.internal
    IdentityFile ~/.ssh/staging

Host staging *.internal !blocked
    User release
"#,
    )
    .expect("write included SSH config");

    let targets =
        discover_ssh_targets_from_path(&ssh_dir.join("config")).expect("discover SSH targets");

    assert_eq!(targets.len(), 2);
    assert_eq!(targets[0].host_alias, "production");
    assert_eq!(targets[0].hostname.as_deref(), Some("10.0.0.8"));
    assert_eq!(targets[0].username.as_deref(), Some("deploy"));
    assert_eq!(targets[0].port, Some(2222));
    assert_eq!(
        targets[0].identity_file.as_deref(),
        Some("~/.ssh/production")
    );
    assert_eq!(targets[1].host_alias, "staging");
    assert_eq!(targets[1].hostname.as_deref(), Some("staging.internal"));
    assert_eq!(targets[1].username.as_deref(), Some("release"));
    assert_eq!(targets[1].port, Some(22));
}

#[test]
fn filters_patterns_negations_and_duplicate_host_aliases() {
    let temp = tempfile::tempdir().expect("temp dir");
    let config = temp.path().join("config");
    std::fs::write(
        &config,
        r#"
Host * *.internal ?ingle !blocked
    User ignored
Host production production
    HostName prod.example.com
Host Production
    Port 2200
"#,
    )
    .expect("write SSH config");

    let targets = discover_ssh_targets_from_path(&config).expect("discover SSH targets");

    assert_eq!(targets.len(), 1);
    assert_eq!(targets[0].host_alias, "production");
    assert_eq!(targets[0].hostname.as_deref(), Some("prod.example.com"));
    assert_eq!(targets[0].port, Some(2200));
}
