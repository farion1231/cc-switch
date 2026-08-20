## MODIFIED Requirements

### Requirement: CodeFree-O AppType registration
The system SHALL register "codefree" as a valid AppType in the AppType enum. CodeFree-O SHALL be classified as an additive mode app (same group as OpenCode/OpenClaw/Hermes). After merging upstream changes, the AppType enum SHALL include both "codefree" and "grokbuild".

#### Scenario: AppType enum includes Codefree after merge
- **WHEN** the AppType enum is iterated after upstream merge
- **THEN** "codefree" is a valid variant alongside "grokbuild"

#### Scenario: Codefree in additive mode group after merge
- **WHEN** app mode classification is checked after upstream merge
- **THEN** Codefree is in the additive mode group

### Requirement: CodeFree-O i18n support
The system SHALL add "CodeFree" translations in all locale files (en, zh, zh-TW, ja). After merging upstream changes, all locale files SHALL contain both codefree-related keys and upstream-new keys.

#### Scenario: All locale files contain codefree keys after merge
- **WHEN** locale files are loaded after upstream merge
- **THEN** codefree-related translation keys are present in en, zh, zh-TW, ja

#### Scenario: All locale files contain upstream-new keys after merge
- **WHEN** locale files are loaded after upstream merge
- **THEN** upstream-new translation keys (presets, pricing, etc.) are present in en, zh, zh-TW, ja

### Requirement: CodeFree-O VisibleApps setting
The system SHALL include a `codefree: bool` field (default true) in the VisibleApps struct. The system SHALL include `current_provider_codefree: Option<String>` in settings. After merging upstream changes, these fields SHALL remain in the struct.

#### Scenario: VisibleApps includes codefree after merge
- **WHEN** settings are loaded after upstream merge
- **THEN** `visible_apps.codefree` is present with default value true
