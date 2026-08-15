//! Server configuration from environment variables.
use anyhow::{bail, Context, Result};
use std::path::PathBuf;

pub const TEAMS_CLIENT_ID: &str = "1fec8e78-bce4-4aaf-ab1b-5451cc387264";
pub const TENANT: &str = "organizations";

pub const SCOPE_CHATSVCAGG: &str = "https://chatsvcagg.teams.microsoft.com/.default";
pub const SCOPE_IC3: &str = "https://ic3.teams.office.com/.default";
pub const SCOPE_GRAPH: &str = "https://graph.microsoft.com/.default";
pub const SCOPE_SPACES: &str = "https://api.spaces.skype.com/Authorization.ReadWrite";

#[derive(Clone, Debug)]
pub struct Config {
    pub bind: String,
    /// Allowed API tokens (client credentials for the REST API).
    pub api_tokens: Vec<String>,
    /// Groups that may receive messages: either immutable thread ids ("19:...")
    /// or exact display names (resolved once at startup to a unique thread id;
    /// ambiguous or missing names fail closed).
    pub allowed_groups: Vec<String>,
    /// Where the Teams refresh token lives / is persisted.
    pub token_store: PathBuf,
    /// Refresh token given directly (optional; file is the fallback).
    pub refresh_token: Option<String>,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        let bind = std::env::var("SQUADS_BIND").unwrap_or_else(|_| "127.0.0.1:8787".into());
        let api_tokens: Vec<String> = std::env::var("SQUADS_API_TOKENS")
            .context("SQUADS_API_TOKENS must be set (comma separated api tokens)")?
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        if api_tokens.is_empty() {
            bail!("SQUADS_API_TOKENS must contain at least one token");
        }
        let allowed_groups: Vec<String> = std::env::var("SQUADS_ALLOWED_GROUPS")
            .unwrap_or_else(|_| "low latency engine devops".into())
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        let token_store = std::env::var("SQUADS_TOKEN_STORE")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                std::env::var("SQUADS_TOKEN_FILE")
                    .map(PathBuf::from)
                    .unwrap_or_else(|_| PathBuf::from("/private/tmp/squads-server-tokens.json"))
            });
        let refresh_token = std::env::var("SQUADS_REFRESH_TOKEN").ok().filter(|s| !s.is_empty());
        Ok(Config { bind, api_tokens, allowed_groups, token_store, refresh_token })
    }
}
