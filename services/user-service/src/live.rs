//! `/v1/live`: a WebSocket that authenticates with its first message, then
//! pushes live updates to the customer.

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
use svc_auth::AccessClaims;

use crate::AppState;

/// Browsers can't send headers on a WebSocket, so the token arrives in the first message.
const AUTH_TIMEOUT: Duration = Duration::from_secs(5);
/// Application close code (4000–4999) for a missing or invalid token.
const CLOSE_UNAUTHORIZED: u16 = 4401;

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ClientMessage {
    Auth { token: String },
}

pub async fn upgrade(ws: WebSocketUpgrade, State(state): State<AppState>) -> Response {
    ws.on_upgrade(move |socket| session(socket, state))
}

async fn session(mut socket: WebSocket, state: AppState) {
    let Some(mut claims) = authenticate(&mut socket, &state).await else {
        close_unauthorized(socket).await;
        return;
    };
    if send_json(&mut socket, json!({ "type": "ready" })).await.is_err() {
        return;
    }
    tracing::debug!(sub = %claims.sub, "live session started");

    // TODO: forward the zones' TELEMETRY stream (1 Hz) and mission.* events for this customer's orders.
    while let Some(Ok(message)) = socket.recv().await {
        match message {
            Message::Text(text) => match serde_json::from_str::<ClientMessage>(&text) {
                // Clients re-send `auth` with a fresh token before the current one expires.
                Ok(ClientMessage::Auth { token }) => match state.auth.verifier.verify(&token).await {
                    Ok(fresh) if fresh.sub == claims.sub => claims = fresh,
                    _ => return close_unauthorized(socket).await,
                },
                Err(_) => {
                    let _ = send_json(&mut socket, json!({ "type": "error", "title": "Unknown message" })).await;
                }
            },
            Message::Close(_) => break,
            _ => {}
        }
    }
}

async fn authenticate(socket: &mut WebSocket, state: &AppState) -> Option<AccessClaims> {
    let first = tokio::time::timeout(AUTH_TIMEOUT, socket.recv()).await.ok()??.ok()?;
    let Message::Text(text) = first else {
        return None;
    };
    let ClientMessage::Auth { token } = serde_json::from_str(&text).ok()?;
    state.auth.verifier.verify(&token).await.ok()
}

async fn send_json(socket: &mut WebSocket, value: serde_json::Value) -> Result<(), axum::Error> {
    socket.send(Message::Text(value.to_string().into())).await
}

async fn close_unauthorized(mut socket: WebSocket) {
    let frame = CloseFrame { code: CLOSE_UNAUTHORIZED, reason: "unauthorized".into() };
    let _ = socket.send(Message::Close(Some(frame))).await;
}
