# squads-server verification report

Server: /private/tmp/squads-fork/squads-server (branch devel_vincent, commit below)
Verify time: 2026-08-14T17:53Z ~ 17:58Z (local machine time)

## Endpoints verified against the live Teams tenant (zw@webull.com)

| Requirement | Endpoint | Result |
|---|---|---|
| group list | GET /api/v1/groups | OK: 110 groups (9 teams + group chats) |
| group info | GET /api/v1/groups/{id} | OK: members + channels |
| group messages | GET /api/v1/groups/{id}/messages | OK: 200 messages w/ fromName+time |
| send in group | POST /api/v1/groups/{id}/messages | OK: sent, confirmed in thread |
| contact list | GET /api/v1/contacts | OK: 300 contacts (people+directory+members) |
| contact info | GET /api/v1/contacts/{id} | OK: Graph profile (name/upn/title/department) |
| contact messages | GET /api/v1/contacts/{id}/messages | OK: 200 1:1 messages |
| send to contact | POST /api/v1/contacts/{id}/messages | OK: sent, confirmed in 1:1 thread |

## Verification group: "low latency engine devops" (id 19:08fb87b4a3824dcaa659e442053d6825@thread.v2, 10 members)

- Group test message sent with the exact required text; confirmed present in thread (fromName: wen zhang).
- 1:1 test message sent to group member Yuxiao Yuan (8:orgid:57bc4823-...); confirmed present in 1:1 thread.
- No messages were sent to anyone outside this group and its members.

## Safety checks

- GET /api/v1/* without token -> 401
- GET /api/v1/* with wrong token -> 401
- POST to non-allowlisted group -> 403 "group is not in the send allowlist" (nothing sent)
- POST to non-member contact -> 403 "contact is not a member of an allowlisted group" (nothing sent)
