//! The workshop (the one call of `desktop/src/lib/api/workshop.ts` the migrated screens use;
//! the rest comes with L'Atelier).

use serde::Deserialize;

use crate::error::ApiResult;
use crate::session::Client;

/// A repair order, as far as the screens that only open one need to know it.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct WorkshopOrderRef {
    pub id: String,
}

/// Open a repair order from a quote, with its customer, vehicle and lines. The quote takes
/// its number on the way; asking again returns the order that already exists.
pub async fn create_order_from_quote(client: &Client, quote_id: &str) -> ApiResult<WorkshopOrderRef> {
    client.post(format!("/api/workshop-orders/from-quote/{quote_id}"), &serde_json::json!({})).await
}
