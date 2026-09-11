import React, { useEffect, useRef, useState } from "react";
import { createRoot } from "react-dom/client";
import { API, SPECTRUM_WS } from "./api";
import { DevicePanel, type DeviceSelection } from "./DevicePanel";
import { VfoPanel, type Vfo, type VfoConfiguration } from "./VfoPanel";
import { AudioPlayer } from "./AudioPlayer";
import { decodeSpectrum } from "./protocol";
import { SpectrumRenderer, WaterfallRenderer, type Marker } from "./renderers";
import "./style.css";

type Status = {
  device: DeviceSelection; name: string; version: string; source: string;
  state: { center_frequency_hz: number; sample_rate_hz: number; running: boolean };
  fft_size: number;
  diagnostics: { received_bytes: number; dropped_iq_blocks: number; dropped_iq_bytes: number;
    hardware_streaming: boolean; stream_faults: number; received_samples: number;
    fft_frames: number; dropped_visualization_frames: number; dropped_event_persistence: number; websocket_clients: number };
};
type Recording = { id: string; directory: string; active: boolean; sample_rate_hz: number; center_frequency_hz: number; elapsed_ms: number; bytes_written: number; samples_written: number; queued_blocks: number; dropped_blocks: number; dropped_bytes: number; write_errors: number; metadata_write_errors: number; projected_bytes_per_second: number; available_disk_bytes: number | null; last_error: string | null };
type Playback = { loaded: boolean; metadata_path: string | null; data_path: string | null; session_id: string | null; hardware: string | null; center_frequency_hz: number; sample_rate_hz: number; total_samples: number; position_samples: number; playing: boolean; ended: boolean };
type Analysis = { peak_frequency_hz: number; peak_dbfs: number; noise_floor_dbfs: number; snr_db: number; bandwidth_3db_hz: number; bandwidth_6db_hz: number; occupied_bandwidth_99_hz: number; amplitude_mean_dbfs: number; amplitude_min_dbfs: number; amplitude_max_dbfs: number };
type SignalEvent = { id: string; start_frequency_hz: number; end_frequency_hz: number; start_time_unix_ns: number; end_time_unix_ns: number | null; peak_dbfs: number; snr_db: number };
type Workspace = { id: string; name: string; payload_json: string };
type WorkspacePayload = { center_frequency_hz?: number; sample_rate_hz?: number; vfos?: VfoConfiguration[] };
type Bookmark = { id: number; recording_id: string | null; sample_index: number; label: string };
type Annotation = { id: number; recording_id: string | null; start_sample: number; end_sample: number; payload_json: string };
type StoredRecording = { id: string; directory: string; sample_rate_hz: number; center_frequency_hz: number; created_at_unix_ns: number };
type Offset = { id: string; label: string; offset_hz: number };
type AnalysisOffsets = { peak_frequency_hz: number; markers: Offset[]; vfos: Offset[] };
type Preference = { key: string; value_json: string };
function App() {
  const [view, setView] = useState("Live");
  const [status, setStatus] = useState<Status | null>(null);
  const [vfos, setVfos] = useState<Vfo[]>([]);
  const [recording, setRecording] = useState<Recording | null>(null);
  const [playback, setPlayback] = useState<Playback | null>(null);
  const [playbackPath, setPlaybackPath] = useState("");
  const [analysis, setAnalysis] = useState<Analysis | null>(null);
  const [markers, setMarkers] = useState<Marker[]>([]);
  const [detections, setDetections] = useState<SignalEvent[]>([]);
  const [workspaces, setWorkspaces] = useState<Workspace[]>([]);
  const [workspaceName, setWorkspaceName] = useState("Live");
  const [bookmarks, setBookmarks] = useState<Bookmark[]>([]);
  const [bookmarkLabel, setBookmarkLabel] = useState("Bookmark");
  const [annotations, setAnnotations] = useState<Annotation[]>([]);
  const [annotationLabel, setAnnotationLabel] = useState("Annotation");
  const [annotationEnd, setAnnotationEnd] = useState<number | null>(null);
  const [storedRecordings, setStoredRecordings] = useState<StoredRecording[]>([]);
  const [offsets, setOffsets] = useState<AnalysisOffsets | null>(null);
  const [stationLabel, setStationLabel] = useState("");
  const [recordingBusy, setRecordingBusy] = useState(false);
  const [error, setError] = useState("");
  const selectedDevice = useRef<DeviceSelection | undefined>(undefined);
  selectedDevice.current = status?.device;
  const currentVfos = useRef<Vfo[]>([]); currentVfos.current = vfos;
  const currentMarkers = useRef<Marker[]>([]); currentMarkers.current = markers;
  const spectrum = useRef<HTMLCanvasElement>(null), waterfall = useRef<HTMLCanvasElement>(null);
  const renderer = useRef<SpectrumRenderer | null>(null);
  const lastFrame = useRef<ReturnType<typeof decodeSpectrum> | null>(null);
  async function refresh() {
    try {
      const [stateResponse, vfoResponse, recordingResponse, playbackResponse, analysisResponse, markerResponse, detectionResponse, workspaceResponse, bookmarkResponse, annotationResponse, storedRecordingResponse, offsetResponse, preferenceResponse] = await Promise.all([fetch(`${API}/status`), fetch(`${API}/vfos`), fetch(`${API}/recording`), fetch(`${API}/playback`), fetch(`${API}/analysis`), fetch(`${API}/markers`), fetch(`${API}/detections`), fetch(`${API}/workspaces`), fetch(`${API}/bookmarks`), fetch(`${API}/annotations`), fetch(`${API}/storage/recordings`), fetch(`${API}/analysis/offsets`), fetch(`${API}/preferences`)]);
      if (!stateResponse.ok || !vfoResponse.ok || !recordingResponse.ok || !playbackResponse.ok || !analysisResponse.ok || !markerResponse.ok || !detectionResponse.ok || !workspaceResponse.ok || !bookmarkResponse.ok || !annotationResponse.ok || !storedRecordingResponse.ok || !offsetResponse.ok || !preferenceResponse.ok) throw new Error("Status request failed");
      const preferences = await preferenceResponse.json() as Preference[];
      const station = preferences.find(preference => preference.key === "station_label");
      if (station) {
        try {
          const value = JSON.parse(station.value_json);
          if (typeof value === "string") setStationLabel(value);
        } catch { /* storage validates JSON; tolerate manually damaged legacy rows. */ }
      }
      setStatus(await stateResponse.json() as Status); setVfos(await vfoResponse.json() as Vfo[]); setRecording(await recordingResponse.json() as Recording | null); setPlayback(await playbackResponse.json() as Playback | null); setAnalysis(await analysisResponse.json() as Analysis | null); setMarkers(await markerResponse.json() as Marker[]); setDetections(await detectionResponse.json() as SignalEvent[]); setWorkspaces(await workspaceResponse.json() as Workspace[]); setBookmarks(await bookmarkResponse.json() as Bookmark[]); setAnnotations(await annotationResponse.json() as Annotation[]); setStoredRecordings(await storedRecordingResponse.json() as StoredRecording[]); setOffsets(await offsetResponse.json() as AnalysisOffsets | null);
    } catch (error) { setError(String(error)); }
  }
  useEffect(() => {
    let alive = true;
    const poll = async () => {
      try {
        const [stateResponse, vfoResponse, recordingResponse, playbackResponse, analysisResponse, markerResponse, detectionResponse, workspaceResponse] = await Promise.all([fetch(`${API}/status`), fetch(`${API}/vfos`), fetch(`${API}/recording`), fetch(`${API}/playback`), fetch(`${API}/analysis`), fetch(`${API}/markers`), fetch(`${API}/detections`), fetch(`${API}/workspaces`)]);
        if (!stateResponse.ok || !vfoResponse.ok || !recordingResponse.ok || !playbackResponse.ok || !analysisResponse.ok || !markerResponse.ok || !detectionResponse.ok || !workspaceResponse.ok) throw new Error("Status request failed");
        const state = await stateResponse.json() as Status, receivers = await vfoResponse.json() as Vfo[], currentRecording = await recordingResponse.json() as Recording | null, currentPlayback = await playbackResponse.json() as Playback | null, currentAnalysis = await analysisResponse.json() as Analysis | null, currentMarkers = await markerResponse.json() as Marker[], currentDetections = await detectionResponse.json() as SignalEvent[], savedWorkspaces = await workspaceResponse.json() as Workspace[];
        if (alive) { setStatus(state); setVfos(receivers); setRecording(currentRecording); setPlayback(currentPlayback); setAnalysis(currentAnalysis); setMarkers(currentMarkers); setDetections(currentDetections); setWorkspaces(savedWorkspaces); }
      } catch { if (alive) setError("Backend offline — run `just demo`"); }
    };
    void poll(); const timer = setInterval(() => void poll(), 1000);
    let s: SpectrumRenderer, w: WaterfallRenderer;
    try {
      s = new SpectrumRenderer(spectrum.current!); w = new WaterfallRenderer(waterfall.current!); renderer.current = s;
    } catch (error) { setError(String(error)); return () => { alive = false; clearInterval(timer); }; }
    const ws = new WebSocket(SPECTRUM_WS); ws.binaryType = "arraybuffer";
    ws.onmessage = event => {
      try {
        if (selectedDevice.current && !selectedDevice.current.supports_iq_streaming) return;
        const frame = decodeSpectrum(event.data as ArrayBuffer); lastFrame.current = frame;
        s.draw(frame.bins); s.overlays(Number(frame.centerHz), frame.sampleRateHz, currentVfos.current, currentMarkers.current); w.push(frame.bins);
      } catch (error) { setError(String(error)); }
    };
    return () => { alive = false; clearInterval(timer); ws.close(); renderer.current = null; };
  }, []);
  useEffect(() => {
    const frame = lastFrame.current;
    if (frame && renderer.current) { renderer.current.draw(frame.bins); renderer.current.overlays(Number(frame.centerHz), frame.sampleRateHz, vfos, markers); }
  }, [vfos, markers]);
  useEffect(() => {
    const tune = (event: KeyboardEvent) => {
      const target = event.target as HTMLElement | null;
      if (target?.matches("input, select, textarea") || (event.key !== "ArrowLeft" && event.key !== "ArrowRight")) return;
      if (!status?.state.center_frequency_hz) return;
      event.preventDefault();
      void patch({ center_frequency_hz: Math.max(100_000, status.state.center_frequency_hz + (event.key === "ArrowRight" ? 1_000 : -1_000)) });
    };
    window.addEventListener("keydown", tune);
    return () => window.removeEventListener("keydown", tune);
  }, [status?.state.center_frequency_hz]);
  async function patch(body: object) {
    try {
      const response = await fetch(`${API}/device/state`, { method: "PATCH", headers: { "content-type": "application/json" }, body: JSON.stringify(body) });
      if (!response.ok) throw new Error(await response.text());
      setStatus(await response.json() as Status); setError("");
    } catch (error) { setError(String(error)); }
  }
  async function recordingAction(method: "POST" | "DELETE") {
    setRecordingBusy(true); setError("");
    try { const response = await fetch(`${API}/recording`, { method }); if (!response.ok) throw new Error(await response.text()); setRecording(method === "POST" ? await response.json() as Recording : null); }
    catch (error) { setError(String(error)); }
    finally { setRecordingBusy(false); }
  }
  async function playbackAction(method: "POST" | "PATCH" | "DELETE", body?: object) {
    try { const response = await fetch(`${API}/playback`, { method, headers: body ? { "content-type": "application/json" } : undefined, body: body ? JSON.stringify(body) : undefined }); if (!response.ok && response.status !== 204) throw new Error(await response.text()); if (response.status !== 204) setPlayback(await response.json() as Playback); setError(""); }
    catch (error) { setError(String(error)); }
  }
  async function addPeakMarker() {
    if (!analysis) return;
    try {
      const marker: Marker = { id: `marker-${Date.now()}`, frequency_hz: Math.round(analysis.peak_frequency_hz), label: "Peak", color: "#ffcf4a" };
      const response = await fetch(`${API}/markers`, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(marker) });
      if (!response.ok) throw new Error(await response.text());
      setMarkers([...markers, marker]);
    } catch (error) { setError(String(error)); }
  }
  async function deleteMarker(id: string) {
    try {
      const response = await fetch(`${API}/markers/${encodeURIComponent(id)}`, { method: "DELETE" });
      if (!response.ok && response.status !== 404) throw new Error(await response.text());
      setMarkers(markers.filter(marker => marker.id !== id)); setError("");
    } catch (error) { setError(String(error)); }
  }
  async function saveWorkspace() {
    try {
      const name = workspaceName.trim();
      if (!name) throw new Error("Workspace name is required");
      const response = await fetch(`${API}/workspaces`, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ id: `workspace-${Date.now()}`, name, payload_json: JSON.stringify({ center_frequency_hz: status?.state.center_frequency_hz, sample_rate_hz: status?.state.sample_rate_hz, vfos: vfos.map(vfo => vfo.configuration) }) }) });
      if (!response.ok) throw new Error(await response.text());
      await refresh();
      setError("");
    } catch (error) { setError(String(error)); }
  }
  async function loadWorkspace(workspace: Workspace) {
    try {
      const payload = JSON.parse(workspace.payload_json) as WorkspacePayload;
      const response = await fetch(`${API}/device/state`, { method: "PATCH", headers: { "content-type": "application/json" }, body: JSON.stringify({ center_frequency_hz: payload.center_frequency_hz, sample_rate_hz: payload.sample_rate_hz }) });
      if (!response.ok) throw new Error(await response.text());
      for (const vfo of vfos) {
        const remove = await fetch(`${API}/vfos/${encodeURIComponent(vfo.id)}`, { method: "DELETE" });
        if (!remove.ok) throw new Error(await remove.text());
      }
      for (const configuration of payload.vfos ?? []) {
        const add = await fetch(`${API}/vfos`, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(configuration) });
        if (!add.ok) throw new Error(await add.text());
      }
      await refresh(); setError("");
    } catch (error) { setError(String(error)); }
  }
  async function deleteWorkspace(id: string) {
    try {
      const response = await fetch(`${API}/workspaces/${encodeURIComponent(id)}`, { method: "DELETE" });
      if (!response.ok && response.status !== 404) throw new Error(await response.text());
      await refresh(); setError("");
    } catch (error) { setError(String(error)); }
  }
  async function saveBookmark() {
    if (!playback?.loaded || !bookmarkLabel.trim()) return;
    try {
      const response = await fetch(`${API}/bookmarks`, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ recording_id: playback.session_id, sample_index: playback.position_samples, label: bookmarkLabel.trim() }) });
      if (!response.ok) throw new Error(await response.text());
      await refresh(); setError("");
    } catch (error) { setError(String(error)); }
  }
  async function deleteBookmark(id: number) {
    try {
      const response = await fetch(`${API}/bookmarks/${id}`, { method: "DELETE" });
      if (!response.ok && response.status !== 404) throw new Error(await response.text());
      await refresh(); setError("");
    } catch (error) { setError(String(error)); }
  }
  async function saveAnnotation() {
    if (!playback?.loaded || !annotationLabel.trim()) return;
    const start_sample = playback.position_samples;
    const end_sample = Math.max(start_sample, annotationEnd ?? start_sample);
    try {
      const response = await fetch(`${API}/annotations`, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ recording_id: playback.session_id, start_sample, end_sample, payload_json: JSON.stringify({ label: annotationLabel.trim() }) }) });
      if (!response.ok) throw new Error(await response.text());
      await refresh(); setError("");
    } catch (error) { setError(String(error)); }
  }
  async function deleteAnnotation(id: number) {
    try {
      const response = await fetch(`${API}/annotations/${id}`, { method: "DELETE" });
      if (!response.ok && response.status !== 404) throw new Error(await response.text());
      await refresh(); setError("");
    } catch (error) { setError(String(error)); }
  }
  async function loadStoredRecording(recording: StoredRecording) {
    const metadata_path = `${recording.directory}/${recording.id}.sigmf-meta`;
    setPlaybackPath(metadata_path);
    await playbackAction("POST", { metadata_path });
    setView("Playback");
  }
  async function saveStationLabel() {
    try {
      const response = await fetch(`${API}/preferences`, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ key: "station_label", value_json: JSON.stringify(stationLabel) }) });
      if (!response.ok) throw new Error(await response.text());
      setError("");
    } catch (error) { setError(String(error)); }
  }
  const visible = !status?.device || status.device.supports_iq_streaming;
  return <main>
    <header><b>RF<span>Scope</span></b><nav>{["Live", "Receivers", "Recordings", "Playback", "Signals", "Analysis", "Workspaces", "Diagnostics", "Settings"].map(name => <button key={name} onClick={() => setView(name)} className={view === name ? "active" : ""}>{name}</button>)}</nav></header>
    <DevicePanel device={status?.device} onChange={() => void refresh()} />
    <section className="device-panel"><h3>Playback</h3><input aria-label="SigMF metadata path" value={playbackPath} onChange={e => setPlaybackPath(e.target.value)} placeholder="/path/to/capture.sigmf-meta" /><button onClick={() => void playbackAction("POST", { metadata_path: playbackPath })} disabled={!playbackPath}>Load</button>{playback?.loaded && <><button onClick={() => void playbackAction("PATCH", { action: playback.playing ? "pause" : "play" })}>{playback.playing ? "Pause" : "Play"}</button><label>Seek sample <input key={`${playback.session_id}-${playback.position_samples}`} aria-label="Playback sample position" type="number" min={0} max={playback.total_samples} defaultValue={playback.position_samples} onBlur={e => void playbackAction("PATCH", { action: "seek", position_samples: Number(e.target.value) })} /></label><button onClick={() => void playbackAction("DELETE")}>Eject</button><span> {playback.session_id} · {playback.position_samples.toLocaleString()} / {playback.total_samples.toLocaleString()} samples</span></>}</section>
    {error && <aside role="alert">{error}</aside>}
    <div hidden={!visible || view !== "Live"}>
      <section className="status">
        <i className={status?.state.running ? "on" : ""} /><strong>{status?.source ?? "offline"}</strong>
        <label>Center <input type="number" value={status?.state.center_frequency_hz ?? 100000000} step="1000" onChange={e => void patch({ center_frequency_hz: Number(e.target.value) })} /></label>
        <label>Rate <select value={status?.state.sample_rate_hz ?? 2000000} onChange={e => {
          const rate = Number(e.target.value), filter = status?.device.capabilities.baseband_filter_bandwidths_hz.filter(hz => hz <= rate).at(-1);
          void patch({ sample_rate_hz: rate, ...(status?.source !== "mock" && filter ? { baseband_filter_bandwidth_hz: Math.min(status?.device.configuration?.baseband_filter_bandwidth_hz ?? filter, filter) } : {}) });
        }}>{[2000000, 8000000, 10000000, 20000000].map(rate => <option key={rate} value={rate}>{rate / 1e6} MS/s</option>)}</select></label>
        <label>FFT <select value={status?.fft_size ?? 2048} onChange={e => void patch({ fft_size: Number(e.target.value) })}>{[1024, 2048, 4096, 8192, 16384].map(size => <option key={size}>{size}</option>)}</select></label>
        <button className="rx" onClick={() => void patch({ running: !status?.state.running })}>{status?.state.running ? "STOP RX" : "START RX"}</button>
        <small>←/→ tune 1 kHz</small>
      </section>
      <section className="scope"><div className="title">SPECTRUM <small>dBFS</small></div><canvas ref={spectrum} /><div className="axis"><span>− BW/2</span><span>{((status?.state.center_frequency_hz ?? 0) / 1e6).toFixed(3)} MHz</span><span>+ BW/2</span></div></section>
      <section className="scope waterfall"><div className="title">WATERFALL <small>newest at top</small></div><canvas ref={waterfall} /></section>
      <VfoPanel vfos={vfos} center={status?.state.center_frequency_hz ?? 0} refresh={() => void refresh()} />
      <AudioPlayer ids={vfos.map(vfo => vfo.id)} />
      <section className="device-panel"><button disabled={recordingBusy || !status?.state.running} onClick={() => void recordingAction(recording ? "DELETE" : "POST")}>{recording ? "Stop recording" : "Start IQ recording"}</button>{recording && <span> {recording.id} · {(recording.bytes_written / 1e6).toFixed(1)} MB · {(recording.elapsed_ms / 1000).toFixed(1)} s · queue {recording.queued_blocks} · drops {recording.dropped_blocks} · metadata errors {recording.metadata_write_errors}</span>}<p>SigMF ci8_le · projected {(recording?.projected_bytes_per_second ?? ((status?.state.sample_rate_hz ?? 0) * 2)) / 1e6} MB/s · disk {recording?.available_disk_bytes == null ? "unknown" : `${(recording.available_disk_bytes / 1e9).toFixed(1)} GB free`}. Recording writes raw IQ on a bounded worker.</p></section>
      <section className="panels"><article><h3>Signal information</h3><p>{status?.source === "mock" ? "Deterministic scene: CW −300 kHz · AM center · NFM +400 kHz" : "Live receiver · uncalibrated dBFS"}</p></article>
        <article><h3>Diagnostics</h3><dl>{Object.entries(status?.diagnostics ?? {}).map(([name, value]) => <React.Fragment key={name}><dt>{name.replaceAll("_", " ")}</dt><dd>{typeof value === "number" ? value.toLocaleString() : String(value)}</dd></React.Fragment>)}</dl></article></section>
    </div>
    {view !== "Live" && <section className="panels workstation-view">
      {view === "Receivers" && <article><h3>Receivers</h3><VfoPanel vfos={vfos} center={status?.state.center_frequency_hz ?? 0} refresh={() => void refresh()} /></article>}
      {view === "Recordings" && <article><h3>Recordings</h3><p>{recording ? `${recording.id} · ${(recording.bytes_written / 1e6).toFixed(1)} MB · ${recording.dropped_blocks} dropped blocks` : "No active recording"}</p>{storedRecordings.length ? <dl>{storedRecordings.map(stored => <React.Fragment key={stored.id}><dt>{stored.id} · {(stored.center_frequency_hz / 1e6).toFixed(6)} MHz · {(stored.sample_rate_hz / 1e6).toFixed(1)} MS/s</dt><dd><button onClick={() => void loadStoredRecording(stored)}>Load in Playback</button></dd></React.Fragment>)}</dl> : <p>No indexed recordings.</p>}</article>}
      {view === "Playback" && <article><h3>Playback timeline</h3><p>{playback?.loaded ? `${playback.session_id} · ${playback.position_samples.toLocaleString()} / ${playback.total_samples.toLocaleString()} samples` : "Load a SigMF metadata file above."}</p>{playback?.loaded && <><label>Bookmark <input aria-label="Bookmark label" value={bookmarkLabel} maxLength={256} onChange={event => setBookmarkLabel(event.target.value)} /></label><button onClick={() => void saveBookmark()}>Save at current sample</button><dl>{bookmarks.filter(bookmark => bookmark.recording_id === playback.session_id).map(bookmark => <React.Fragment key={bookmark.id}><dt>{bookmark.label} · {bookmark.sample_index.toLocaleString()}</dt><dd><button onClick={() => void playbackAction("PATCH", { action: "seek", position_samples: bookmark.sample_index })}>Seek</button><button onClick={() => void deleteBookmark(bookmark.id)}>Delete</button></dd></React.Fragment>)}</dl><label>Annotation <input aria-label="Annotation label" value={annotationLabel} maxLength={256} onChange={event => setAnnotationLabel(event.target.value)} /></label><label>End sample <input aria-label="Annotation end sample" type="number" min={playback.position_samples} max={playback.total_samples} value={annotationEnd ?? playback.position_samples} onChange={event => setAnnotationEnd(Number(event.target.value))} /></label><button onClick={() => void saveAnnotation()}>Save range</button><dl>{annotations.filter(annotation => annotation.recording_id === playback.session_id).map(annotation => <React.Fragment key={annotation.id}><dt>{(() => { try { return (JSON.parse(annotation.payload_json) as { label?: string }).label ?? "Annotation"; } catch { return "Annotation"; } })()} · {annotation.start_sample.toLocaleString()}–{annotation.end_sample.toLocaleString()}</dt><dd><button onClick={() => void playbackAction("PATCH", { action: "seek", position_samples: annotation.start_sample })}>Seek</button><button onClick={() => void deleteAnnotation(annotation.id)}>Delete</button></dd></React.Fragment>)}</dl></>}</article>}
      {view === "Signals" && <article><h3>Signals</h3>{detections.length ? <dl>{detections.map(event => <React.Fragment key={event.id}><dt>{event.id} · {(event.start_frequency_hz / 1e6).toFixed(6)} MHz</dt><dd>{event.end_time_unix_ns == null ? "active" : "complete"} · {event.snr_db.toFixed(1)} dB SNR · {event.peak_dbfs.toFixed(1)} dBFS</dd></React.Fragment>)}</dl> : <p>No threshold events yet.</p>}</article>}
      {view === "Analysis" && <article><h3>Analysis</h3>{analysis ? <><button onClick={() => void addPeakMarker()}>Mark current peak</button><dl>{Object.entries(analysis).map(([key, value]) => <React.Fragment key={key}><dt>{key.replaceAll("_", " ")}</dt><dd>{value.toLocaleString(undefined, { maximumFractionDigits: 2 })}</dd></React.Fragment>)}</dl><p>{markers.length} marker{markers.length === 1 ? "" : "s"} active</p><dl>{markers.map(marker => <React.Fragment key={marker.id}><dt>{marker.label} · {(marker.frequency_hz / 1e6).toFixed(6)} MHz</dt><dd><button onClick={() => void deleteMarker(marker.id)}>Remove</button></dd></React.Fragment>)}</dl>{offsets && <><h4>Peak offsets</h4><dl>{[...offsets.markers, ...offsets.vfos].map(offset => <React.Fragment key={offset.id}><dt>{offset.label}</dt><dd>{offset.offset_hz.toLocaleString(undefined, { maximumFractionDigits: 1 })} Hz</dd></React.Fragment>)}</dl></>}</> : <p>Waiting for a spectrum frame.</p>}</article>}
      {view === "Workspaces" && <article><h3>Workspaces</h3><label>Name <input aria-label="Workspace name" value={workspaceName} maxLength={128} onChange={event => setWorkspaceName(event.target.value)} /></label><button onClick={() => void saveWorkspace()}>Save current workspace</button><p>Restoring applies capture settings and recreates saved VFO configurations with fresh runtime IDs.</p>{workspaces.length ? <dl>{workspaces.map(workspace => <React.Fragment key={workspace.id}><dt>{workspace.name}</dt><dd><button onClick={() => void loadWorkspace(workspace)}>Load</button><button onClick={() => void deleteWorkspace(workspace.id)}>Delete</button></dd></React.Fragment>)}</dl> : <p>No saved workspaces.</p>}</article>}
      {view === "Diagnostics" && <article><h3>Diagnostics</h3><dl>{Object.entries(status?.diagnostics ?? {}).map(([name, value]) => <React.Fragment key={name}><dt>{name.replaceAll("_", " ")}</dt><dd>{String(value)}</dd></React.Fragment>)}</dl></article>}
      {view === "Settings" && <article><h3>Settings</h3><p>Server: {API} · source: {status?.source ?? "offline"}</p><label>Station label <input aria-label="Station label" value={stationLabel} maxLength={128} onChange={event => setStationLabel(event.target.value)} /></label><button onClick={() => void saveStationLabel()}>Save</button></article>}
    </section>}
  </main>;
}
createRoot(document.getElementById("root")!).render(<React.StrictMode><App /></React.StrictMode>);
