import { useEffect, useState } from "react";
import { API } from "./api";

export type VfoConfiguration = {
  name: string; frequency_hz: number; mode: "am" | "nfm" | "usb" | "lsb";
  bandwidth_hz: number; squelch_dbfs: number | null; agc: boolean;
  volume: number; mute: boolean; solo: boolean; audio_highpass_hz: number; audio_lowpass_hz: number;
};
export type Vfo = {
  id: string; configuration: VfoConfiguration; recording: boolean;
  output_rate_hz: number; processed_samples: number; channel_power_dbfs: number;
  demodulated_samples: number; demodulated_peak: number;
  suspended_reason: string | null;
};
export function newVfo(frequency_hz: number): VfoConfiguration {
  return { name: "Receiver", frequency_hz, mode: "am", bandwidth_hz: 10000,
    squelch_dbfs: null, agc: true, volume: 1, mute: false, solo: false, audio_highpass_hz: 80, audio_lowpass_hz: 5000 };
}
async function change(path: string, method: string, configuration?: VfoConfiguration) {
  const response = await fetch(`${API}/vfos${path}`, { method,
    headers: { "content-type": "application/json" }, body: configuration ? JSON.stringify(configuration) : undefined });
  if (!response.ok) throw new Error(await response.text());
}
function Receiver({ vfo, refresh, report }: { vfo: Vfo; refresh: () => void; report: (error: string) => void }) {
  const [draft, setDraft] = useState(vfo.configuration);
  const serialized = JSON.stringify(vfo.configuration);
  useEffect(() => setDraft(JSON.parse(serialized) as VfoConfiguration), [serialized]);
  const [busy, setBusy] = useState(false);
  async function save(remove = false) {
    setBusy(true);
    try { await change(`/${encodeURIComponent(vfo.id)}`, remove ? "DELETE" : "PATCH", remove ? undefined : draft); report(""); refresh(); }
    catch (error) { report(String(error)); }
    finally { setBusy(false); }
  }
  return <form className="vfo-row" onSubmit={event => { event.preventDefault(); void save(); }}>
    <label>Name <input aria-label="Receiver name" value={draft.name} maxLength={128} required onChange={e => setDraft({ ...draft, name: e.target.value })} /></label>
    <label>Frequency (Hz) <input aria-label="VFO frequency" type="number" value={draft.frequency_hz} step={100} required onChange={e => setDraft({ ...draft, frequency_hz: Number(e.target.value) })} /></label>
    <label>Mode <select value={draft.mode} onChange={e => setDraft({ ...draft, mode: e.target.value as VfoConfiguration["mode"] })}>{["am", "nfm", "usb", "lsb"].map(mode => <option key={mode} value={mode}>{mode.toUpperCase()}</option>)}</select></label>
    <label>Bandwidth (Hz) <input aria-label="VFO bandwidth" type="number" min={500} max={100000} step={100} value={draft.bandwidth_hz} required onChange={e => setDraft({ ...draft, bandwidth_hz: Number(e.target.value) })} /></label>
    <label>Volume <input aria-label="VFO volume" type="number" min={0} max={2} step={0.05} value={draft.volume} onChange={e => setDraft({ ...draft, volume: Number(e.target.value) })} /></label>
    <label><input aria-label="VFO mute" type="checkbox" checked={draft.mute} onChange={e => setDraft({ ...draft, mute: e.target.checked })} /> Mute</label>
    <label><input aria-label="VFO solo" type="checkbox" checked={draft.solo} onChange={e => setDraft({ ...draft, solo: e.target.checked })} /> Solo</label>
    <label>Squ elch dBFS <input aria-label="VFO squelch" type="number" min={-120} max={0} value={draft.squelch_dbfs ?? ""} onChange={e => setDraft({ ...draft, squelch_dbfs: e.target.value === "" ? null : Number(e.target.value) })} /></label>
    <button disabled={busy}>Apply VFO</button><button type="button" disabled={busy} onClick={() => void save(true)}>Remove</button>
    <small>{vfo.suspended_reason ?? `${vfo.output_rate_hz.toLocaleString()} samples/s channel · ${vfo.channel_power_dbfs.toFixed(1)} dBFS · ${vfo.demodulated_samples.toLocaleString()} audio samples · peak ${vfo.demodulated_peak.toFixed(2)}`}</small>
  </form>;
}
export function VfoPanel({ vfos, center, refresh }: { vfos: Vfo[]; center: number; refresh: () => void }) {
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  async function add() {
    setBusy(true);
    try { await change("", "POST", newVfo(center)); setError(""); refresh(); }
    catch (error) { setError(String(error)); }
    finally { setBusy(false); }
  }
  return <section className="device-panel"><h3>Receivers / VFOs</h3>
    <button disabled={busy || !center} onClick={() => void add()}>Add VFO at center</button>
    <p>VFO tuning stays within the captured spectrum. AM, NFM, USB and LSB demodulation is active; browser playback is the next milestone.</p>
    {vfos.map(vfo => <Receiver key={vfo.id} vfo={vfo} refresh={refresh} report={setError} />)}
    {error && <p role="alert">{error}</p>}
  </section>;
}
