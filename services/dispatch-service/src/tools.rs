//! MCP tools Alexa+ uses to find shops, build a cart and order by voice.
//!
//! Every tool is a stub until milestone 5: it logs its arguments and returns
//! `not_implemented`, so the tool list can already be explored in MCP Inspector.

// Parameter fields and the NATS client are read once the tools are implemented.
#![allow(dead_code)]

use rmcp::{
    ServerHandler,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{Implementation, ServerCapabilities, ServerInfo},
    schemars, tool, tool_handler, tool_router,
};
use serde::Deserialize;
use serde_json::json;

const INSTRUCTIONS: &str = "Drone Drop orders from local Seattle shops for simulated drone delivery; \
payments run in Stripe test mode. Follow this protocol: \
1) find shops with search_nearby_shops or search_places; \
2) show the menu and call update_cart until the customer is happy; \
3) read back the shop, items, total, drop-off location and ETA from the quote; \
4) call place_order only after the customer explicitly says yes. \
New drop-off locations must be verified in the Drone Drop app before they can be used.";

#[derive(Clone)]
pub struct DroneDrop {
    nats: async_nats::Client,
    tool_router: ToolRouter<Self>,
}

impl DroneDrop {
    pub fn new(nats: async_nats::Client) -> Self {
        Self { nats, tool_router: Self::tool_router() }
    }
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SearchNearbyShops {
    /// Place category, e.g. `coffee_shop`, `bakery` or `restaurant`.
    pub category: Option<String>,
    /// Verified delivery location to search around. Defaults to the customer's default location.
    pub near_location_id: Option<String>,
    /// Search radius in metres.
    pub radius_m: Option<u32>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SearchPlaces {
    /// Name or address of a specific shop, restaurant or place.
    pub query: String,
    /// Verified delivery location to bias results towards.
    pub near_location_id: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ShopId {
    pub shop_id: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct GetMenu {
    pub shop_id: String,
    /// Optional text to filter items by.
    pub query: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct Limit {
    /// Maximum number of results.
    pub limit: Option<u32>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct OrderHistory {
    /// Maximum number of orders.
    pub limit: Option<u32>,
    /// Only orders from this shop.
    pub shop_id: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct OrderId {
    pub order_id: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct OptionalOrderId {
    /// Defaults to the customer's most recent active order.
    pub order_id: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct NewDeliveryLocation {
    /// Street address to geocode.
    pub address: String,
    /// Short name the customer uses for it, e.g. "office".
    pub label: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct UpdateCart {
    pub shop_id: String,
    /// The complete cart; replaces any previous items.
    pub items: Vec<CartItem>,
    /// A verified delivery location.
    pub delivery_location_id: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct CartItem {
    pub item_id: String,
    pub qty: u32,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct PlaceOrder {
    /// `quote_id` from the latest `update_cart` result.
    pub quote_id: String,
    /// The quote total you read back to the customer, in cents.
    pub expected_total_cents: i64,
    /// True only if the customer explicitly said yes to the read-back.
    pub customer_confirmed: bool,
}

#[tool_router]
impl DroneDrop {
    #[tool(description = "Find shops near a verified delivery location. Only registered Drone Drop shops can be ordered from; others are marked as not on Drone Drop.")]
    fn search_nearby_shops(&self, Parameters(params): Parameters<SearchNearbyShops>) -> String {
        not_implemented("search_nearby_shops", &params)
    }

    #[tool(description = "Search for a specific shop, restaurant or address by name or address. Flags which results are registered Drone Drop shops.")]
    fn search_places(&self, Parameters(params): Parameters<SearchPlaces>) -> String {
        not_implemented("search_places", &params)
    }

    #[tool(description = "Get a shop's address, opening hours, whether it is accepting orders, prep time, distance and delivery ETA.")]
    fn get_shop_details(&self, Parameters(params): Parameters<ShopId>) -> String {
        not_implemented("get_shop_details", &params)
    }

    #[tool(description = "Get a shop's menu items with price, availability and weight.")]
    fn get_menu(&self, Parameters(params): Parameters<GetMenu>) -> String {
        not_implemented("get_menu", &params)
    }

    #[tool(description = "List shops the customer has ordered from and drop-off locations they have used, with last order date and count.")]
    fn list_past_order_locations(&self, Parameters(params): Parameters<Limit>) -> String {
        not_implemented("list_past_order_locations", &params)
    }

    #[tool(description = "List the customer's past orders, newest first.")]
    fn list_order_history(&self, Parameters(params): Parameters<OrderHistory>) -> String {
        not_implemented("list_order_history", &params)
    }

    #[tool(description = "Start a new cart with the items of a past order. Read back the new quote before ordering.")]
    fn reorder(&self, Parameters(params): Parameters<OrderId>) -> String {
        not_implemented("reorder", &params)
    }

    #[tool(description = "List the customer's verified delivery locations and any still pending verification.")]
    fn list_delivery_locations(&self) -> String {
        not_implemented("list_delivery_locations", &())
    }

    #[tool(description = "Add a new delivery location. It stays pending until the customer verifies it with MFA in the Drone Drop app; tell them to do so.")]
    fn request_new_delivery_location(&self, Parameters(params): Parameters<NewDeliveryLocation>) -> String {
        not_implemented("request_new_delivery_location", &params)
    }

    #[tool(description = "Replace the cart and get a quote (subtotal, delivery fee, total, weight check, ETA) valid for 10 minutes. Read the quote back to the customer and ask them to confirm.")]
    fn update_cart(&self, Parameters(params): Parameters<UpdateCart>) -> String {
        not_implemented("update_cart", &params)
    }

    #[tool(description = "Place the order and authorize payment. Call only after reading back the quote and the customer explicitly saying yes. The business must accept before the card is charged.")]
    fn place_order(&self, Parameters(params): Parameters<PlaceOrder>) -> String {
        not_implemented("place_order", &params)
    }

    #[tool(description = "Get the live status of an order: payment, merchant acceptance and drone progress.")]
    fn get_order_status(&self, Parameters(params): Parameters<OptionalOrderId>) -> String {
        not_implemented("get_order_status", &params)
    }

    #[tool(description = "Cancel an order before pickup. The payment is voided or refunded and the drone recalled.")]
    fn cancel_order(&self, Parameters(params): Parameters<OrderId>) -> String {
        not_implemented("cancel_order", &params)
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for DroneDrop {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::from_build_env())
            .with_instructions(INSTRUCTIONS)
    }
}

fn not_implemented(tool: &str, params: &impl std::fmt::Debug) -> String {
    tracing::info!(tool, ?params, "stub tool called");
    json!({
        "status": "not_implemented",
        "tool": tool,
        "next_step": "Tell the customer this Drone Drop feature is not available yet.",
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn server() -> DroneDrop {
        // Connects in the background, so no NATS server is needed.
        let nats = async_nats::ConnectOptions::new()
            .retry_on_initial_connect()
            .connect("nats://127.0.0.1:1")
            .await
            .unwrap();
        DroneDrop::new(nats)
    }

    #[tokio::test]
    async fn lists_every_tool() {
        let mut names: Vec<String> =
            server().await.tool_router.list_all().into_iter().map(|tool| tool.name.to_string()).collect();
        names.sort();
        assert_eq!(
            names,
            [
                "cancel_order",
                "get_menu",
                "get_order_status",
                "get_shop_details",
                "list_delivery_locations",
                "list_order_history",
                "list_past_order_locations",
                "place_order",
                "reorder",
                "request_new_delivery_location",
                "search_nearby_shops",
                "search_places",
                "update_cart",
            ]
        );
    }

    #[tokio::test]
    async fn place_order_requires_quote_total_and_confirmation() {
        let server = server().await;
        let tool = server.tool_router.get("place_order").unwrap();
        let mut required: Vec<&str> =
            tool.input_schema["required"].as_array().unwrap().iter().map(|v| v.as_str().unwrap()).collect();
        required.sort();
        assert_eq!(required, ["customer_confirmed", "expected_total_cents", "quote_id"]);
    }
}
