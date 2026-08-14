# squads-server

REST server built on top of [Squads](https://github.com/zhangw/Squads) (branch `devel_vincent`).
It reuses Squads' Teams authentication (device-code flow with the official Teams client id)
and exposes the Teams APIs behind a token-authenticated REST interface, so any client
(CLI, scripts, bots, other services) can read and send Teams messages without a GUI.

## Features

- `GET  /api/v1/groups`                    - group list (teams + group chats)
- `GET  /api/v1/groups/{id}`               - group info (members / channels)
- `GET  /api/v1/groups/{id}/messages`      - group messages (paged, `?pageSize=&before=`)
- `POST /api/v1/groups/{id}/messages`      - send a message to a group chat `{"text": "..."}`
- `GET  /api/v1/contacts`                  - contact list (people + directory + chat members)
- `GET  /api/v1/contacts/{id}`             - contact info (MRIs via short profile, AAD ids via Graph)
- `GET  /api/v1/contacts/{id}/messages`    - 1:1 messages with a contact
- `POST /api/v1/contacts/{id}/messages`    - send a 1:1 message to a contact `{"text": "..."}`
- `GET  /healthz`                          - health check (no auth)

All `/api/v1` routes require `Authorization: Bearer <api-token>`.

## Safety: send allowlist

Message sending is gated by an allowlist of group names (`SQUADS_ALLOWED_GROUPS`):

- `POST /groups/{id}/messages` is only accepted for chats whose title is in the allowlist.
- `POST /contacts/{id}/messages` is only accepted for contacts who are members of an allowlisted group.
- Everything else returns `403` and sends nothing.

Default allowlist: `low latency engine devops`.

## Setup

```bash
# 1. one-time: obtain a Teams refresh token via Squads device-code flow
#    (run the Squads GUI once and log in, or use the device-code script in
#    /private/tmp/squads-auth). The token store defaults to
#    SQUADS_TOKEN_STORE=/private/tmp/squads-server-tokens.json

# 2. run the server
export SQUADS_API_TOKENS="<your-api-token>"
export SQUADS_ALLOWED_GROUPS="low latency engine devops"
export SQUADS_TOKEN_FILE="/private/tmp/squads-auth/tokens.json"  # {refresh_token: "..."}
cargo run

# 3. use it
curl -H "Authorization: Bearer <your-api-token>" http://127.0.0.1:8787/api/v1/groups
curl -H "Authorization: Bearer <your-api-token>" \
     -H "Content-Type: application/json" \
     -d '{"text":"hello"}' http://127.0.0.1:8787/api/v1/groups/<group-id>/messages
```

## Environment variables

| Variable | Default | Meaning |
|---|---|---|
| `SQUADS_BIND` | `127.0.0.1:8787` | listen address |
| `SQUADS_API_TOKENS` | - | comma-separated client API tokens (required) |
| `SQUADS_ALLOWED_GROUPS` | `low latency engine devops` | group names allowed to receive messages |
| `SQUADS_REFRESH_TOKEN` | - | Teams refresh token (alternative to the file) |
| `SQUADS_TOKEN_STORE` | `/private/tmp/squads-server-tokens.json` | where the refresh token is read/persisted |
