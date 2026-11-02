# FIO Bank Integration Requirements

**Purpose**: Configure read-only API connection to FIO bank for cashflow anomaly detection.

## Required Credentials

- `FIO_API_KEY` - API key from FIO bank partner portal
- `FIO_API_SECRET` - API secret (if applicable)
- `FIO_REDIRECT_URI` - OAuth redirect URI for token refresh

## API Endpoints

- `GET /v1/accounts` - List account balances
- `GET /v1/transactions` - List recent transactions
- `GET /v1/cashflow/forecast` - Cashflow forecast (if available)

## Success Criteria

- [ ] API returns account data without authentication errors
- [ ] Transactions feed updates at least daily
- [ ] Cashflow anomalies are detectable
- [ ] Data is visible in OpenClaw heartbeat `financeSignal` check

## Dependencies

- CTO to configure API credentials in gateway secret store
- Steve to approve credential sharing with OpenClaw