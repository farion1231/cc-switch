## MODIFIED Requirements

### Requirement: CodeFree-O database path discovery
The system SHALL discover the CodeFree-O SQLite database at `%HOME%/.codefree-o/.local/share/codefree.db` by default. The system SHALL support `CODEFREE_DB` environment variable to override the default path. The system SHALL expose `get_codefree_data_dir()` returning the data directory path. After merging upstream changes, the database path discovery logic SHALL remain unchanged and functional.

#### Scenario: Default database path after upstream merge
- **WHEN** `CODEFREE_DB` environment variable is not set and upstream changes are merged
- **THEN** the system uses `%HOME%/.codefree-o/.local/share/codefree.db` as the database path

#### Scenario: Environment variable override after upstream merge
- **WHEN** `CODEFREE_DB` environment variable is set to a valid path and upstream changes are merged
- **THEN** the system uses the `CODEFREE_DB` value as the database path

#### Scenario: Schema migration chain integrity after merge
- **WHEN** upstream schema.rs changes are merged with codefree migration changes
- **THEN** the migration chain v13→v14→v15→v16 remains intact and `SCHEMA_VERSION = 16`

### Requirement: CodeFree-O session usage synchronization
The system SHALL synchronize CodeFree-O session and message data from the SQLite database with app_type="codefree", provider_id="_codefree_session", data_source="codefree_session". The request_id format SHALL be `codefree_session:{session_id}:{message_id}`. After merging upstream changes, the synchronization logic SHALL remain functional and the `sync_all_unlocked` function SHALL include the codefree sync call.

#### Scenario: Synchronization after upstream merge
- **WHEN** cc-switch starts after merging upstream changes and CodeFree-O database exists
- **THEN** all sessions and messages are synchronized with the correct app_type, provider_id, and request_id format

#### Scenario: sync_all_unlocked includes codefree after merge
- **WHEN** the `sync_all_unlocked` function is called after upstream merge
- **THEN** `sync_codefree_usage` is called alongside other app sync functions
