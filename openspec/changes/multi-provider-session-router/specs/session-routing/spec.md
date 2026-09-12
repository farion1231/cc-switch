## Purpose

Assigns independent API providers to each Claude Code terminal session, enabling true multi-task parallelism by routing requests based on the session identity carried in HTTP headers.

## ADDED Requirements

### Requirement: Session identity extraction

The proxy SHALL extract the session identity from incoming HTTP requests using the `X-Claude-Code-Session-Id` header as the primary source, falling back to `metadata.session_id` in the request body.

#### Scenario: Session identified by header
- **WHEN** a request arrives with `X-Claude-Code-Session-Id: abc-123`
- **THEN** the proxy SHALL use `abc-123` as the session identity

#### Scenario: Session identified by metadata
- **WHEN** a request arrives without the session header but with `metadata.session_id: def-456`
- **THEN** the proxy SHALL use `def-456` as the session identity

#### Scenario: No session identity available
- **WHEN** a request arrives with neither header nor metadata session ID
- **THEN** the proxy SHALL fall back to the default provider routing logic

### Requirement: Session-to-provider assignment

The proxy SHALL assign a provider to each new session. The same session SHALL always use the same provider (session consistency). The assignment SHALL be persisted in the database.

#### Scenario: New session assigned via Round-Robin
- **WHEN** a new session appears and the strategy is `round_robin`
- **THEN** the proxy SHALL assign the next provider in the failover queue in turn

#### Scenario: New session assigned via Least-Loaded
- **WHEN** a new session appears and the strategy is `least_loaded`
- **THEN** the proxy SHALL assign the provider with the fewest active sessions

#### Scenario: Existing session reuses same provider
- **WHEN** a subsequent request arrives with the same session ID
- **THEN** the proxy SHALL route to the previously assigned provider

### Requirement: Session-level failover

If the assigned provider is circuit-broken, the proxy SHALL attempt to failover to the next available provider for that session. The failover count SHALL be recorded.

#### Scenario: Provider circuit-broken
- **WHEN** the session's assigned provider is circuit-broken
- **THEN** the proxy SHALL failover to the next provider in the failover queue

#### Scenario: All providers circuit-broken
- **WHEN** all providers in the failover queue are circuit-broken
- **THEN** the proxy SHALL return an error and fall back to the default routing

### Requirement: Session lifecycle management

Sessions SHALL have a configurable TTL (time-to-live). Sessions inactive beyond the TTL SHALL be automatically cleaned up. The TTL SHALL default to 3600 seconds (1 hour).

#### Scenario: Session TTL expiry
- **WHEN** a session has been inactive for longer than the configured TTL
- **THEN** the proxy SHALL clean up the session route mapping

#### Scenario: Session re-activation within TTL
- **WHEN** a request arrives for an existing session within the TTL window
- **THEN** the proxy SHALL update the last-used timestamp and keep the route

### Requirement: Session routing management UI

The application SHALL provide a user interface to:
- Enable/disable session routing
- Configure the assignment strategy (Round-Robin or Least-Loaded)
- Configure the session TTL
- View active sessions and their assigned providers
- View provider load distribution
- Manually delete session routes
- Trigger cleanup of expired sessions

#### Scenario: Enable session routing
- **WHEN** the user toggles session routing on
- **THEN** subsequent new sessions SHALL be routed using the configured strategy

#### Scenario: View active sessions
- **WHEN** the user opens the session routing page
- **THEN** the UI SHALL display all active sessions with their provider, request count, and last activity time

#### Scenario: Cleanup expired sessions
- **WHEN** the user clicks "Cleanup Expired"
- **THEN** the proxy SHALL remove all sessions inactive beyond the TTL

### Requirement: Compatibility with existing features

Session routing SHALL be disabled by default. When disabled, the proxy SHALL behave exactly as before. Session routing SHALL be compatible with the existing circuit breaker and failover queue mechanisms.

#### Scenario: Disabled by default
- **WHEN** the change is applied
- **THEN** the session routing feature SHALL be disabled

#### Scenario: Existing behavior preserved
- **WHEN** session routing is disabled
- **THEN** all requests SHALL use the existing provider routing logic unchanged