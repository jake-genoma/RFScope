use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Path, State,
    },
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
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
    engine.enable_event_persistence();
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
        .route("/api/v1/analysis/offsets", get(analysis_offsets))
        .route("/api/v1/analysis/observations", get(analysis_observations))
        .route("/api/v1/analysis/export", post(analysis_export))
        .route("/api/v1/analysis/query", post(analysis_query))
        .route("/api/v1/sessions", get(sessions))
        .route("/api/v1/recordings", get(recordings))
        .route("/api/v1/storage/recordings", get(storage_recordings))
        .route("/api/v1/detections", get(detections))
        .route("/api/v1/workspaces", get(workspaces).post(save_workspace))
        .route(
            "/api/v1/workspaces/{id}",
            axum::routing::delete(delete_workspace),
        )
        .route("/api/v1/bookmarks", get(bookmarks).post(create_bookmark))
        .route(
            "/api/v1/bookmarks/{id}",
            axum::routing::delete(delete_bookmark),
        )
        .route(
            "/api/v1/annotations",
            get(annotations).post(create_annotation),
        )
        .route(
            "/api/v1/annotations/{id}",
            axum::routing::delete(delete_annotation),
        )
        .route("/api/v1/preferences", get(preferences).post(set_preference))
        .route("/api/v1/markers", get(markers).post(add_marker))
        .route("/api/v1/markers/{id}", axum::routing::delete(remove_marker))
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
    state.engine.shutdown_event_persistence();
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
#[derive(serde::Serialize)]
struct FrequencyOffset {
    id: String,
    label: String,
    offset_hz: f64,
}
#[derive(serde::Serialize)]
struct AnalysisOffsets {
    peak_frequency_hz: f64,
    markers: Vec<FrequencyOffset>,
    vfos: Vec<FrequencyOffset>,
}
async fn analysis_offsets(
    State(e): State<Arc<AppState>>,
) -> Result<Json<Option<AnalysisOffsets>>, ApiError> {
    let Some(measurement) = e.engine.latest_measurements.read().await.clone() else {
        return Ok(Json(None));
    };
    let peak_frequency_hz = measurement.peak_frequency_hz;
    let markers = e
        .engine
        .markers
        .read()
        .await
        .iter()
        .map(|marker| FrequencyOffset {
            id: marker.id.clone(),
            label: marker.label.clone(),
            offset_hz: peak_frequency_hz - marker.frequency_hz as f64,
        })
        .collect();
    let vfos = e
        .engine
        .vfos
        .list()
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error))?
        .into_iter()
        .map(|vfo| FrequencyOffset {
            id: vfo.id,
            label: vfo.configuration.name,
            offset_hz: peak_frequency_hz - vfo.configuration.frequency_hz as f64,
        })
        .collect();
    Ok(Json(Some(AnalysisOffsets {
        peak_frequency_hz,
        markers,
        vfos,
    })))
}
async fn analysis_observations(
    State(e): State<Arc<AppState>>,
) -> Json<Vec<rf_engine::observations::Observation>> {
    let rows = e
        .engine
        .observations
        .lock()
        .map(|rows| rows.iter().cloned().collect())
        .unwrap_or_default();
    Json(rows)
}
#[derive(serde::Serialize)]
struct ObservationExport {
    path: String,
    count: usize,
}
async fn analysis_export(
    State(e): State<Arc<AppState>>,
) -> Result<Json<ObservationExport>, ApiError> {
    let rows: Vec<_> = e
        .engine
        .observations
        .lock()
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "observation lock poisoned".into(),
            )
        })?
        .iter()
        .cloned()
        .collect();
    let path =
        std::env::var("RFSCOPE_OBSERVATIONS").unwrap_or_else(|_| "observations.parquet".into());
    rf_engine::observations::write_parquet(&path, &rows)
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    Ok(Json(ObservationExport {
        path,
        count: rows.len(),
    }))
}
#[derive(serde::Deserialize)]
struct AnalysisQueryRequest {
    path: String,
    limit: Option<usize>,
}
async fn analysis_query(
    Json(request): Json<AnalysisQueryRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let sql = rf_engine::duckdb::parquet_query(request.path, request.limit.unwrap_or(1000));
    tokio::task::spawn_blocking(move || rf_engine::duckdb::query(&sql))
        .await
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
        .map(Json)
        .map_err(|error| (StatusCode::SERVICE_UNAVAILABLE, error.to_string()))
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
async fn detections(
    State(e): State<Arc<AppState>>,
) -> Result<Json<Vec<rf_types::SignalEvent>>, ApiError> {
    let mut events = std::collections::BTreeMap::new();
    if let Some(storage) = &e.engine.storage {
        for event in storage
            .signal_events(4096)
            .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
        {
            events.insert(
                event.id.clone(),
                rf_types::SignalEvent {
                    id: event.id,
                    start_frequency_hz: event.start_frequency_hz,
                    end_frequency_hz: event.end_frequency_hz,
                    start_time_unix_ns: event.start_time_unix_ns,
                    end_time_unix_ns: Some(event.end_time_unix_ns),
                    peak_dbfs: event.peak_dbfs,
                    snr_db: event.snr_db,
                },
            );
        }
    }
    if let Ok(live) = e.engine.events.lock() {
        for event in live.events() {
            events.insert(event.id.clone(), event);
        }
    }
    Ok(Json(events.into_values().collect()))
}
async fn workspaces(
    State(e): State<Arc<AppState>>,
) -> Result<Json<Vec<rf_engine::storage::StoredWorkspace>>, ApiError> {
    let Some(storage) = &e.engine.storage else {
        return Ok(Json(Vec::new()));
    };
    storage
        .workspaces()
        .map(Json)
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))
}
async fn save_workspace(
    State(e): State<Arc<AppState>>,
    Json(workspace): Json<rf_engine::storage::StoredWorkspace>,
) -> Result<Json<rf_engine::storage::StoredWorkspace>, ApiError> {
    if workspace.id.trim().is_empty() || workspace.name.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "workspace id and name are required".into(),
        ));
    }
    let Some(storage) = &e.engine.storage else {
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            "persistent storage is unavailable".into(),
        ));
    };
    storage
        .upsert_workspace(&workspace)
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    Ok(Json(workspace))
}
async fn delete_workspace(
    State(e): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let Some(storage) = &e.engine.storage else {
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            "persistent storage is unavailable".into(),
        ));
    };
    if storage
        .remove_workspace(&id)
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
    {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err((StatusCode::NOT_FOUND, "workspace not found".into()))
    }
}
#[derive(serde::Deserialize)]
struct BookmarkRequest {
    recording_id: Option<String>,
    sample_index: u64,
    label: String,
}
async fn bookmarks(
    State(e): State<Arc<AppState>>,
) -> Result<Json<Vec<rf_engine::storage::StoredBookmark>>, ApiError> {
    let Some(storage) = &e.engine.storage else {
        return Ok(Json(Vec::new()));
    };
    storage
        .bookmarks()
        .map(Json)
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))
}
async fn create_bookmark(
    State(e): State<Arc<AppState>>,
    Json(request): Json<BookmarkRequest>,
) -> Result<Json<rf_engine::storage::StoredBookmark>, ApiError> {
    if request.label.trim().is_empty() {
        return Err((StatusCode::BAD_REQUEST, "bookmark label is required".into()));
    }
    let Some(storage) = &e.engine.storage else {
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            "persistent storage is unavailable".into(),
        ));
    };
    storage
        .create_bookmark(&rf_engine::storage::StoredBookmark {
            id: 0,
            recording_id: request.recording_id,
            sample_index: request.sample_index,
            label: request.label,
        })
        .map(Json)
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))
}
async fn delete_bookmark(
    State(e): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<StatusCode, ApiError> {
    let Some(storage) = &e.engine.storage else {
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            "persistent storage is unavailable".into(),
        ));
    };
    if storage
        .remove_bookmark(id)
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
    {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err((StatusCode::NOT_FOUND, "bookmark not found".into()))
    }
}
#[derive(serde::Deserialize)]
struct AnnotationRequest {
    recording_id: Option<String>,
    start_sample: u64,
    end_sample: u64,
    payload_json: String,
}
async fn annotations(
    State(e): State<Arc<AppState>>,
) -> Result<Json<Vec<rf_engine::storage::StoredAnnotation>>, ApiError> {
    let Some(storage) = &e.engine.storage else {
        return Ok(Json(Vec::new()));
    };
    storage
        .annotations()
        .map(Json)
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))
}
async fn create_annotation(
    State(e): State<Arc<AppState>>,
    Json(request): Json<AnnotationRequest>,
) -> Result<Json<rf_engine::storage::StoredAnnotation>, ApiError> {
    if request.end_sample < request.start_sample
        || serde_json::from_str::<serde_json::Value>(&request.payload_json).is_err()
    {
        return Err((
            StatusCode::BAD_REQUEST,
            "annotation range or payload is invalid".into(),
        ));
    }
    let Some(storage) = &e.engine.storage else {
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            "persistent storage is unavailable".into(),
        ));
    };
    storage
        .create_annotation(&rf_engine::storage::StoredAnnotation {
            id: 0,
            recording_id: request.recording_id,
            start_sample: request.start_sample,
            end_sample: request.end_sample,
            payload_json: request.payload_json,
        })
        .map(Json)
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))
}
async fn delete_annotation(
    State(e): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<StatusCode, ApiError> {
    let Some(storage) = &e.engine.storage else {
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            "persistent storage is unavailable".into(),
        ));
    };
    if storage
        .remove_annotation(id)
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
    {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err((StatusCode::NOT_FOUND, "annotation not found".into()))
    }
}
async fn preferences(
    State(e): State<Arc<AppState>>,
) -> Result<Json<Vec<rf_engine::storage::StoredPreference>>, ApiError> {
    let Some(storage) = &e.engine.storage else {
        return Ok(Json(Vec::new()));
    };
    storage
        .preferences()
        .map(Json)
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))
}
async fn set_preference(
    State(e): State<Arc<AppState>>,
    Json(preference): Json<rf_engine::storage::StoredPreference>,
) -> Result<Json<rf_engine::storage::StoredPreference>, ApiError> {
    if preference.key.trim().is_empty()
        || serde_json::from_str::<serde_json::Value>(&preference.value_json).is_err()
    {
        return Err((
            StatusCode::BAD_REQUEST,
            "preference key or JSON value is invalid".into(),
        ));
    }
    let Some(storage) = &e.engine.storage else {
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            "persistent storage is unavailable".into(),
        ));
    };
    storage
        .set_preference(&preference)
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    Ok(Json(preference))
}
async fn markers(State(e): State<Arc<AppState>>) -> Json<Vec<rf_types::SignalMarker>> {
    Json(e.engine.markers.read().await.clone())
}
async fn add_marker(
    State(e): State<Arc<AppState>>,
    Json(marker): Json<rf_types::SignalMarker>,
) -> Result<Json<rf_types::SignalMarker>, ApiError> {
    if marker.id.trim().is_empty() || marker.frequency_hz == 0 {
        return Err((
            StatusCode::BAD_REQUEST,
            "marker id and frequency are required".into(),
        ));
    }
    let mut markers = e.engine.markers.write().await;
    if markers.iter().any(|existing| existing.id == marker.id) {
        return Err((StatusCode::CONFLICT, "marker id already exists".into()));
    }
    markers.push(marker.clone());
    Ok(Json(marker))
}
async fn remove_marker(
    State(e): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let mut markers = e.engine.markers.write().await;
    let before = markers.len();
    markers.retain(|marker| marker.id != id);
    if markers.len() == before {
        return Err((StatusCode::NOT_FOUND, "marker not found".into()));
    }
    Ok(StatusCode::NO_CONTENT)
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
    let requested_patch = serde_json::to_value(&p)
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
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
    let status = snapshot(&e).await?;
    if let Err(error) = e
        .engine
        .recording
        .record_device_state(&status.state, requested_patch)
    {
        tracing::error!(%error, "failed to record SigMF device-setting event");
    }
    Ok(Json(status))
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
    fn persistent_state() -> Arc<AppState> {
        let storage = rf_engine::storage::Storage::in_memory().unwrap();
        Arc::new(AppState {
            engine: Engine::mock_with_storage(Some(Arc::new(storage))),
            devices: DeviceController::new().unwrap(),
            mutations: Mutex::new(()),
        })
    }
    #[tokio::test]
    async fn preferences_are_validated_and_persisted() {
        let e = persistent_state();
        let stored = set_preference(
            State(e.clone()),
            Json(rf_engine::storage::StoredPreference {
                key: "station_label".into(),
                value_json: "\"Lab\"".into(),
            }),
        )
        .await
        .unwrap()
        .0;
        assert_eq!(stored.key, "station_label");
        assert_eq!(preferences(State(e.clone())).await.unwrap().0.len(), 1);
        let invalid = set_preference(
            State(e.clone()),
            Json(rf_engine::storage::StoredPreference {
                key: "station_label".into(),
                value_json: "not json".into(),
            }),
        )
        .await
        .unwrap_err();
        assert_eq!(invalid.0, StatusCode::BAD_REQUEST);
        e.devices.shutdown().unwrap();
    }
    #[tokio::test]
    async fn analysis_offsets_use_current_peak_marker_and_vfo() {
        let e = state();
        *e.engine.latest_measurements.write().await =
            Some(rf_dsp::analysis::SpectrumMeasurements {
                peak_frequency_hz: 100_001_000.0,
                peak_dbfs: -10.0,
                noise_floor_dbfs: -80.0,
                snr_db: 70.0,
                bandwidth_3db_hz: 1.0,
                bandwidth_6db_hz: 2.0,
                occupied_bandwidth_99_hz: 3.0,
                amplitude_mean_dbfs: -60.0,
                amplitude_min_dbfs: -80.0,
                amplitude_max_dbfs: -10.0,
            });
        e.engine.markers.write().await.push(rf_types::SignalMarker {
            id: "marker".into(),
            frequency_hz: 100_000_000,
            label: "reference".into(),
            color: "#fff".into(),
        });
        let config = rf_types::VfoConfiguration {
            name: "receiver".into(),
            frequency_hz: 100_002_000,
            mode: rf_types::ReceiverMode::Am,
            bandwidth_hz: 10_000,
            squelch_dbfs: None,
            agc: true,
            volume: 1.0,
            mute: false,
            solo: false,
            audio_highpass_hz: 80,
            audio_lowpass_hz: 5_000,
        };
        let _ = add_vfo(State(e.clone()), Json(config)).await.unwrap();
        let offsets = analysis_offsets(State(e.clone())).await.unwrap().0.unwrap();
        assert_eq!(offsets.markers[0].offset_hz, 1_000.0);
        assert_eq!(offsets.vfos[0].offset_hz, -1_000.0);
        e.devices.shutdown().unwrap();
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
    #[tokio::test]
    async fn markers_are_validated_and_removed() {
        let e = state();
        let marker = rf_types::SignalMarker {
            id: "m1".into(),
            frequency_hz: 100_000_000,
            label: "carrier".into(),
            color: "#ff0".into(),
        };
        assert_eq!(
            add_marker(State(e.clone()), Json(marker.clone()))
                .await
                .unwrap()
                .0,
            marker
        );
        assert_eq!(markers(State(e.clone())).await.0.len(), 1);
        assert_eq!(
            remove_marker(State(e.clone()), Path("m1".into()))
                .await
                .unwrap(),
            StatusCode::NO_CONTENT
        );
        assert!(remove_marker(State(e.clone()), Path("m1".into()))
            .await
            .is_err());
        e.devices.shutdown().unwrap();
    }
}
