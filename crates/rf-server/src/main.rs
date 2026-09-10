use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Path, State,
    },
    http::StatusCode,
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use rf_device::control::{ControlError, DeviceController};
use rf_engine::Engine;
use rf_types::{DeviceCommand, DeviceInventory, DeviceSelection, DeviceStatePatch, Status};
use serde::Deserialize;
use std::{net::SocketAddr, sync::Arc};
use tokio::sync::Mutex;
use tower_http::cors::CorsLayer;
struct AppState {
    engine: Arc<Engine>,
    devices: DeviceController,
    mutations: Mutex<()>,
}
type ApiError = (StatusCode, String);
fn api_error(error: ControlError) -> ApiError {
    let code = match error {
        ControlError::Invalid(_) => StatusCode::BAD_REQUEST,
        ControlError::Conflict(_) => StatusCode::CONFLICT,
        _ => StatusCode::SERVICE_UNAVAILABLE,
    };
    (code, error.to_string())
}
async fn device_call<T: Send + 'static>(
    state: &AppState,
    call: impl FnOnce(DeviceController) -> rf_device::control::Result<T> + Send + 'static,
) -> Result<T, ApiError> {
    let devices = state.devices.clone();
    tokio::task::spawn_blocking(move || call(devices))
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .map_err(api_error)
}
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    let port = std::env::var("RFSCOPE_PORT")
        .unwrap_or_else(|_| "8787".into())
        .parse::<u16>()?;
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    let storage_path = std::env::var("RFSCOPE_DB").unwrap_or_else(|_| "rfscope.sqlite3".into());
    let storage = rf_engine::storage::Storage::open(storage_path)?;
    let engine = Engine::mock_with_storage(Some(Arc::new(storage)));
    let controller = DeviceController::new()?;
    let dsp_engine = engine.clone();
    let dsp_devices = controller.clone();
    // Dedicated DSP thread keeps CPU processing outside the network runtime.
    let dsp_thread = std::thread::Builder::new()
        .name("sdr-dsp".into())
        .spawn(move || {
            match tokio::runtime::Builder::new_current_thread()
                .enable_time()
                .build()
            {
                Ok(runtime) => runtime.block_on(dsp_engine.run(dsp_devices)),
                Err(error) => tracing::error!(%error, "DSP runtime failed"),
            }
        })?;
    let state = Arc::new(AppState {
        engine,
        devices: controller,
        mutations: Mutex::new(()),
    });
    let app = Router::new()
        .route("/api/v1/devices", get(devices))
        .route("/api/v1/device/control", get(device_status).patch(control))
        .route("/api/v1/status", get(status))
        .route("/api/v1/device/state", get(status).patch(patch_state))
        .route("/api/v1/metrics", get(metrics))
        .route("/api/v1/analysis", get(analysis))
        .route("/api/v1/sessions", get(sessions))
        .route("/api/v1/recordings", get(recordings))
        .route("/api/v1/storage/recordings", get(storage_recordings))
        .route("/api/v1/detections", get(detections))
        .route("/api/v1/stream/spectrum", get(ws))
        .route("/api/v1/stream/audio", get(ws_audio))
        .route(
            "/api/v1/recording",
            get(recording_status)
                .post(recording_start)
                .delete(recording_stop),
        )
        .route(
            "/api/v1/playback",
            get(playback_status)
                .post(playback_load)
                .patch(playback_command)
                .delete(playback_clear),
        )
        .route("/api/v1/vfos", get(vfos).post(add_vfo))
        .route(
            "/api/v1/vfos/{id}",
            axum::routing::patch(update_vfo).delete(remove_vfo),
        )
        .layer(CorsLayer::permissive())
        .with_state(state.clone());
    tracing::info!(%addr,"RFScope server ready");
    let served = axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await;
    state
        .engine
        .shutdown
        .store(true, std::sync::atomic::Ordering::Release);
    dsp_thread.join().map_err(|_| "DSP thread panicked")?;
    device_call(&state, |d| d.shutdown())
        .await
        .map_err(|(_, message)| message)?;
    served?;
    Ok(())
}
async fn snapshot(e: &AppState) -> Result<Status, ApiError> {
    let device = device_call(e, |d| d.snapshot()).await?;
    let mut state = e.engine.state.read().await.clone();
    if let Some(playback) = e.engine.playback.summary() {
        state.center_frequency_hz = playback.center_frequency_hz;
        state.sample_rate_hz = playback.sample_rate_hz;
        state.running = playback.playing;
    }
    if device.descriptor.driver != "mock" {
        state.running = device.running;
        state.center_frequency_hz = device
            .configuration
            .as_ref()
            .map_or(0, |c| c.center_frequency_hz);
        state.sample_rate_hz = device
            .configuration
            .as_ref()
            .map_or(0, |c| c.sample_rate_hz);
    }
    Ok(Status {
        name: "RFScope",
        version: env!("CARGO_PKG_VERSION"),
        source: device.descriptor.driver.clone(),
        device,
        state,
        fft_size: *e.engine.fft_size.read().await,
        diagnostics: e.engine.diagnostics(),
    })
}
async fn status(State(e): State<Arc<AppState>>) -> Result<Json<Status>, ApiError> {
    let _guard = e.mutations.lock().await;
    Ok(Json(snapshot(&e).await?))
}
async fn metrics(State(e): State<Arc<AppState>>) -> Json<rf_types::Diagnostics> {
    Json(e.engine.diagnostics())
}
async fn analysis(
    State(e): State<Arc<AppState>>,
) -> Json<Option<rf_dsp::analysis::SpectrumMeasurements>> {
    Json(e.engine.latest_measurements.read().await.clone())
}
async fn sessions(
    State(e): State<Arc<AppState>>,
) -> Result<Json<Vec<rf_engine::storage::StoredSession>>, ApiError> {
    let Some(storage) = &e.engine.storage else {
        return Ok(Json(Vec::new()));
    };
    storage
        .sessions()
        .map(Json)
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))
}
async fn storage_recordings(
    State(e): State<Arc<AppState>>,
) -> Result<Json<Vec<rf_engine::storage::StoredRecording>>, ApiError> {
    let Some(storage) = &e.engine.storage else {
        return Ok(Json(Vec::new()));
    };
    storage
        .recordings()
        .map(Json)
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))
}
async fn recordings(
    State(e): State<Arc<AppState>>,
) -> Json<Vec<rf_engine::recording::RecordingSummary>> {
    Json(e.engine.recording.status().into_iter().collect())
}
async fn detections(State(_e): State<Arc<AppState>>) -> Json<Vec<serde_json::Value>> {
    Json(Vec::new())
}
async fn devices(State(e): State<Arc<AppState>>) -> Result<Json<DeviceInventory>, ApiError> {
    Ok(Json(device_call(&e, |d| d.inventory()).await?))
}
async fn recording_status(
    State(e): State<Arc<AppState>>,
) -> Json<Option<rf_engine::recording::RecordingSummary>> {
    Json(e.engine.recording.status())
}
async fn recording_start(
    State(e): State<Arc<AppState>>,
) -> Result<Json<rf_engine::recording::RecordingSummary>, ApiError> {
    let _guard = e.mutations.lock().await;
    let status = snapshot(&e).await?;
    let summary = e
        .engine
        .recording
        .start(
            status.state.center_frequency_hz,
            status.state.sample_rate_hz,
            format!(
                "{} {}",
                status.device.descriptor.driver, status.device.descriptor.name
            ),
        )
        .map_err(|error| (StatusCode::BAD_REQUEST, error.to_string()))?;
    if let Some(storage) = &e.engine.storage {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos() as u64);
        if let Err(error) = storage
            .record_session(
                &summary.id,
                &status.source,
                summary.center_frequency_hz,
                summary.sample_rate_hz,
                now,
            )
            .and_then(|_| {
                storage.index_recording(
                    &rf_engine::storage::StoredRecording {
                        id: summary.id.clone(),
                        directory: summary.directory.clone(),
                        sample_rate_hz: summary.sample_rate_hz,
                        center_frequency_hz: summary.center_frequency_hz,
                        created_at_unix_ns: now,
                    },
                    Some(&summary.id),
                )
            })
        {
            tracing::error!(%error, recording_id = %summary.id, "failed to index recording in SQLite");
        }
    }
    Ok(Json(summary))
}
async fn recording_stop(
    State(e): State<Arc<AppState>>,
) -> Result<Json<rf_engine::recording::RecordingSummary>, ApiError> {
    let _guard = e.mutations.lock().await;
    e.engine
        .recording
        .stop()
        .map(Json)
        .map_err(|error| (StatusCode::SERVICE_UNAVAILABLE, error.to_string()))
}
#[derive(Debug, Deserialize)]
struct PlaybackLoadRequest {
    metadata_path: String,
}
#[derive(Debug, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
enum PlaybackRequest {
    Play,
    Pause,
    Seek { position_samples: u64 },
}
async fn playback_status(
    State(e): State<Arc<AppState>>,
) -> Json<Option<rf_engine::playback::PlaybackSummary>> {
    Json(e.engine.playback.summary())
}
async fn playback_load(
    State(e): State<Arc<AppState>>,
    Json(request): Json<PlaybackLoadRequest>,
) -> Result<Json<rf_engine::playback::PlaybackSummary>, ApiError> {
    let _guard = e.mutations.lock().await;
    if e.engine.hardware.load(std::sync::atomic::Ordering::Acquire) {
        return Err((
            StatusCode::CONFLICT,
            "stop the hardware source before loading playback".into(),
        ));
    }
    e.engine
        .playback
        .load(request.metadata_path)
        .map(Json)
        .map_err(|error| (StatusCode::BAD_REQUEST, error.to_string()))
}
async fn playback_command(
    State(e): State<Arc<AppState>>,
    Json(request): Json<PlaybackRequest>,
) -> Result<Json<rf_engine::playback::PlaybackSummary>, ApiError> {
    let _guard = e.mutations.lock().await;
    let result = match request {
        PlaybackRequest::Play => e.engine.playback.play(),
        PlaybackRequest::Pause => e.engine.playback.pause(),
        PlaybackRequest::Seek { position_samples } => e.engine.playback.seek(position_samples),
    };
    result
        .map(Json)
        .map_err(|error| (StatusCode::BAD_REQUEST, error.to_string()))
}
async fn playback_clear(State(e): State<Arc<AppState>>) -> Result<StatusCode, ApiError> {
    let _guard = e.mutations.lock().await;
    e.engine
        .playback
        .clear()
        .map(|()| StatusCode::NO_CONTENT)
        .map_err(|error| (StatusCode::BAD_REQUEST, error.to_string()))
}
async fn vfos(State(e): State<Arc<AppState>>) -> Result<Json<Vec<rf_types::Vfo>>, ApiError> {
    e.engine
        .vfos
        .list()
        .map(Json)
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error))
}
async fn add_vfo(
    State(e): State<Arc<AppState>>,
    Json(config): Json<rf_types::VfoConfiguration>,
) -> Result<Json<rf_types::Vfo>, ApiError> {
    let _guard = e.mutations.lock().await;
    let capture = snapshot(&e).await?.state;
    e.engine
        .vfos
        .put(None, config, &capture)
        .map(Json)
        .map_err(|error| (StatusCode::BAD_REQUEST, error))
}
async fn update_vfo(
    State(e): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(config): Json<rf_types::VfoConfiguration>,
) -> Result<Json<rf_types::Vfo>, ApiError> {
    let _guard = e.mutations.lock().await;
    let capture = snapshot(&e).await?.state;
    e.engine
        .vfos
        .put(Some(&id), config, &capture)
        .map(Json)
        .map_err(|error| (StatusCode::BAD_REQUEST, error))
}
async fn remove_vfo(
    State(e): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let _guard = e.mutations.lock().await;
    e.engine
        .vfos
        .remove(&id)
        .map(|()| StatusCode::NO_CONTENT)
        .map_err(|error| (StatusCode::NOT_FOUND, error))
}
async fn device_status(State(e): State<Arc<AppState>>) -> Result<Json<DeviceSelection>, ApiError> {
    Ok(Json(device_call(&e, |d| d.snapshot()).await?))
}
async fn control(
    State(e): State<Arc<AppState>>,
    Json(command): Json<DeviceCommand>,
) -> Result<Json<Status>, ApiError> {
    let _guard = e.mutations.lock().await;
    // Pause before native selection so a cancelled HTTP request cannot leave mock IQ
    // running underneath a hardware selection that completes on the worker.
    if matches!(&command, DeviceCommand::Select { id } if id != "mock-0") {
        e.engine.state.write().await.running = false;
        e.engine
            .hardware
            .store(true, std::sync::atomic::Ordering::Release);
    }
    let selected = device_call(&e, move |d| d.command(command)).await?;
    e.engine.hardware.store(
        selected.descriptor.driver != "mock",
        std::sync::atomic::Ordering::Release,
    );
    Ok(Json(snapshot(&e).await?))
}
async fn patch_state(
    State(e): State<Arc<AppState>>,
    Json(p): Json<DeviceStatePatch>,
) -> Result<Json<Status>, ApiError> {
    let _guard = e.mutations.lock().await;
    let selected = device_call(&e, |d| d.snapshot()).await?;
    if let Some(size) = p.fft_size {
        if !(1024..=65536).contains(&size) || !size.is_power_of_two() {
            return Err((
                StatusCode::BAD_REQUEST,
                "fft_size must be a power of two from 1024 through 65536".into(),
            ));
        }
    }
    if selected.descriptor.driver != "mock" {
        let reconfigure = p.center_frequency_hz.is_some()
            || p.sample_rate_hz.is_some()
            || p.gains.is_some()
            || p.baseband_filter_bandwidth_hz.is_some();
        let mut configuration = selected.configuration.ok_or_else(|| {
            (
                StatusCode::CONFLICT,
                "open the selected device first".into(),
            )
        })?;
        if let Some(v) = p.center_frequency_hz {
            configuration.center_frequency_hz = v;
        }
        if let Some(v) = p.sample_rate_hz {
            configuration.sample_rate_hz = v;
        }
        if let Some(v) = p.gains {
            configuration.gains = v;
        }
        if let Some(v) = p.baseband_filter_bandwidth_hz {
            configuration.baseband_filter_bandwidth_hz = v;
        }
        if reconfigure {
            device_call(&e, move |d| {
                d.command(DeviceCommand::Configure { configuration })
            })
            .await?;
        }
        if let Some(running) = p.running {
            device_call(&e, move |d| {
                d.command(if running {
                    DeviceCommand::Start
                } else {
                    DeviceCommand::Stop
                })
            })
            .await?;
        }
        if let Some(size) = p.fft_size {
            *e.engine.fft_size.write().await = size;
        }
    } else {
        if p.gains.is_some() || p.baseband_filter_bandwidth_hz.is_some() {
            return Err((
                StatusCode::BAD_REQUEST,
                "mock has no hardware gain or filter controls".into(),
            ));
        }
        if let Some(size) = p.fft_size {
            if !(1024..=65536).contains(&size) || !size.is_power_of_two() {
                return Err((
                    StatusCode::BAD_REQUEST,
                    "fft_size must be a power of two from 1024 through 65536".into(),
                ));
            }
        }
        let mut next = e.engine.state.read().await.clone();
        if let Some(v) = p.center_frequency_hz {
            next.center_frequency_hz = v;
        }
        if let Some(v) = p.sample_rate_hz {
            next.sample_rate_hz = v;
        }
        // Validate with the existing source capability implementation before changing shared state.
        use rf_device::IqSource;
        rf_device::MockSource::default()
            .configure(next.center_frequency_hz, next.sample_rate_hz)
            .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
        if let Some(v) = p.running {
            next.running = v;
        }
        *e.engine.state.write().await = next;
        if let Some(size) = p.fft_size {
            *e.engine.fft_size.write().await = size;
        }
    }
    Ok(Json(snapshot(&e).await?))
}
async fn ws(upgrade: WebSocketUpgrade, State(e): State<Arc<AppState>>) -> impl IntoResponse {
    upgrade.on_upgrade(move |socket| stream(socket, e.engine.clone()))
}
async fn ws_audio(upgrade: WebSocketUpgrade, State(e): State<Arc<AppState>>) -> impl IntoResponse {
    upgrade.on_upgrade(move |socket| stream_audio(socket, e.engine.clone()))
}
async fn stream_audio(mut socket: WebSocket, engine: Arc<Engine>) {
    use std::sync::atomic::Ordering;
    struct Client(Arc<Engine>);
    impl Drop for Client {
        fn drop(&mut self) {
            self.0.vfos.audio.clients.fetch_sub(1, Ordering::Relaxed);
        }
    }
    engine.vfos.audio.clients.fetch_add(1, Ordering::Relaxed);
    let _client = Client(engine.clone());
    let mut frames = engine.vfos.audio.frames.subscribe();
    loop {
        tokio::select! {
            frame=frames.recv() => match frame {
                Ok(frame) => {
                    if !matches!(tokio::time::timeout(std::time::Duration::from_millis(250),socket.send(Message::Binary(frame))).await,Ok(Ok(()))) { break; }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(count)) => {
                    engine.vfos.audio.lagged.fetch_add(count,Ordering::Relaxed);
                    frames=frames.resubscribe();
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            },
            message=socket.recv() => match message { None|Some(Err(_))|Some(Ok(Message::Close(_)))=>break, _=>{} },
            _=tokio::signal::ctrl_c()=>break,
        }
    }
}
async fn stream(mut socket: WebSocket, e: Arc<Engine>) {
    e.client_connected();
    let mut rx = e.frames.subscribe();
    loop {
        tokio::select! {
            frame = rx.recv() => match frame {
                Ok(frame) => if socket.send(Message::Binary(frame)).await.is_err() { break; },
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            },
            message = socket.recv() => match message {
                None | Some(Err(_)) | Some(Ok(Message::Close(_))) => break,
                _ => {},
            },
            _ = tokio::signal::ctrl_c() => break,
        }
    }
    e.client_disconnected();
}

#[cfg(test)]
mod tests {
    use super::*;
    fn state() -> Arc<AppState> {
        Arc::new(AppState {
            engine: Engine::mock(),
            devices: DeviceController::new().unwrap(),
            mutations: Mutex::new(()),
        })
    }
    #[tokio::test]
    async fn vfo_edits_preserve_capture_and_invalid_changes_are_atomic() {
        let e = state();
        let config = rf_types::VfoConfiguration {
            name: "test".into(),
            frequency_hz: 100_000_000,
            mode: rf_types::ReceiverMode::Am,
            bandwidth_hz: 10000,
            squelch_dbfs: None,
            agc: true,
            volume: 1.0,
            mute: false,
            solo: false,
            audio_highpass_hz: 80,
            audio_lowpass_hz: 5000,
        };
        let receiver = add_vfo(State(e.clone()), Json(config.clone()))
            .await
            .unwrap()
            .0;
        let mut tuned = config.clone();
        tuned.frequency_hz += 100_000;
        let updated = update_vfo(
            State(e.clone()),
            Path(receiver.id.clone()),
            Json(tuned.clone()),
        )
        .await
        .unwrap();
        assert_eq!(updated.0.id, receiver.id);
        assert_eq!(
            snapshot(&e).await.unwrap().state.center_frequency_hz,
            config.frequency_hz
        );
        let mut invalid = tuned.clone();
        invalid.frequency_hz += 10_000_000;
        assert!(
            update_vfo(State(e.clone()), Path(receiver.id.clone()), Json(invalid))
                .await
                .is_err()
        );
        assert_eq!(
            vfos(State(e.clone())).await.unwrap().0[0].configuration,
            tuned
        );
        assert_eq!(
            remove_vfo(State(e.clone()), Path(receiver.id))
                .await
                .unwrap(),
            StatusCode::NO_CONTENT
        );
        assert!(vfos(State(e.clone())).await.unwrap().0.is_empty());
        e.devices.shutdown().unwrap();
    }
    #[tokio::test]
    async fn mock_api_preserves_state_on_invalid_patch() {
        let e = state();
        let result = patch_state(
            State(e.clone()),
            Json(DeviceStatePatch {
                center_frequency_hz: Some(101_000_000),
                sample_rate_hz: Some(1),
                fft_size: Some(4096),
                ..Default::default()
            }),
        )
        .await;
        assert_eq!(result.unwrap_err().0, StatusCode::BAD_REQUEST);
        let s = status(State(e.clone())).await.unwrap().0;
        assert_eq!(s.state.center_frequency_hz, 100_000_000);
        assert_eq!(s.fft_size, 2048);
        assert_eq!(s.source, "mock");
        assert!(s.device.supports_iq_streaming);
        e.devices.shutdown().unwrap();
    }
    #[tokio::test]
    async fn mock_api_controls_and_hardware_rejection() {
        let e = state();
        let s = patch_state(
            State(e.clone()),
            Json(DeviceStatePatch {
                running: Some(false),
                center_frequency_hz: Some(101_000_000),
                ..Default::default()
            }),
        )
        .await
        .unwrap()
        .0;
        assert!(!s.state.running);
        assert_eq!(s.state.center_frequency_hz, 101_000_000);
        let error = patch_state(
            State(e.clone()),
            Json(DeviceStatePatch {
                gains: Some(Default::default()),
                ..Default::default()
            }),
        )
        .await
        .unwrap_err();
        assert_eq!(error.0, StatusCode::BAD_REQUEST);
        e.devices.shutdown().unwrap();
    }
}
