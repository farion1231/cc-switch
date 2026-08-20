## MODIFIED Requirements

### Requirement: Database path resolution with custom directory
When a custom CodeFree configuration directory is set, the system SHALL append `.local/share` as the data sub-path before resolving the database file path. The database path SHALL be `<custom_root>/.local/share/codefree.db`, consistent with the default path structure.

#### Scenario: Custom directory database path
- **WHEN** user sets a custom CodeFree configuration directory to `/custom/path`
- **THEN** the database path SHALL resolve to `/custom/path/.local/share/codefree.db`

#### Scenario: Default directory database path
- **WHEN** no custom directory is set
- **THEN** the database path SHALL resolve to `~/.codefree-o/.local/share/codefree.db`
