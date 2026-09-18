//! The `/v1` merchant API described in `api/openapi/merchant.yaml`.
//!
//! Every operation requires an access token for this API with the `merchant`
//! scope. Operations return `501` until their data access and gRPC clients exist.

use axum::{
    Json, Router,
    extract::{FromRequestParts, Path, State},
    http::{StatusCode, request::Parts},
    response::Response,
    routing::{get, post},
};
use serde_json::{Value, json};
use svc_auth::{AccessClaims, AuthRejection, Authenticated, require_scope};
use svc_common::{not_implemented, problem};
use uuid::Uuid;

use crate::AppState;

/// A signed-in merchant: a valid access token for this API carrying the `merchant` scope.
/// Membership of the business in the path is checked by each handler once it has data.
#[allow(dead_code)] // handlers read the claims once they check membership (milestone 3)
pub struct Merchant(pub AccessClaims);

impl FromRequestParts<AppState> for Merchant {
    type Rejection = AuthRejection;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let Authenticated(claims) = Authenticated::from_request_parts(parts, state).await?;
        require_scope(&claims, "merchant")?;
        Ok(Self(claims))
    }
}

pub fn routes() -> Router<AppState> {
    let business = Router::new()
        .route("/orders", get(pending))
        .route("/orders/{order_id}/accept", post(pending))
        .route("/orders/{order_id}/reject", post(pending))
        .route("/menu", get(menu))
        .route("/menu/sections", post(create_section))
        .route("/menu/items", post(create_item))
        .route("/menu/items/{item_id}", axum::routing::patch(update_item))
        .route("/station", get(pending).put(pending))
        .route("/payouts/status", get(pending))
        .route("/payouts/onboarding-link", post(pending));

    Router::new()
        .route(
            "/.well-known/oauth-protected-resource",
            get(protected_resource_metadata),
        )
        .route("/v1/businesses", get(list_businesses))
        .nest("/v1/businesses/{business_id}", business)
        .route("/v1/orders/live", get(crate::live::upgrade))
}

/// RFC 9728 metadata: which authorization server issues tokens for this API.
async fn protected_resource_metadata(State(state): State<AppState>) -> Json<Value> {
    Json(json!({
        "resource": state.auth.verifier.audience(),
        "authorization_servers": [state.auth.verifier.issuer()],
        "scopes_supported": ["openid", "email", "merchant"],
        "bearer_methods_supported": ["header"],
    }))
}

fn user_id(claims: &AccessClaims) -> Result<Uuid, Response> {
    claims.sub.parse().map_err(|_| {
        problem(
            StatusCode::UNAUTHORIZED,
            "Invalid subject",
            Some("INVALID_TOKEN"),
        )
    })
}

async fn member(
    state: &AppState,
    claims: &AccessClaims,
    business_id: Uuid,
) -> Result<(), Response> {
    let user_id = user_id(claims)?;
    let is_member: (bool,) = sqlx::query_as(
        "SELECT EXISTS (SELECT 1 FROM business_members WHERE business_id = $1 AND user_sub = $2)",
    )
    .bind(business_id)
    .bind(user_id)
    .fetch_one(&state.db)
    .await
    .map_err(|_| problem(StatusCode::INTERNAL_SERVER_ERROR, "Database error", None))?;
    if is_member.0 {
        Ok(())
    } else {
        Err(problem(
            StatusCode::FORBIDDEN,
            "You are not a member of this business",
            Some("BUSINESS_ACCESS_DENIED"),
        ))
    }
}

fn business_id(value: &str) -> Result<Uuid, Response> {
    value.parse().map_err(|_| {
        problem(
            StatusCode::BAD_REQUEST,
            "Invalid business id",
            Some("INVALID_BUSINESS_ID"),
        )
    })
}

pub async fn list_businesses(
    State(state): State<AppState>,
    Merchant(claims): Merchant,
) -> Result<Json<Value>, Response> {
    let user_id = user_id(&claims)?;
    let rows = sqlx::query_as::<
        _,
        (
            Uuid,
            String,
            String,
            f64,
            f64,
            String,
            String,
            bool,
            bool,
            i32,
        ),
    >(
        "SELECT b.id, b.name, b.address, b.lat, b.lon, b.status, bm.role,
                b.accepting_orders, b.payments_enabled, b.prep_time_minutes
         FROM businesses b JOIN business_members bm ON bm.business_id = b.id
         WHERE bm.user_sub = $1 ORDER BY b.created_at",
    )
    .bind(user_id)
    .fetch_all(&state.db)
    .await
    .map_err(|_| problem(StatusCode::INTERNAL_SERVER_ERROR, "Database error", None))?;
    Ok(Json(json!({
        "businesses": rows.into_iter().map(|(id, name, address, lat, lon, status, role, accepting_orders, payments_enabled, prep_time_minutes)| json!({
            "business_id": id,
            "name": name,
            "address": address,
            "position": { "lat": lat, "lon": lon },
            "status": status,
            "role": role,
            "accepting_orders": accepting_orders,
            "payments_enabled": payments_enabled,
            "prep_time_minutes": prep_time_minutes,
        })).collect::<Vec<_>>()
    })))
}

pub async fn menu(
    State(state): State<AppState>,
    Merchant(claims): Merchant,
    Path(raw_business_id): Path<String>,
) -> Result<Json<Value>, Response> {
    let business_id = business_id(&raw_business_id)?;
    member(&state, &claims, business_id).await?;
    let sections = sqlx::query_as::<_, (Uuid, String, i32)>(
        "SELECT id, name, position FROM menu_sections WHERE business_id = $1 ORDER BY position, id",
    )
    .bind(business_id)
    .fetch_all(&state.db)
    .await
    .map_err(|_| problem(StatusCode::INTERNAL_SERVER_ERROR, "Database error", None))?;
    let items = sqlx::query_as::<_, (Uuid, Uuid, String, String, String, i64, String, i32, Option<i32>, bool)>(
        "SELECT id, section_id, name, spoken_name, description, price_cents, currency, weight_g, stock_qty, available
         FROM menu_items WHERE business_id = $1 ORDER BY section_id, name",
    )
    .bind(business_id)
    .fetch_all(&state.db)
    .await
    .map_err(|_| problem(StatusCode::INTERNAL_SERVER_ERROR, "Database error", None))?;
    Ok(Json(
        json!({ "sections": sections.into_iter().map(|(id, name, position)| {
        let section_items = items.iter().filter(|item| item.1 == id).map(|(item_id, section_id, name, spoken_name, description, price_cents, currency, weight_g, stock_qty, available)| json!({
            "item_id": item_id, "section_id": section_id, "name": name, "spoken_name": spoken_name, "description": description,
            "price": { "amount_cents": price_cents, "currency": currency }, "weight_g": weight_g,
            "stock_qty": stock_qty, "available": available,
        })).collect::<Vec<_>>();
        json!({ "section_id": id, "name": name, "position": position, "items": section_items })
    }).collect::<Vec<_>>() }),
    ))
}

#[derive(serde::Deserialize)]
struct SectionInput {
    name: String,
}

async fn create_section(
    State(state): State<AppState>,
    Merchant(claims): Merchant,
    Path(raw_business_id): Path<String>,
    Json(input): Json<SectionInput>,
) -> Result<(StatusCode, Json<Value>), Response> {
    let business_id = business_id(&raw_business_id)?;
    member(&state, &claims, business_id).await?;
    let name = input.name.trim();
    if name.is_empty() {
        return Err(problem(
            StatusCode::BAD_REQUEST,
            "Section name is required",
            Some("INVALID_NAME"),
        ));
    }
    let position: (i32,) = sqlx::query_as(
        "SELECT COALESCE(max(position), -1) + 1 FROM menu_sections WHERE business_id = $1",
    )
    .bind(business_id)
    .fetch_one(&state.db)
    .await
    .map_err(|_| problem(StatusCode::INTERNAL_SERVER_ERROR, "Database error", None))?;
    let id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO menu_sections (id, business_id, name, position) VALUES ($1, $2, $3, $4)",
    )
    .bind(id)
    .bind(business_id)
    .bind(name)
    .bind(position.0)
    .execute(&state.db)
    .await
    .map_err(|_| problem(StatusCode::INTERNAL_SERVER_ERROR, "Database error", None))?;
    Ok((
        StatusCode::CREATED,
        Json(json!({ "section_id": id, "name": name, "position": position.0, "items": [] })),
    ))
}

#[derive(serde::Deserialize, serde::Serialize)]
struct MoneyInput {
    amount_cents: i64,
    currency: String,
}

#[derive(serde::Deserialize)]
struct ItemInput {
    section_id: Uuid,
    name: String,
    /// What Alexa+ reads out. Defaults to the name.
    spoken_name: Option<String>,
    #[serde(default)]
    description: String,
    price: MoneyInput,
    weight_g: i32,
    stock_qty: Option<i32>,
    #[serde(default = "default_true")]
    available: bool,
}

fn default_true() -> bool {
    true
}

async fn create_item(
    State(state): State<AppState>,
    Merchant(claims): Merchant,
    Path(raw_business_id): Path<String>,
    Json(input): Json<ItemInput>,
) -> Result<(StatusCode, Json<Value>), Response> {
    let business_id = business_id(&raw_business_id)?;
    member(&state, &claims, business_id).await?;
    if input.name.trim().is_empty()
        || input.price.amount_cents < 0
        || input.weight_g <= 0
        || input.stock_qty.is_some_and(|qty| qty < 0)
    {
        return Err(problem(
            StatusCode::BAD_REQUEST,
            "Invalid menu item",
            Some("INVALID_MENU_ITEM"),
        ));
    }
    let id = Uuid::now_v7();
    let spoken_name = input
        .spoken_name
        .as_deref()
        .map(str::trim)
        .filter(|spoken| !spoken.is_empty())
        .unwrap_or(input.name.trim());
    let inserted = sqlx::query("INSERT INTO menu_items (id, business_id, section_id, name, spoken_name, description, price_cents, currency, weight_g, stock_qty, available) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)")
        .bind(id).bind(business_id).bind(input.section_id).bind(input.name.trim()).bind(spoken_name).bind(input.description.trim()).bind(input.price.amount_cents).bind(input.price.currency.to_lowercase()).bind(input.weight_g).bind(input.stock_qty).bind(input.available).execute(&state.db).await;
    if let Err(sqlx::Error::Database(error)) = &inserted
        && error.constraint().is_some()
    {
        return Err(problem(
            StatusCode::BAD_REQUEST,
            "The menu section does not belong to this business",
            Some("INVALID_SECTION"),
        ));
    }
    inserted.map_err(|_| problem(StatusCode::INTERNAL_SERVER_ERROR, "Database error", None))?;
    Ok((
        StatusCode::CREATED,
        Json(
            json!({ "item_id": id, "section_id": input.section_id, "name": input.name.trim(), "spoken_name": spoken_name, "description": input.description.trim(), "price": input.price, "weight_g": input.weight_g, "stock_qty": input.stock_qty, "available": input.available }),
        ),
    ))
}

async fn update_item(
    State(state): State<AppState>,
    Merchant(claims): Merchant,
    Path((raw_business_id, item_id)): Path<(String, Uuid)>,
    Json(input): Json<Value>,
) -> Result<Json<Value>, Response> {
    let business_id = business_id(&raw_business_id)?;
    member(&state, &claims, business_id).await?;
    let result = sqlx::query("UPDATE menu_items SET name = COALESCE($3->>'name', name), spoken_name = COALESCE($3->>'spoken_name', spoken_name), description = COALESCE($3->>'description', description), price_cents = COALESCE(($3->'price'->>'amount_cents')::bigint, price_cents), weight_g = COALESCE(($3->>'weight_g')::integer, weight_g), stock_qty = CASE WHEN $3 ? 'stock_qty' THEN ($3->>'stock_qty')::integer ELSE stock_qty END, available = COALESCE(($3->>'available')::boolean, available), updated_at = now() WHERE id = $1 AND business_id = $2")
        .bind(item_id).bind(business_id).bind(&input).execute(&state.db).await.map_err(|_| problem(StatusCode::BAD_REQUEST, "Invalid menu item update", Some("INVALID_MENU_ITEM")))?;
    if result.rows_affected() == 0 {
        return Err(problem(StatusCode::NOT_FOUND, "Menu item not found", None));
    }
    let row = sqlx::query_as::<_, (Uuid, Uuid, String, String, String, i64, String, i32, Option<i32>, bool)>("SELECT id, section_id, name, spoken_name, description, price_cents, currency, weight_g, stock_qty, available FROM menu_items WHERE id = $1 AND business_id = $2").bind(item_id).bind(business_id).fetch_one(&state.db).await.map_err(|_| problem(StatusCode::INTERNAL_SERVER_ERROR, "Database error", None))?;
    Ok(Json(
        json!({ "item_id": row.0, "section_id": row.1, "name": row.2, "spoken_name": row.3, "description": row.4, "price": { "amount_cents": row.5, "currency": row.6 }, "weight_g": row.7, "stock_qty": row.8, "available": row.9 }),
    ))
}

/// A merchant operation whose implementation isn't wired up yet.
async fn pending(Merchant(_): Merchant) -> Response {
    not_implemented()
}
