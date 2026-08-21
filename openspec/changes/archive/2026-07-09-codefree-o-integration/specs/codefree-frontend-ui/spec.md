## ADDED Requirements

### Requirement: CodeFree-O AppType registration
The system SHALL register "codefree" as a valid AppType in the AppType enum. CodeFree-O SHALL be classified as an additive mode app (same group as OpenCode/OpenClaw/Hermes).

#### Scenario: AppType enum includes Codefree
- **WHEN** the AppType enum is iterated
- **THEN** "codefree" is a valid variant

#### Scenario: Codefree in additive mode group
- **WHEN** app mode classification is checked
- **THEN** Codefree is in the additive mode group

### Requirement: CodeFree-O AppSwitcher icon
The system SHALL display a teal-colored SVG icon (`</>` code symbol) for CodeFree-O in the AppSwitcher component.

#### Scenario: AppSwitcher displays CodeFree icon
- **WHEN** the AppSwitcher is rendered
- **THEN** CodeFree-O appears with its teal `</>` icon

### Requirement: CodeFree-O navigation behavior
The system SHALL show only the sessions navigation button when CodeFree-O is selected. The system SHALL NOT show providers, skills, prompts, or mcp navigation buttons for CodeFree-O.

#### Scenario: CodeFree-O selected navigation
- **WHEN** CodeFree-O is the active app
- **THEN** only the sessions (History icon) button is visible in the navigation bar

#### Scenario: CodeFree-O providers redirect
- **WHEN** CodeFree-O is selected and the current view is "providers"
- **THEN** the system automatically redirects to the "sessions" view

### Requirement: CodeFree-O hasProviderSupport is false
The system SHALL set `hasProviderSupport` to false when the active app is CodeFree-O. This hides the provider management UI (add provider button, ProxyToggle, FailoverToggle, ProfileSwitcher).

#### Scenario: Provider UI hidden for CodeFree
- **WHEN** CodeFree-O is the active app
- **THEN** provider management elements (add button, proxy toggle, failover toggle, profile switcher) are not displayed

### Requirement: CodeFree-O sessions view header
The system SHALL display a CC Switch logo and Settings button in the CodeFree-O sessions view header (similar to providers view layout), instead of the standard sessions header.

#### Scenario: CodeFree sessions view header
- **WHEN** CodeFree-O is selected and sessions view is active
- **THEN** the header shows CC Switch logo + Settings button + UpdateBadge

### Requirement: CodeFree-O excluded from homepage display settings
The system SHALL NOT include CodeFree-O in the "Settings > General > Homepage display" options. CodeFree-O SHALL still appear in the AppSwitcher for switching between apps.

#### Scenario: Homepage display settings
- **WHEN** user opens Settings > General > Homepage display
- **THEN** CodeFree-O is not listed as a selectable homepage app

#### Scenario: AppSwitcher still shows CodeFree
- **WHEN** user views the AppSwitcher
- **THEN** CodeFree-O is available for switching

### Requirement: CodeFree-O VisibleApps setting
The system SHALL include a `codefree: bool` field (default true) in the VisibleApps struct. The system SHALL include `current_provider_codefree: Option<String>` in settings.

#### Scenario: VisibleApps includes codefree
- **WHEN** settings are loaded
- **THEN** `visible_apps.codefree` is present with default value true

### Requirement: CodeFree-O i18n support
The system SHALL add "CodeFree" translations in all locale files (en, zh, zh-TW, ja).

#### Scenario: English locale
- **WHEN** locale is English
- **THEN** CodeFree-O is displayed as "CodeFree"

#### Scenario: Chinese locale
- **WHEN** locale is Chinese
- **THEN** CodeFree-O is displayed as "CodeFree"

### Requirement: Revert unnecessary modifications
The system SHALL revert git uncommitted changes that do not belong to the core CodeFree-O integration features. Specifically:
- `app_config.rs` McpRoot/PromptRoot codefree fields SHALL be reverted (CodeFree-O does not need MCP/Prompt root)
- `deeplink/provider.rs` Codefree branch SHALL be reverted (no deeplink support needed)
- `proxy/providers/mod.rs` Codefree proxy adapter SHALL be reverted (no proxy support needed)
- Other modifications not directly related to session viewing, token stats, skills, MCP config, or version check SHALL be evaluated and reverted if unnecessary

#### Scenario: Minimal code changes
- **WHEN** the final diff is reviewed
- **THEN** only changes directly supporting the 5 capabilities remain
