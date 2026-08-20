## MODIFIED Requirements

### Requirement: CodeFree-O MCP configuration management
The system SHALL support reading and writing CodeFree-O's MCP configuration file at `%HOME%/.codefree-o/.config/codefree.json`. The configuration format SHALL be compatible with the existing MCP config schema (same as opencode.json). After merging upstream changes, the MCP configuration management SHALL remain functional and the `McpApps` struct SHALL include the `codefree` field.

#### Scenario: Read MCP config for CodeFree after merge
- **WHEN** the MCP service reads configuration for app_type="codefree" after upstream merge
- **THEN** the system reads from `%HOME%/.codefree-o/.config/codefree.json`

#### Scenario: McpApps struct includes codefree field after merge
- **WHEN** the `McpApps` struct is initialized after upstream merge
- **THEN** the struct includes `codefree: bool` field alongside `grokbuild` and other app fields

### Requirement: CodeFree-O appears in MCP panel UI
The system SHALL display CodeFree-O as an available app in the MCP management panel, showing the count of MCP servers enabled for CodeFree-O. After merging upstream changes, the MCP panel UI SHALL continue to display CodeFree-O.

#### Scenario: MCP panel shows CodeFree app count after merge
- **WHEN** user opens the MCP management panel after upstream merge
- **THEN** the AppCountBar includes CodeFree with its teal badge showing the count of servers where `server.apps.codefree === true`

#### Scenario: MCP form includes CodeFree checkbox after merge
- **WHEN** user creates or edits an MCP server after upstream merge
- **THEN** the form includes a CodeFree checkbox in McpFormModal.tsx
