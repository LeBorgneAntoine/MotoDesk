pub mod accounting;
pub mod assistant;
pub mod auth;
pub mod background;
pub mod backup;
pub mod catalog;
pub mod clients;
pub mod company;
pub mod dashboard;
pub mod data;
pub mod documents;
pub mod fiscal;
pub mod health;
pub mod import;
pub mod mesh;
pub mod network_accounts;
pub mod pos;
pub mod sales;
pub mod shared;
pub mod stock;
pub mod sync;
pub mod workshop;

use axum::{middleware, Router};

use crate::local_server::local_auth;
use crate::local_server::state::SharedState;

/// Loopback business routes. Mesh apply is mounted separately on the LAN port.
pub fn business_router(state: SharedState) -> Router<SharedState> {
    Router::new()
        .nest("/api/auth", auth::router())
        .nest("/api/clients", clients::router())
        .nest("/api/vehicles", clients::vehicles_router())
        .merge(catalog::router())
        .nest("/api/settings", catalog::settings_router())
        .nest("/api/templates", catalog::templates_router())
        .nest("/api/import", import::router())
        .nest("/api/data", data::router())
        .nest("/api/sales", sales::router())
        .nest("/api/payments", sales::payments_router())
        .merge(pos::router())
        .merge(company::router())
        .merge(documents::router())
        .merge(fiscal::router())
        .merge(accounting::router())
        .merge(mesh::admin_router())
        .merge(network_accounts::router())
        .merge(background::router())
        .merge(sync::router())
        .merge(backup::router())
        .merge(shared::router())
        .merge(dashboard::router())
        .merge(workshop::router())
        .merge(stock::router())
        .merge(assistant::router())
        .layer(middleware::from_fn_with_state(
            state,
            local_auth::require_local_token,
        ))
}
