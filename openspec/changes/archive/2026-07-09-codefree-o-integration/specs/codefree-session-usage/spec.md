## ADDED Requirements

### Requirement: CodeFree-O database path discovery
The system SHALL discover the CodeFree-O SQLite database at `%HOME%/.codefree-o/.local/share/codefree.db` by default. The system SHALL support `CODEFREE_DB` environment variable to override the default path. The system SHALL expose `get_codefree_data_dir()` returning the data directory path.

#### Scenario: Default database path
- **WHEN** `CODEFREE_DB` environment variable is not set
- **THEN** the system uses `%HOME%/.codefree-o/.local/share/codefree.db` as the database path

#### Scenario: Environment variable override
- **WHEN** `CODEFREE_DB` environment variable is set to a valid path
- **THEN** the system uses the `CODEFREE_DB` value as the database path

### Requirement: CodeFree-O session usage synchronization
The system SHALL synchronize CodeFree-O session and message data from the SQLite database with app_type="codefree", provider_id="_codefree_session", data_source="codefree_session". The request_id format SHALL be `codefree_session:{session_id}:{message_id}`.

#### Scenario: First-time synchronization
- **WHEN** cc-switch starts and CodeFree-O database exists
- **THEN** all sessions and messages are synchronized with the correct app_type, provider_id, and request_id format

#### Scenario: Periodic synchronization
- **WHEN** the periodic sync timer fires
- **THEN** new and updated CodeFree-O sessions are synchronized

#### Scenario: Database not found
- **WHEN** CodeFree-O database file does not exist
- **THEN** synchronization is skipped without error

### Requirement: CodeFree-O token cost calculation
The system SHALL calculate token costs for CodeFree-O sessions. When the `cost` field in the database is 0, the system SHALL fall back to `find_model_pricing` for price lookup.

#### Scenario: Cost field is zero
- **WHEN** a CodeFree-O message has cost=0
- **THEN** the system uses `find_model_pricing` to calculate the cost based on model and token counts

#### Scenario: Cost field is non-zero
- **WHEN** a CodeFree-O message has cost>0
- **THEN** the system uses the stored cost value directly

### Requirement: CodeFree-O usage stats inclusion
The system SHALL include CodeFree-O data in the `allow_missing_cache_creation` match with value "codefree". CodeFree-O SHALL NOT be included in `CACHE_INCLUSIVE_APP_TYPES`.

#### Scenario: Usage stats aggregation
- **WHEN** usage statistics are calculated
- **THEN** CodeFree-O sessions are included in the aggregation with app_type="codefree"
