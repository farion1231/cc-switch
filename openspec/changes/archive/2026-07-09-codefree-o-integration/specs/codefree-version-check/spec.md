## ADDED Requirements

### Requirement: CodeFree-O version detection
The system SHALL detect the installed version of CodeFree-O by running `codefree-o --version` command. The system SHALL parse the version string from the command output.

#### Scenario: CodeFree-O is installed
- **WHEN** the version check runs for CodeFree-O
- **THEN** the system executes `codefree-o --version` and returns the parsed version string

#### Scenario: CodeFree-O is not installed
- **WHEN** `codefree-o --version` command fails (not found)
- **THEN** the system reports CodeFree-O as not installed

### Requirement: CodeFree-O upgrade command
The system SHALL provide the upgrade command `codefree-o upgrade` for CodeFree-O in the environment detection section.

#### Scenario: Upgrade available
- **WHEN** a newer version of CodeFree-O is available
- **THEN** the system displays the upgrade command `codefree-o upgrade`

#### Scenario: User triggers upgrade
- **WHEN** user clicks the upgrade button for CodeFree-O
- **THEN** the system executes `codefree-o upgrade`

### Requirement: CodeFree-O installation script
The system SHALL provide the installation script `npm install -g @srdcloud/codefree-o --registry=https://registry.npmjs.org/` for CodeFree-O in the environment detection section.

#### Scenario: CodeFree-O not installed - show install script
- **WHEN** CodeFree-O is not detected and user views environment detection
- **THEN** the system displays the installation command `npm install -g @srdcloud/codefree-o --registry=https://registry.npmjs.org/`

### Requirement: CodeFree-O version check in settings
The system SHALL include CodeFree-O in the "Settings > About > Local Environment Detection" section alongside other supported apps.

#### Scenario: Environment detection displays CodeFree
- **WHEN** user opens Settings > About > Local Environment Detection
- **THEN** CodeFree-O appears with its version status (installed/not installed, current version, upgrade available)
