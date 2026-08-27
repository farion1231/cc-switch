## ADDED Requirements

### Requirement: CodeFree-O skills directory symlink management
The system SHALL support managing skill symlinks for CodeFree-O with the skills root directory at `%HOME%/.codefree-o/skills`. The system SHALL use the same symlink mechanism as existing apps (opencode, hermes, etc.).

#### Scenario: Skills directory path resolution
- **WHEN** the skills service resolves the root path for app_type="codefree"
- **THEN** the system returns `%HOME%/.codefree-o/skills`

#### Scenario: Create skill symlink for CodeFree
- **WHEN** user creates a skill symlink for CodeFree-O
- **THEN** the symlink is created in `%HOME%/.codefree-o/skills/` pointing to the target skill directory

#### Scenario: List skills for CodeFree
- **WHEN** user views skills for CodeFree-O
- **THEN** the system lists all symlinks in `%HOME%/.codefree-o/skills/`

#### Scenario: Delete skill symlink for CodeFree
- **WHEN** user deletes a skill symlink for CodeFree-O
- **THEN** the symlink is removed from `%HOME%/.codefree-o/skills/`

### Requirement: CodeFree-O MCP configuration management
The system SHALL support reading and writing CodeFree-O's MCP configuration file at `%HOME%/.codefree-o/.config/codefree.json`. The configuration format SHALL be compatible with the existing MCP config schema (same as opencode.json).

#### Scenario: Read MCP config for CodeFree
- **WHEN** the MCP service reads configuration for app_type="codefree"
- **THEN** the system reads from `%HOME%/.codefree-o/.config/codefree.json`

#### Scenario: Write MCP config for CodeFree
- **WHEN** the MCP service writes configuration for app_type="codefree"
- **THEN** the system writes to `%HOME%/.codefree-o/.config/codefree.json`

#### Scenario: Config file does not exist
- **WHEN** the MCP config file for CodeFree-O does not exist
- **THEN** the system creates a new empty config file at the expected path

#### Scenario: Config directory does not exist
- **WHEN** the `.codefree-o/.config/` directory does not exist
- **THEN** the system creates the directory before writing the config file

### Requirement: CodeFree-O appears in Skills panel UI
The system SHALL display CodeFree-O as an available app in the Skills management panel, allowing users to toggle skill enablement for CodeFree-O and view skill counts.

#### Scenario: Skills panel shows CodeFree app toggle
- **WHEN** user opens the Skills management panel
- **THEN** the AppCountBar and AppToggleGroup include CodeFree with its teal badge and icon

#### Scenario: Skills panel counts CodeFree skills
- **WHEN** the Skills panel calculates enabled counts per app
- **THEN** CodeFree's count reflects the number of skills where `skill.apps.codefree === true`

#### Scenario: Toggle skill for CodeFree
- **WHEN** user toggles a skill's CodeFree app switch
- **THEN** the skill's `apps.codefree` field is updated and the MCP config is synced to `codefree.json`

### Requirement: CodeFree-O appears in MCP panel UI
The system SHALL display CodeFree-O as an available app in the MCP management panel, showing the count of MCP servers enabled for CodeFree-O.

#### Scenario: MCP panel shows CodeFree app count
- **WHEN** user opens the MCP management panel
- **THEN** the AppCountBar includes CodeFree with its teal badge showing the count of servers where `server.apps.codefree === true`

#### Scenario: MCP form shows CodeFree checkbox
- **WHEN** user creates or edits an MCP server
- **THEN** the form includes a CodeFree checkbox (already implemented in McpFormModal.tsx)
