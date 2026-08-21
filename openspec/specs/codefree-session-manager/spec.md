# CodeFree Session Manager

## Purpose

Define session management capabilities for CodeFree-O within CC Switch, including session scanning, message loading, deletion, and provider root path resolution.

## Requirements

### Requirement: CodeFree-O session scanning
The system SHALL scan CodeFree-O sessions from the SQLite database in a dedicated thread (7th thread in parallel scan). The provider_id SHALL be "codefree". The resume_command SHALL be `codefree -s {session_id}`.

#### Scenario: Parallel session scan
- **WHEN** session scanning is initiated
- **THEN** CodeFree-O sessions are scanned in a separate thread alongside the existing 6 threads

#### Scenario: Session with messages
- **WHEN** a CodeFree-O session has messages in the database
- **THEN** the session is returned with its metadata (id, title, created_at, updated_at, message_count)

#### Scenario: Empty database
- **WHEN** CodeFree-O database exists but has no sessions
- **THEN** an empty session list is returned

### Requirement: CodeFree-O message loading
The system SHALL load messages for a CodeFree-O session from the SQLite database. The system SHALL use the same SQL queries as opencode (identical schema).

#### Scenario: Load messages for a session
- **WHEN** user selects a CodeFree-O session
- **THEN** all messages for that session are loaded from the database with role, content, and timestamp

#### Scenario: Session not found
- **WHEN** the requested session_id does not exist in the database
- **THEN** an empty message list is returned

### Requirement: CodeFree-O session deletion
The system SHALL support deleting a CodeFree-O session from the SQLite database. The system SHALL NOT support JSON file storage (unlike opencode).

#### Scenario: Delete a session
- **WHEN** user deletes a CodeFree-O session
- **THEN** the session and all its messages are removed from the SQLite database

#### Scenario: Non-SQLite storage attempt
- **WHEN** a delete is attempted on a non-SQLite storage path
- **THEN** the system returns an error indicating SQLite-only support

### Requirement: CodeFree-O provider root path
The system SHALL return `%HOME%/.codefree-o` as the provider root path for CodeFree-O in `provider_roots`.

#### Scenario: Provider root path lookup
- **WHEN** `provider_roots` is called with app_type="codefree"
- **THEN** the system returns the path from `get_codefree_data_dir()`
