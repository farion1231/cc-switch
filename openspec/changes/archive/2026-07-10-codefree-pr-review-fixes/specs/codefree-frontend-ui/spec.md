## MODIFIED Requirements

### Requirement: App type validation for initial app selection
The system SHALL include "codefree" in the set of valid application identifiers used to validate the last-used app on startup. When a user selects CodeFree and reloads the application, the system SHALL restore CodeFree as the active app instead of falling back to the default.

#### Scenario: Restore CodeFree after reload
- **WHEN** user selects CodeFree in AppSwitcher and reloads the application
- **THEN** the system SHALL restore CodeFree as the active app

#### Scenario: CodeFree not in valid apps
- **WHEN** "codefree" is missing from VALID_APPS
- **THEN** the saved "codefree" value SHALL NOT be discarded on reload
