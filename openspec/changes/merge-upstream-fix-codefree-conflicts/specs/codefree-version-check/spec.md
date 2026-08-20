## MODIFIED Requirements

### Requirement: CodeFree-O version detection
The system SHALL detect the installed version of CodeFree-O by running `codefree-o --version` command. The system SHALL parse the version string from the command output. After merging upstream changes, the version detection logic SHALL remain functional and the `tool_executable_candidates` SHALL include codefree-o executable names.

#### Scenario: CodeFree-O version detection after merge
- **WHEN** the version check runs for CodeFree-O after upstream merge
- **THEN** the system executes `codefree-o --version` and returns the parsed version string

#### Scenario: CodeFree-O not installed after merge
- **WHEN** `codefree-o --version` command fails (not found) after upstream merge
- **THEN** the system reports CodeFree-O as not installed

### Requirement: CodeFree-O version check in settings
The system SHALL include CodeFree-O in the "Settings > About > Local Environment Detection" section alongside other supported apps. After merging upstream changes, CodeFree-O SHALL continue to appear in the environment detection section.

#### Scenario: Environment detection displays CodeFree after merge
- **WHEN** user opens Settings > About > Local Environment Detection after upstream merge
- **THEN** CodeFree-O appears with its version status (installed/not installed, current version, upgrade available)
