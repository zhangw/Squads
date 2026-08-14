//! Teams / Graph HTTP calls, ported from Squads src/api.rs.
use anyhow::{anyhow, bail, Result};
use crate::auth::TokenManager;
use crate::config::{SCOPE_CHATSVCAGG, SCOPE_GRAPH, SCOPE_IC3};

pub struct TeamsClient {
    pub tokens: TokenManager,
}

impl TeamsClient {
    /// Squads teams_me: list of teams + chats.
    pub async fn teams_me(&self) -> Result<serde_json::Value> {
        let token = self.tokens.token(SCOPE_CHATSVCAGG).await?;
        let res = self
            .tokens
            .http()
            .get("https://teams.microsoft.com/api/csa/emea/api/v2/teams/users/me")
            .bearer_auth(&token)
            .query(&[("isPrefetch", "false"), ("enableMembershipSummary", "true"), ("enableRC2Fetch", "false")])
            .send()
            .await?;
        let status = res.status();
        let body = res.text().await?;
        if !status.is_success() {
            bail!("teams_me failed: HTTP {}: {}", status, &body[..body.len().min(300)]);
        }
        Ok(serde_json::from_str(&body)?)
    }

    /// Squads conversations: messages of a thread, optionally paging before a message id.
    pub async fn messages(&self, thread_id: &str, page_size: u32, before: Option<&str>) -> Result<serde_json::Value> {
        let token = self.tokens.token(SCOPE_IC3).await?;
        let thread_part = match before {
            Some(msg_id) => format!("{};messageid={}", thread_id, msg_id),
            None => thread_id.to_string(),
        };
        let url = format!(
            "https://teams.microsoft.com/api/chatsvc/emea/v1/users/ME/conversations/{}/messages",
            thread_part
        );
        let ps = page_size.min(200).to_string();
        let res = self
            .tokens
            .http()
            .get(&url)
            .bearer_auth(&token)
            .query(&[("pageSize", ps.as_str())])
            .send()
            .await?;
        let status = res.status();
        let body = res.text().await?;
        if !status.is_success() {
            bail!("messages failed: HTTP {}: {}", status, &body[..body.len().min(300)]);
        }
        Ok(serde_json::from_str(&body)?)
    }

    /// Squads send_message: POST a message JSON to a conversation thread.
    pub async fn send_message(&self, thread_id: &str, payload: &serde_json::Value) -> Result<serde_json::Value> {
        let token = self.tokens.token(SCOPE_IC3).await?;
        let url = format!(
            "https://teams.microsoft.com/api/chatsvc/emea/v1/users/ME/conversations/{}/messages",
            thread_id
        );
        let res = self
            .tokens
            .http()
            .post(&url)
            .bearer_auth(&token)
            .json(payload)
            .send()
            .await?;
        let status = res.status();
        let body = res.text().await?;
        if !status.is_success() {
            bail!("send_message failed: HTTP {}: {}", status, &body[..body.len().min(500)]);
        }
        match serde_json::from_str(&body) {
            Ok(v) => Ok(v),
            Err(_) => Ok(serde_json::json!({ "raw": body })),
        }
    }

    /// Squads fetch_short_profile: profile info for a list of user ids (MRIs / oids / upns).
    pub async fn fetch_short_profiles(&self, user_ids: &[String]) -> Result<serde_json::Value> {
        if user_ids.is_empty() {
            return Ok(serde_json::json!([]));
        }
        let token = self.tokens.token(SCOPE_CHATSVCAGG).await?;
        let body = format!("[\"{}\"]", user_ids.join("\",\""));
        let res = self
            .tokens
            .http()
            .post("https://teams.microsoft.com/api/mt/part/emea-02/beta/users/fetchShortProfile")
            .bearer_auth(&token)
            .header("content-type", "application/json;charset=UTF-8")
            .query(&[
                ("isMailAddress", "false"),
                ("enableGuest", "true"),
                ("skypeTeamsInfo", "true"),
                ("canBeSmtpAddress", "false"),
                ("includeIBBarredUsers", "true"),
                ("includeDisabledAccounts", "true"),
                ("useSkypeNameIfMissing", "false"),
            ])
            .body(body)
            .send()
            .await?;
        let status = res.status();
        let text = res.text().await?;
        if !status.is_success() {
            bail!("fetchShortProfile failed: HTTP {}: {}", status, &text[..text.len().min(300)]);
        }
        if text.trim().is_empty() {
            // 204 No Content: the mt/part endpoint returned nothing for this tenant.
            return Ok(serde_json::json!([]));
        }
        Ok(serde_json::from_str(&text)?)
    }

    /// Graph directory search (contacts).
    pub async fn graph_users(&self, top: u32) -> Result<serde_json::Value> {
        let token = self.tokens.token(SCOPE_GRAPH).await?;
        let top_s = top.min(50).to_string();
        let res = self
            .tokens
            .http()
            .get("https://graph.microsoft.com/v1.0/users")
            .bearer_auth(&token)
            .query(&[("$select", "id,displayName,userPrincipalName,mail,jobTitle"), ("$top", top_s.as_str())])
            .send()
            .await?;
        let status = res.status();
        let body = res.text().await?;
        if !status.is_success() {
            bail!("graph users failed: HTTP {}: {}", status, &body[..body.len().min(300)]);
        }
        Ok(serde_json::from_str(&body)?)
    }

    /// Graph /me/people (frequent contacts ranking).
    pub async fn graph_people(&self) -> Result<serde_json::Value> {
        let token = self.tokens.token(SCOPE_GRAPH).await?;
        let res = self
            .tokens
            .http()
            .get("https://graph.microsoft.com/v1.0/me/people")
            .bearer_auth(&token)
            .query(&[("$top", "50")])
            .send()
            .await?;
        let status = res.status();
        let body = res.text().await?;
        if !status.is_success() {
            bail!("graph people failed: HTTP {}: {}", status, &body[..body.len().min(300)]);
        }
        Ok(serde_json::from_str(&body)?)
    }

    /// Graph single user by AAD object id.
    pub async fn graph_user(&self, oid: &str) -> Result<serde_json::Value> {
        let token = self.tokens.token(SCOPE_GRAPH).await?;
        let res = self
            .tokens
            .http()
            .get(format!("https://graph.microsoft.com/v1.0/users/{}", oid))
            .bearer_auth(&token)
            .query(&[("$select", "id,displayName,userPrincipalName,mail,jobTitle,department")])
            .send()
            .await?;
        let status = res.status();
        let body = res.text().await?;
        if !status.is_success() {
            return Err(anyhow!("graph user {} failed: HTTP {}: {}", oid, status, &body[..body.len().min(300)]));
        }
        Ok(serde_json::from_str(&body)?)
    }
}
