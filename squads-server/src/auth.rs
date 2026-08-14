//! Teams token management, ported from Squads src/api.rs / src/auth.rs.
use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;

use crate::config::{SCOPE_SPACES, TEAMS_CLIENT_ID, TENANT};

const CLAIMS: &str = "{\"access_token\":{\"xms_cc\":{\"values\":[\"CP1\"]}}}";

pub fn now_epoch() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() as i64
}

#[derive(Clone, Serialize, Deserialize)]
pub struct StoredTokens {
    pub refresh_token: String,
    #[serde(default)]
    pub tenant: Option<String>,
}

#[derive(Clone)]
pub struct MeInfo {
    pub oid: String,
    pub display_name: String,
    pub upn: String,
}

/// Manages the Teams refresh token and per-scope access tokens, mirroring
/// Squads get_or_gen_token / renew_refresh_token.
#[derive(Clone)]
pub struct TokenManager {
    client: reqwest::Client,
    refresh_token: Arc<RwLock<String>>,
    access_tokens: Arc<RwLock<HashMap<String, (String, i64)>>>, // scope -> (value, expires)
    store_path: Arc<std::path::PathBuf>,
    me: Arc<RwLock<Option<MeInfo>>>,
}

impl TokenManager {
    pub async fn load(store_path: std::path::PathBuf, refresh_token_env: Option<String>) -> Result<Self> {
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        let mut refresh_token = refresh_token_env;
        if refresh_token.is_none() {
            if let Ok(text) = tokio::fs::read_to_string(&store_path).await {
                let v: serde_json::Value = serde_json::from_str(&text)
                    .with_context(|| format!("token store {} is not valid JSON", store_path.display()))?;
                // Accept our own format {"refresh_token": "..."} or Squads
                // {"refresh_token": {"value": "..."}} cache format.
                refresh_token = match v.get("refresh_token") {
                    Some(serde_json::Value::String(s)) => Some(s.clone()),
                    Some(serde_json::Value::Object(o)) => o.get("value").and_then(|x| x.as_str()).map(String::from),
                    _ => None,
                };
            }
        }
        let refresh_token = refresh_token.ok_or_else(|| anyhow!(
            "no refresh token: set SQUADS_REFRESH_TOKEN or SQUADS_TOKEN_STORE file (tried {})",
            store_path.display()
        ))?;
        Ok(Self {
            client,
            refresh_token: Arc::new(RwLock::new(refresh_token)),
            access_tokens: Arc::new(RwLock::new(HashMap::new())),
            store_path: Arc::new(store_path),
            me: Arc::new(RwLock::new(None)),
        })
    }

    pub fn http(&self) -> &reqwest::Client {
        &self.client
    }

    async fn persist_refresh_token(&self, rt: &str) {
        let data = serde_json::to_string(&StoredTokens {
            refresh_token: rt.to_string(),
            tenant: Some(TENANT.to_string()),
        })
        .unwrap_or_default();
        if let Some(parent) = self.store_path.parent() {
            let _ = tokio::fs::create_dir_all(parent).await;
        }
        let _ = tokio::fs::write(&*self.store_path, data).await;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = tokio::fs::set_permissions(&*self.store_path, std::fs::Permissions::from_mode(0o600)).await;
        }
    }

    /// Exchange the refresh token for an access token of `scope`
    /// (exactly Squads gen_token: v2.0/token + CP1 claims).
    async fn refresh_scope(&self, scope: &str) -> Result<(String, i64)> {
        let rt = self.refresh_token.read().await.clone();
        let form = [
            ("client_id", TEAMS_CLIENT_ID.to_string()),
            ("scope", format!("{} openid profile offline_access", scope)),
            ("grant_type", "refresh_token".to_string()),
            ("refresh_token", rt.clone()),
            ("claims", CLAIMS.to_string()),
        ];
        let res = self
            .client
            .post(format!("https://login.microsoftonline.com/{}/oauth2/v2.0/token", TENANT))
            .header("origin", "https://teams.microsoft.com")
            .form(&form)
            .send()
            .await?;
        let status = res.status();
        let body = res.text().await?;
        if !status.is_success() {
            bail!("token refresh for scope {} failed: HTTP {}: {}", scope, status, &body[..body.len().min(300)]);
        }
        let v: serde_json::Value = serde_json::from_str(&body)?;
        let access_token = v.get("access_token").and_then(|x| x.as_str())
            .ok_or_else(|| anyhow!("no access_token in response"))?
            .to_string();
        let expires_in = v.get("expires_in").and_then(|x| x.as_str()).and_then(|x| x.parse::<i64>().ok()).unwrap_or(3599);
        // Rotated refresh token (FOCI): keep the newest one.
        if let Some(new_rt) = v.get("refresh_token").and_then(|x| x.as_str()) {
            *self.refresh_token.write().await = new_rt.to_string();
            self.persist_refresh_token(new_rt).await;
        }
        Ok((access_token, now_epoch() + expires_in - 60))
    }

    /// Return a valid access token for `scope`, refreshing when needed.
    pub async fn token(&self, scope: &str) -> Result<String> {
        {
            let cache = self.access_tokens.read().await;
            if let Some((tok, exp)) = cache.get(scope) {
                if *exp > now_epoch() {
                    return Ok(tok.clone());
                }
            }
        }
        let (tok, exp) = self.refresh_scope(scope).await?;
        self.access_tokens.write().await.insert(scope.to_string(), (tok.clone(), exp));
        Ok(tok)
    }

    /// Skype token used by fetchShortProfile etc. (Squads gen_skype_token).
    pub async fn skype_token(&self) -> Result<String> {
        {
            let cache = self.access_tokens.read().await;
            if let Some((tok, exp)) = cache.get("skype_token") {
                if *exp > now_epoch() {
                    return Ok(tok.clone());
                }
            }
        }
        let spaces = self.token(SCOPE_SPACES).await?;
        let res = self
            .client
            .post("https://teams.microsoft.com/api/authsvc/v1.0/authz")
            .header("authorization", format!("Bearer {}", spaces))
            .header("Content-Length", "0")
            .send()
            .await?;
        let status = res.status();
        let body = res.text().await?;
        if !status.is_success() {
            bail!("authsvc failed: HTTP {}: {}", status, &body[..body.len().min(300)]);
        }
        let v: serde_json::Value = serde_json::from_str(&body)?;
        let tokens = v.get("tokens").ok_or_else(|| anyhow!("authsvc: no tokens field"))?;
        let skype = tokens.get("skypeToken").and_then(|x| x.as_str()).ok_or_else(|| anyhow!("authsvc: no skypeToken"))?;
        let expires_in = tokens.get("expiresIn").and_then(|x| x.as_i64()).unwrap_or(3600);
        let exp = now_epoch() + expires_in - 60;
        self.access_tokens.write().await.insert("skype_token".to_string(), (skype.to_string(), exp));
        Ok(skype.to_string())
    }

    /// Cached identity of the signed-in user (from Graph /me).
    pub async fn me(&self) -> Result<MeInfo> {
        if let Some(me) = self.me.read().await.clone() {
            return Ok(me);
        }
        let token = self.token(crate::config::SCOPE_GRAPH).await?;
        let res = self.client.get("https://graph.microsoft.com/v1.0/me")
            .bearer_auth(&token).send().await?;
        let status = res.status();
        let body = res.text().await?;
        if !status.is_success() {
            bail!("graph /me failed: HTTP {}: {}", status, &body[..body.len().min(300)]);
        }
        let v: serde_json::Value = serde_json::from_str(&body)?;
        let me = MeInfo {
            oid: v.get("id").and_then(|x| x.as_str()).unwrap_or_default().to_string(),
            display_name: v.get("displayName").and_then(|x| x.as_str()).unwrap_or_default().to_string(),
            upn: v.get("userPrincipalName").and_then(|x| x.as_str()).unwrap_or_default().to_string(),
        };
        *self.me.write().await = Some(me.clone());
        Ok(me)
    }
}
