use std::sync::Arc;
use tokio::sync::RwLock;

use squads_server::api::{self, AppState};
use squads_server::auth::TokenManager;
use squads_server::config::Config;
use squads_server::teams::TeamsClient;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "squads_server=info,tower_http=info".into()),
        )
        .init();

    let cfg = Config::from_env()?;
    tracing::info!("loading Teams token from {}", cfg.token_store.display());
    let tokens = TokenManager::load(cfg.token_store.clone(), cfg.refresh_token.clone()).await?;
    let teams = TeamsClient { tokens };
    let state = Arc::new(AppState {
        cfg: cfg.clone(),
        teams,
        dir: RwLock::new(None),
    });
    let app = api::router(state);
    let listener = tokio::net::TcpListener::bind(&cfg.bind).await?;
    tracing::info!("squads-server listening on {} (allowed groups: {:?})", cfg.bind, cfg.allowed_groups);
    axum::serve(listener, app).await?;
    Ok(())
}
