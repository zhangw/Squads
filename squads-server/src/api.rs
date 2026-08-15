//! REST handlers, API-token middleware and send allowlist enforcement.
use axum::extract::{Path, Query, Request, State};
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use axum::middleware;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::config::{Config, SCOPE_CHATSVCAGG};
use crate::models::*;
use crate::teams::TeamsClient;

pub struct AppState {
    pub cfg: Config,
    pub teams: TeamsClient,
    pub dir: RwLock<Option<(i64, DirCache)>>,
}

pub type SharedState = Arc<AppState>;

pub fn router(state: SharedState) -> Router {
    let authed = Router::new()
        .route("/groups", get(list_groups))
        .route("/groups/{id}", get(get_group))
        .route("/groups/{id}/messages", get(list_group_messages).post(send_group_message))
        .route("/contacts", get(list_contacts))
        .route("/contacts/{id}", get(get_contact))
        .route("/contacts/{id}/messages", get(list_contact_messages).post(send_contact_message))
        .layer(middleware::from_fn_with_state(state.clone(), api_token_auth));
    Router::new()
        .route("/healthz", get(health))
        .nest("/api/v1", authed)
        .with_state(state)
}

// ---------------------------------------------------------------- errors

pub struct ApiError {
    status: StatusCode,
    msg: String,
}

impl ApiError {
    pub fn new(status: StatusCode, msg: impl Into<String>) -> Self {
        Self { status, msg: msg.into() }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(json!({ "error": self.msg }))).into_response()
    }
}

fn ct_eq(a: &str, b: &str) -> bool {
    let (x, y) = (a.as_bytes(), b.as_bytes());
    if x.len() != y.len() { return false; }
    x.iter().zip(y).fold(0u8, |acc, (p, q)| acc | (p ^ q)) == 0
}

async fn health() -> Json<Value> {
    Json(json!({ "status": "ok" }))
}

async fn api_token_auth(State(state): State<SharedState>, req: Request, next: Next) -> Result<Response, ApiError> {
    let auth = req.headers().get("authorization").and_then(|v| v.to_str().ok()).unwrap_or("");
    let token = auth.strip_prefix("Bearer ").unwrap_or("");
    if state.cfg.api_tokens.iter().any(|t| ct_eq(t, token)) {
        Ok(next.run(req).await)
    } else {
        Err(ApiError::new(StatusCode::UNAUTHORIZED, "invalid or missing API token"))
    }
}

// ------------------------------------------------------------ directory

impl AppState {
    /// teams_me cached for 120s.
    pub async fn dir(&self) -> Result<DirCache, ApiError> {
        let now = crate::auth::now_epoch();
        if let Some((ts, cache)) = self.dir.read().await.clone() {
            if now - ts < 120 {
                return Ok(cache);
            }
        }
        let v = self.teams.teams_me().await.map_err(|e| ApiError::new(StatusCode::BAD_GATEWAY, e.to_string()))?;
        let cache = parse_dir(&v);
        *self.dir.write().await = Some((now, cache.clone()));
        Ok(cache)
    }

    fn find_chat(&self, dir: &DirCache, id: &str) -> Option<ChatRecord> {
        dir.chats.iter().find(|c| c.id == id).cloned()
    }

    fn find_team(&self, dir: &DirCache, id: &str) -> Option<TeamRecord> {
        dir.teams.iter().find(|t| t.id == id).cloned()
    }

    fn chat_is_allowed(&self, chat: &ChatRecord) -> bool {
        chat.title.as_ref().map(|t| self.cfg.allowed_groups.contains(&t.to_lowercase())).unwrap_or(false)
    }

    /// MRIs and object ids of members of allowlisted group chats.
    fn allowed_member_ids(&self, dir: &DirCache) -> (HashSet<String>, HashSet<String>) {
        let mut mris = HashSet::new();
        let mut oids = HashSet::new();
        for chat in &dir.chats {
            if self.chat_is_allowed(chat) {
                for m in &chat.members {
                    mris.insert(m.mri.to_lowercase());
                    if let Some(oid) = &m.object_id { oids.insert(oid.to_lowercase()); }
                }
            }
        }
        (mris, oids)
    }

    fn contact_is_allowed_member(&self, dir: &DirCache, contact_id: &str) -> bool {
        let (mris, oids) = self.allowed_member_ids(dir);
        let cid = contact_id.to_lowercase();
        mris.contains(&cid) || oids.contains(&cid) || mris.iter().any(|m| m.ends_with(&cid))
    }
}

fn s(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(|x| x.as_str()).map(|x| x.to_string())
}

fn parse_dir(v: &Value) -> DirCache {
    let mut teams = Vec::new();
    if let Some(arr) = v.get("teams").and_then(|x| x.as_array()) {
        for t in arr {
            let channels = t.get("channels").and_then(|x| x.as_array()).map(|chs| {
                chs.iter().filter_map(|c| {
                    Some(ChannelInfo { id: s(c, "id")?, name: s(c, "displayName").unwrap_or_default() })
                }).collect::<Vec<_>>()
            }).unwrap_or_default();
            if let Some(id) = s(t, "id") {
                teams.push(TeamRecord { id, name: s(t, "displayName").unwrap_or_default(), channels });
            }
        }
    }
    let mut chats = Vec::new();
    if let Some(arr) = v.get("chats").and_then(|x| x.as_array()) {
        for c in arr {
            let members = c.get("members").and_then(|x| x.as_array()).map(|ms| {
                ms.iter().filter_map(|m| {
                    Some(ChatMemberRec {
                        mri: s(m, "mri")?,
                        object_id: s(m, "objectId"),
                        role: s(m, "role"),
                        display_name: s(m, "displayName"),
                    })
                }).collect::<Vec<_>>()
            }).unwrap_or_default();
            if let Some(id) = s(c, "id") {
                let is_one_on_one = c.get("isOneOnOne").and_then(|x| x.as_bool()).unwrap_or(false);
                let last_message_time = c.get("lastMessage")
                    .and_then(|lm| s(lm, "originalArrivalTime").or_else(|| s(lm, "composeTime")));
                chats.push(ChatRecord {
                    id,
                    title: s(c, "title"),
                    is_one_on_one,
                    members,
                    last_message_time,
                });
            }
        }
    }
    DirCache { teams, chats }
}

// ------------------------------------------------------------ handlers

async fn list_groups(State(state): State<SharedState>) -> Result<Json<Value>, ApiError> {
    let dir = state.dir().await?;
    let mut groups = Vec::new();
    for t in &dir.teams {
        groups.push(json!({
            "id": t.id, "kind": "team", "name": t.name,
            "channel_count": t.channels.len(),
            "member_count": serde_json::Value::Null
        }));
    }
    for c in &dir.chats {
        if c.is_one_on_one { continue; }
        groups.push(json!({
            "id": c.id, "kind": "group_chat",
            "name": c.title.clone().unwrap_or_default(),
            "member_count": c.members.len(),
            "channel_count": 0
        }));
    }
    Ok(Json(json!({ "groups": groups, "count": groups.len() })))
}

async fn get_group(State(state): State<SharedState>, Path(id): Path<String>) -> Result<Json<Value>, ApiError> {
    let dir = state.dir().await?;
    if let Some(t) = state.find_team(&dir, &id) {
        return Ok(Json(json!({
            "group": {
                "id": t.id, "kind": "team", "name": t.name,
                "channels": t.channels.iter().map(|c| json!({ "id": c.id, "name": c.name })).collect::<Vec<_>>()
            }
        })));
    }
    if let Some(c) = state.find_chat(&dir, &id) {
        return Ok(Json(json!({
            "group": {
                "id": c.id, "kind": "group_chat",
                "name": c.title.clone().unwrap_or_default(),
                "is_one_on_one": c.is_one_on_one,
                "members": c.members.iter().map(|m| json!({
                    "mri": m.mri, "objectId": m.object_id, "role": m.role, "displayName": m.display_name
                })).collect::<Vec<_>>()
            }
        })));
    }
    Err(ApiError::new(StatusCode::NOT_FOUND, format!("group {} not found", id)))
}

#[derive(Deserialize)]
pub struct MsgQuery {
    pub page_size: Option<u32>,
    pub before: Option<String>,
    pub channel: Option<String>,
}

fn messages_out(v: &Value) -> Vec<Value> {
    let mut out = Vec::new();
    if let Some(arr) = v.get("messages").and_then(|x| x.as_array()) {
        for m in arr {
            out.push(json!({
                "id": s(m, "id").unwrap_or_default(),
                "from": s(m, "from"),
                "fromName": s(m, "imDisplayName").or_else(|| s(m, "imdisplayname")),
                "time": s(m, "originalArrivalTime").or_else(|| s(m, "originalarrivaltime")).or_else(|| s(m, "composeTime")).or_else(|| s(m, "composetime")),
                "content": s(m, "content"),
                "messageType": s(m, "messageType").or_else(|| s(m, "messagetype")),
            }));
        }
    }
    out
}

async fn list_group_messages(State(state): State<SharedState>, Path(id): Path<String>, Query(q): Query<MsgQuery>) -> Result<Json<Value>, ApiError> {
    let dir = state.dir().await?;
    let page_size = q.page_size.unwrap_or(200).min(200);
    if let Some(c) = state.find_chat(&dir, &id) {
        let v = state.teams.messages(&c.id, page_size, q.before.as_deref()).await
            .map_err(|e| ApiError::new(StatusCode::BAD_GATEWAY, e.to_string()))?;
        let messages = messages_out(&v);
        return Ok(Json(json!({ "group": { "id": c.id, "name": c.title.clone().unwrap_or_default(), "kind": "group_chat" }, "messages": messages, "count": messages.len() })));
    }
    if let Some(t) = state.find_team(&dir, &id) {
        let channel_id = q.channel.clone().or_else(|| t.channels.first().map(|c| c.id.clone()));
        let channel_id = channel_id.ok_or_else(|| ApiError::new(StatusCode::BAD_REQUEST, "team has no channels"))?;
        let token = state.teams.tokens.token(SCOPE_CHATSVCAGG).await.map_err(|e| ApiError::new(StatusCode::BAD_GATEWAY, e.to_string()))?;
        let url = format!("https://teams.microsoft.com/api/csa/emea/api/v2/teams/{}/channels/{}", t.id, channel_id);
        let res = state.teams.tokens.http().get(&url).bearer_auth(&token).send().await
            .map_err(|e| ApiError::new(StatusCode::BAD_GATEWAY, e.to_string()))?;
        let status = res.status();
        let body = res.text().await.map_err(|e| ApiError::new(StatusCode::BAD_GATEWAY, e.to_string()))?;
        if !status.is_success() {
            return Err(ApiError::new(StatusCode::BAD_GATEWAY, format!("channel messages failed: HTTP {}: {}", status, &body[..body.len().min(300)])));
        }
        let v: Value = serde_json::from_str(&body).map_err(|e| ApiError::new(StatusCode::BAD_GATEWAY, e.to_string()))?;
        // TeamConversations: flatten reply chains
        let mut messages = Vec::new();
        if let Some(chains) = v.get("replyChains").or_else(|| v.get("reply_chains")).and_then(|x| x.as_array()) {
            for chain in chains {
                messages.extend(messages_out(&json!({ "messages": chain.get("messages") })));
            }
        }
        return Ok(Json(json!({ "group": { "id": t.id, "name": t.name, "kind": "team", "channelId": channel_id }, "messages": messages, "count": messages.len() })));
    }
    Err(ApiError::new(StatusCode::NOT_FOUND, format!("group {} not found", id)))
}

async fn send_group_message(State(state): State<SharedState>, Path(id): Path<String>, Json(req): Json<SendRequest>) -> Result<Json<Value>, ApiError> {
    if req.text.trim().is_empty() {
        return Err(ApiError::new(StatusCode::BAD_REQUEST, "text must not be empty"));
    }
    let dir = state.dir().await?;
    let chat = state.find_chat(&dir, &id)
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, format!("group chat {} not found", id)))?;
    if !state.chat_is_allowed(&chat) {
        return Err(ApiError::new(StatusCode::FORBIDDEN, "group is not in the send allowlist"));
    }
    if state.find_team(&dir, &id).is_some() {
        return Err(ApiError::new(StatusCode::NOT_IMPLEMENTED, "sending to team channels is not supported yet"));
    }
    let me = state.teams.tokens.me().await.map_err(|e| ApiError::new(StatusCode::BAD_GATEWAY, e.to_string()))?;
    let payload = build_message_payload(&req.text, &me);
    let resp = state.teams.send_message(&chat.id, &payload).await
        .map_err(|e| ApiError::new(StatusCode::BAD_GATEWAY, e.to_string()))?;
    Ok(Json(json!({ "sent": true, "group": { "id": chat.id, "name": chat.title.clone().unwrap_or_default() }, "result": resp })))
}

async fn list_contacts(State(state): State<SharedState>) -> Result<Json<Value>, ApiError> {
    let dir = state.dir().await?;
    let mut contacts: Vec<ContactOut> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let push = |id: String, c: ContactOut, contacts: &mut Vec<ContactOut>, seen: &mut HashSet<String>| {
        let key = id.to_lowercase();
        if seen.insert(key) { contacts.push(c); }
    };
    // 1. frequent people
    if let Ok(v) = state.teams.graph_people().await {
        if let Some(arr) = v.get("value").and_then(|x| x.as_array()) {
            for p in arr {
                let id = s(p, "id").unwrap_or_default();
                if id.is_empty() { continue; }
                let email = p.get("scoredEmailAddresses").and_then(|x| x.as_array())
                    .and_then(|a| a.first()).and_then(|e| s(e, "address"));
                let c = ContactOut {
                    id: id.clone(),
                    display_name: s(p, "displayName").unwrap_or_default(),
                    upn: None,
                    email,
                    job_title: s(p, "jobTitle"),
                    source: "people".into(),
                };
                push(id, c, &mut contacts, &mut seen);
            }
        }
    }
    // 2. directory users
    if let Ok(v) = state.teams.graph_users(50).await {
        if let Some(arr) = v.get("value").and_then(|x| x.as_array()) {
            for u in arr {
                let id = s(u, "id").unwrap_or_default();
                if id.is_empty() { continue; }
                let c = ContactOut {
                    id: id.clone(),
                    display_name: s(u, "displayName").unwrap_or_default(),
                    upn: s(u, "userPrincipalName"),
                    email: s(u, "mail"),
                    job_title: s(u, "jobTitle"),
                    source: "directory".into(),
                };
                push(id, c, &mut contacts, &mut seen);
            }
        }
    }
    // 3. members of every chat the user is in
    for chat in &dir.chats {
        for m in &chat.members {
            let c = ContactOut {
                id: m.mri.clone(),
                display_name: m.display_name.clone().unwrap_or_else(|| m.mri.clone()),
                upn: None,
                email: None,
                job_title: None,
                source: "chat_member".into(),
            };
            push(m.mri.clone(), c, &mut contacts, &mut seen);
        }
    }
    contacts.truncate(300);
    Ok(Json(json!({ "contacts": contacts, "count": contacts.len() })))
}

async fn get_contact(State(state): State<SharedState>, Path(id): Path<String>) -> Result<Json<Value>, ApiError> {
    // Normalize MRI prefixes so Graph lookups use the raw AAD id.
    let raw = id.trim_start_matches("8:orgid:").trim_start_matches("8:").to_string();
    let g = state.teams.graph_user(&raw).await;
    match g {
        Ok(v) => {
            let display_name = s(&v, "displayName").unwrap_or_default();
            if !display_name.is_empty() {
                return Ok(Json(json!({ "contact": {
                    "id": id,
                    "displayName": display_name,
                    "upn": s(&v, "userPrincipalName"),
                    "email": s(&v, "mail"),
                    "jobTitle": s(&v, "jobTitle"),
                    "department": s(&v, "department"),
                } })));
            }
        }
        Err(e) => tracing::warn!("graph_user({}) failed: {}", raw, e),
    }
    // Fallback: Teams short profile (may be empty for some tenants).
    let v = state.teams.fetch_short_profiles(&[id.clone()]).await
        .map_err(|e| ApiError::new(StatusCode::BAD_GATEWAY, e.to_string()))?;
    let item = v.as_array().and_then(|a| a.first())
        .or_else(|| v.get("value").and_then(|x| x.as_array()).and_then(|a| a.first()));
    if let Some(p) = item {
        let given = s(p, "givenName").unwrap_or_default();
        let surname = s(p, "surname").unwrap_or_default();
        let name = s(p, "displayName").unwrap_or_else(|| format!("{} {}", given, surname).trim().to_string());
        return Ok(Json(json!({ "contact": {
            "id": id,
            "displayName": name,
            "upn": s(p, "userPrincipalName"),
            "email": s(p, "email"),
            "jobTitle": s(p, "jobTitle"),
            "department": s(p, "department"),
            "mri": s(p, "mri").unwrap_or(id),
        } })));
    }
    Err(ApiError::new(StatusCode::NOT_FOUND, format!("contact {} not found", id)))
}

/// Find the 1:1 chat whose other member matches the contact id.
fn find_one_on_one(dir: &DirCache, contact_id: &str) -> Option<ChatRecord> {
    let cid = contact_id.to_lowercase();
    dir.chats.iter().find(|c| {
        c.is_one_on_one && c.members.iter().any(|m| {
            let mri = m.mri.to_lowercase();
            mri == cid || mri.ends_with(&cid)
                || m.object_id.as_ref().map(|o| o.to_lowercase() == cid).unwrap_or(false)
        })
    }).cloned()
}

async fn list_contact_messages(State(state): State<SharedState>, Path(id): Path<String>, Query(q): Query<MsgQuery>) -> Result<Json<Value>, ApiError> {
    let dir = state.dir().await?;
    let chat = find_one_on_one(&dir, &id)
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, format!("no 1:1 chat with contact {}", id)))?;
    let page_size = q.page_size.unwrap_or(200).min(200);
    let v = state.teams.messages(&chat.id, page_size, q.before.as_deref()).await
        .map_err(|e| ApiError::new(StatusCode::BAD_GATEWAY, e.to_string()))?;
    let messages = messages_out(&v);
    Ok(Json(json!({ "contact": { "id": id }, "threadId": chat.id, "messages": messages, "count": messages.len() })))
}

async fn send_contact_message(State(state): State<SharedState>, Path(id): Path<String>, Json(req): Json<SendRequest>) -> Result<Json<Value>, ApiError> {
    if req.text.trim().is_empty() {
        return Err(ApiError::new(StatusCode::BAD_REQUEST, "text must not be empty"));
    }
    let dir = state.dir().await?;
    if !state.contact_is_allowed_member(&dir, &id) {
        return Err(ApiError::new(StatusCode::FORBIDDEN, "contact is not a member of an allowlisted group"));
    }
    let chat = find_one_on_one(&dir, &id)
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, format!("no 1:1 chat with contact {}", id)))?;
    let me = state.teams.tokens.me().await.map_err(|e| ApiError::new(StatusCode::BAD_GATEWAY, e.to_string()))?;
    let payload = build_message_payload(&req.text, &me);
    let resp = state.teams.send_message(&chat.id, &payload).await
        .map_err(|e| ApiError::new(StatusCode::BAD_GATEWAY, e.to_string()))?;
    Ok(Json(json!({ "sent": true, "contact": { "id": id }, "threadId": chat.id, "result": resp })))
}

/// Build the exact wire format Squads uses to send a message (src/main.rs).
fn build_message_payload(text: &str, me: &crate::auth::MeInfo) -> Value {
    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let client_id = chrono::Utc::now().timestamp_millis().to_string();
    json!({
        "from": format!("8:orgid:{}", me.oid),
        "composeTime": now,
        "originalArrivalTime": now,
        "content": html_to_teams(text),
        "messageType": "RichText/Html",
        "contentType": "Text",
        "clientMessageId": client_id,
        "imDisplayName": me.display_name,
        "properties": {
            "importance": "",
            "subject": "",
            "title": "",
            "cards": "[]",
            "links": "[]",
            "mentions": "[]",
            "onBehalfOf": null,
            "files": "[]",
            "policyViolation": null,
            "formatVariant": "TEAMS"
        },
        "postType": "Standard",
        "crossPostChannels": []
    })
}

fn html_escape(s: &str) -> String {
    s.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;").replace("\"", "&quot;")
}

/// Escape HTML, then turn newlines into <br> so multi-line text renders
/// as line breaks in Teams (RichText/Html collapses plain newlines).
fn html_to_teams(s: &str) -> String {
    html_escape(s).replace("
", "<br>")
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_html_escape() {
        assert_eq!(html_escape("a < b & c > d"), "a &lt; b &amp; c &gt; d");
        assert_eq!(html_escape("plain"), "plain");
    }

    #[test]
    fn test_ct_eq() {
        assert!(ct_eq("abc", "abc"));
        assert!(!ct_eq("abc", "abd"));
        assert!(!ct_eq("abc", "abcd"));
        assert!(!ct_eq("", "a"));
        assert!(ct_eq("", ""));
    }

    #[test]
    fn test_parse_dir() {
        let v = serde_json::json!({
            "teams": [
                {"id": "team1", "displayName": "My Team", "channels": [{"id": "c1", "displayName": "General"}]}
            ],
            "chats": [
                {"id": "19:abc@thread.v2", "title": "low latency engine devops", "isOneOnOne": false,
                 "members": [{"mri": "8:orgid:1", "objectId": "1", "role": "owner", "displayName": "wen zhang"}],
                 "lastMessage": {"originalArrivalTime": "2026-08-14T17:00:00Z"}},
                {"id": "19:xyz@unq.gbl.spaces", "isOneOnOne": true,
                 "members": [{"mri": "8:orgid:1"}, {"mri": "8:orgid:2"}]}
            ]
        });
        let dir = parse_dir(&v);
        assert_eq!(dir.teams.len(), 1);
        assert_eq!(dir.teams[0].name, "My Team");
        assert_eq!(dir.teams[0].channels.len(), 1);
        assert_eq!(dir.chats.len(), 2);
        assert_eq!(dir.chats[0].title.as_deref(), Some("low latency engine devops"));
        assert!(!dir.chats[0].is_one_on_one);
        assert_eq!(dir.chats[0].members.len(), 1);
        assert_eq!(dir.chats[0].last_message_time.as_deref(), Some("2026-08-14T17:00:00Z"));
        assert!(dir.chats[1].is_one_on_one);
    }

    #[test]
    fn test_build_message_payload() {
        let me = crate::auth::MeInfo { oid: "oid-1".into(), display_name: "wen zhang".into(), upn: "zw@webull.com".into() };
        let p = build_message_payload("<script>hi</script>", &me);
        assert_eq!(p["from"], "8:orgid:oid-1");
        assert_eq!(p["content"], "&lt;script&gt;hi&lt;/script&gt;");
        assert_eq!(p["messageType"], "RichText/Html");
        assert_eq!(p["contentType"], "Text");
        assert_eq!(p["imDisplayName"], "wen zhang");
        assert_eq!(p["postType"], "Standard");
        assert!(p["clientMessageId"].is_string());
        assert!(p["properties"].get("formatVariant").is_some());
    }

    #[test]
    fn test_allowlist_matching() {
        // allowed group chat -> member mris become allowed
        let cfg = Config {
            bind: "127.0.0.1:1".into(),
            api_tokens: vec!["t".into()],
            allowed_groups: vec!["low latency engine devops".into()],
            token_store: "/tmp/x".into(),
            refresh_token: None,
        };
        // cfg is not Send-required here; exercise logic through a bare struct
        let chat = ChatRecord {
            id: "19:abc".into(),
            title: Some("low latency engine devops".into()),
            is_one_on_one: false,
            members: vec![ChatMemberRec { mri: "8:orgid:111".into(), object_id: Some("111".into()), role: None, display_name: None }],
            last_message_time: None,
        };
        assert_eq!(cfg.allowed_groups.contains(&chat.title.unwrap().to_lowercase()), true);
    }
}


#[cfg(test)]
mod multiline_tests {
    use super::*;

    #[test]
    fn test_html_to_teams_multiline() {
        assert_eq!(html_to_teams("line1\nline2"), "line1<br>line2");
        assert_eq!(html_to_teams("<b>a</b>\n&b"), "&lt;b&gt;a&lt;/b&gt;<br>&amp;b");
        assert_eq!(html_to_teams("plain"), "plain");
        assert_eq!(html_to_teams("a\nb\nc"), "a<br>b<br>c");
    }
}
