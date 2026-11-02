# Support Desk Integration Requirements

**Purpose**: Connect ticketing system API for support backlog monitoring.

## Required Credentials

- `SUPPORT_DESK_API_KEY` - API key from support desk provider
- `SUPPORT_DESK_API_SECRET` - API secret (if applicable)
- `SUPPORT_DESK_DOMAIN` - Your support desk domain (e.g., freshdesk.com)

## API Endpoints

- `GET /v1/tickets` - List open tickets
- `GET /v1/tickets/stats` - Summary statistics (open, resolved, etc.)
- `GET /v1/agents` - List support agents

## Success Criteria

- [ ] API returns ticket data without authentication errors
- [ ] Ticket feed updates at least hourly
- [ ] Support backlog count is visible in OpenClaw heartbeat `supportBacklog` check
- [ ] Average response time metrics are available

## Dependencies

- CTO to configure API credentials in gateway secret store
- Steve to approve credential sharing with OpenClaw