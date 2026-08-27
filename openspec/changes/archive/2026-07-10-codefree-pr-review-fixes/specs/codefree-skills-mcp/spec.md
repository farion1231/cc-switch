## MODIFIED Requirements

### Requirement: VisibleApps deserialization compatibility
When deserializing VisibleApps from persisted settings where the `codefree` field is absent (e.g., settings saved before this update), the system SHALL use the struct's default value (`true`) instead of the serde bool default (`false`). This ensures existing users see CodeFree in AppSwitcher after upgrading.

#### Scenario: Old settings without codefree field
- **WHEN** persisted settings do not contain the `codefree` field in VisibleApps
- **THEN** the deserialized `codefree` value SHALL be `true` (from VisibleApps::default())

#### Scenario: New settings with codefree field
- **WHEN** persisted settings contain `codefree: false` in VisibleApps
- **THEN** the deserialized `codefree` value SHALL be `false` (respecting user choice)

### Requirement: CodeFree skills default directory
The system SHALL use `~/.codefree-o/.config/skills` as the default CodeFree skills directory, not `~/.codefree-o/skills`.

#### Scenario: Default skills path
- **WHEN** no custom skills override directory is configured
- **THEN** CodeFree skills SHALL be loaded from `~/.codefree-o/.config/skills`

#### Scenario: Custom skills override directory
- **WHEN** a custom skills override directory is configured
- **THEN** CodeFree skills SHALL be loaded from the custom directory
