//! `/v1/orders/live`: a WebSocket that authenticates with its first message,
//! then pushes orders-board updates for one business.

use std::time::Duration;

use axum::{
    extract::{
        State,
        ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade},
    },
    response::Response,
};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::AppState;

/// Browsers can't send headers on a WebSocket, so the token arrives in the first message.
const AUTH_TIMEOUT: Duration = Duration::from_secs(5);
/// Application close code (4000–4999) for a missing or invalid token.
const CLOSE_UNAUTHORIZED: u16 = 4401;

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ClientMessage {
    Auth { token: String, business_id: Uuid },
}

pub async fn upgrade(ws: WebSocketUpgrade, State(state): State<AppState>) -> Response {
    ws.on_upgrade(move |socket| session(socket, state))
}

async fn session(mut socket: WebSocket, state: AppState) {
    let Some((sub, business_id)) = authenticate(&mut socket, &state).await else {
        let frame = CloseFrame {
            code: CLOSE_UNAUTHORIZED,
            reason: "unauthorized".into(),
        };
        let _ = socket.send(Message::Close(Some(frame))).await;
        return;
    };
    if socket
        .send(Message::Text(json!({ "type": "ready" }).to_string().into()))
        .await
        .is_err()
    {
        return;
    }
    tracing::debug!(%sub, %business_id, "orders board session started");

    // TODO(milestone 3): check membership of business_id, then push `board_order` messages
    // from the order.* stream.
    while let Some(Ok(message)) = socket.recv().await {
        if let Message::Close(_) = message {
            break;
        }
    }
}

async fn authenticate(socket: &mut WebSocket, state: &AppState) -> Option<(String, Uuid)> {
    let first = tokio::time::timeout(AUTH_TIMEOUT, socket.recv())
        .await
        .ok()??
        .ok()?;
    let Message::Text(text) = first else {
        return None;
    };
    let ClientMessage::Auth { token, business_id } = serde_json::from_str(&text).ok()?;
    let claims = state.auth.verifier.verify(&token).await.ok()?;
    claims
        .has_scope("merchant")
        .then_some((claims.sub, business_id))
}
