use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    http::StatusCode,
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use rf_engine::Engine;
use rf_types::{DeviceStatePatch, Status};
use std::{net::SocketAddr, sync::Arc};
use tower_http::cors::CorsLayer;
#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    let engine = Engine::mock();
    tokio::spawn(engine.clone().run());
    let app = Router::new()
        .route("/api/v1/status", get(status))
        .route("/api/v1/device/state", get(status).patch(patch_state))
        .route("/api/v1/metrics", get(metrics))
        .route("/api/v1/stream/spectrum", get(ws))
        .layer(CorsLayer::permissive())
        .with_state(engine);
    let addr = SocketAddr::from(([127, 0, 0, 1], 8787));
    tracing::info!(%addr,"RFScope mock server ready");
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("localhost port must be bindable");
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await
        .expect("server runtime failed");
}
async fn status(State(e): State<Arc<Engine>>) -> Json<Status> {
    let state = e.state.read().await.clone();
    let fft_size = *e.fft_size.read().await;
    Json(Status {
        name: "RFScope",
        version: env!("CARGO_PKG_VERSION"),
        source: "mock",
        state,
        fft_size,
        diagnostics: e.diagnostics(),
    })
}
async fn metrics(State(e): State<Arc<Engine>>) -> Json<rf_types::Diagnostics> {
    Json(e.diagnostics())
}
async fn patch_state(
    State(e): State<Arc<Engine>>,
    Json(p): Json<DeviceStatePatch>,
) -> Result<Json<Status>, (StatusCode, String)> {
    if let Some(size) = p.fft_size {
        if !(1024..=65536).contains(&size) || !size.is_power_of_two() {
            return Err((
                StatusCode::BAD_REQUEST,
                "fft_size must be a power of two from 1024 through 65536".into(),
            ));
        }
        *e.fft_size.write().await = size;
    }
    let mut s = e.state.write().await;
    if let Some(v) = p.center_frequency_hz {
        s.center_frequency_hz = v
    }
    if let Some(v) = p.sample_rate_hz {
        if !(200_000..=20_000_000).contains(&v) {
            return Err((
                StatusCode::BAD_REQUEST,
                "sample rate outside mock capabilities".into(),
            ));
        }
        s.sample_rate_hz = v
    }
    if let Some(v) = p.running {
        s.running = v
    }
    drop(s);
    Ok(status(State(e)).await)
}
async fn ws(upgrade: WebSocketUpgrade, State(e): State<Arc<Engine>>) -> impl IntoResponse {
    upgrade.on_upgrade(move |socket| stream(socket, e))
}
async fn stream(mut socket: WebSocket, e: Arc<Engine>) {
    e.client_connected();
    let mut rx = e.frames.subscribe();
    while let Ok(frame) = rx.recv().await {
        if socket.send(Message::Binary(frame)).await.is_err() {
            break;
        }
    }
    e.client_disconnected();
}
