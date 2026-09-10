import { API } from "./api";
import { useEffect, useState } from "react";

type Range = { min: number; max: number; step: number };
type Descriptor = { id: string; name: string; driver: string };
export type Configuration = {
  center_frequency_hz: number;
  sample_rate_hz: number;
  gains: Record<string, number>;
  baseband_filter_bandwidth_hz: number;
};
export type DeviceSelection = {
  descriptor: Descriptor;
  capabilities: {
    frequency_hz: Range;
    sample_rate_hz: Range;
    gain_stages: { id: string; label: string; unit: string; range: Range }[];
    baseband_filter_bandwidths_hz: number[];
  };
  metadata: Record<string, string>;
  opened: boolean;
  supports_iq_streaming: boolean;
  configuration: Configuration | null;
};

export function DevicePanel({ device, onChange }: { device?: DeviceSelection; onChange: () => void }) {
  const [devices, setDevices] = useState<Descriptor[]>([]);
  const [warnings, setWarnings] = useState<string[]>([]);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [draft, setDraft] = useState<Configuration | null>(null);
  const serialized = JSON.stringify(device?.configuration ?? null);
  useEffect(() => { setDraft(JSON.parse(serialized) as Configuration | null); }, [serialized, device?.descriptor.id]);
  async function refresh() {
    try {
      const response = await fetch(`${API}/devices`);
      if (!response.ok) throw new Error(await response.text());
      const inventory = await response.json() as { devices: Descriptor[]; warnings: string[] };
      setDevices(inventory.devices); setWarnings(inventory.warnings); setError("");
    } catch (e) { setError(String(e)); }
  }
  useEffect(() => { void refresh(); }, []);
  async function command(body: object) {
    setBusy(true); setError("");
    try {
      const response = await fetch(`${API}/device/control`, { method: "PATCH", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) });
      if (!response.ok) throw new Error(await response.text());
    } catch (e) { setError(String(e)); }
    finally { setBusy(false); onChange(); }
  }
  const hardware = device && device.descriptor.driver !== "mock";
  return <section className="device-panel">
    <label>Device <select aria-label="Device" disabled={busy || device?.opened} value={device?.descriptor.id ?? "mock-0"} onChange={e => void command({ action: "select", id: e.target.value })}>
      {devices.map(d => <option key={d.id} value={d.id}>{d.name}</option>)}
    </select></label>
    <button disabled={busy} onClick={() => void refresh()}>Refresh devices</button>
    {hardware && <>
      <button disabled={busy} onClick={() => void command({ action: device.opened ? "close" : "open" })}>{device.opened ? "Close device" : "Open device"}</button>
      <p><strong>{device.descriptor.name}</strong> · {device.opened ? "Owned" : "Closed"} · Receive-only · use START RX for live IQ.</p>
      <dl>{Object.entries(device.metadata).map(([key, value]) => <div key={key}><dt>{key.replaceAll("_", " ")}</dt><dd>{value}</dd></div>)}</dl>
      {device.opened && draft && <form onSubmit={e => { e.preventDefault(); void command({ action: "configure", configuration: draft }); }}>
        <label>Center frequency (Hz) <input aria-label="Hardware center frequency" type="number" required {...device.capabilities.frequency_hz} value={draft.center_frequency_hz} onChange={e => setDraft({ ...draft, center_frequency_hz: Number(e.target.value) })} /></label>
        <label>Sample rate (Hz) <input aria-label="Hardware sample rate" type="number" required {...device.capabilities.sample_rate_hz} value={draft.sample_rate_hz} onChange={e => setDraft({ ...draft, sample_rate_hz: Number(e.target.value) })} /></label>
        {device.capabilities.gain_stages.map(stage => <label key={stage.id}>{stage.label} ({stage.unit}) <input aria-label={stage.label} type="number" required {...stage.range} value={draft.gains[stage.id]} onChange={e => setDraft({ ...draft, gains: { ...draft.gains, [stage.id]: Number(e.target.value) } })} /></label>)}
        <label>Baseband filter <select aria-label="Baseband filter" value={draft.baseband_filter_bandwidth_hz} onChange={e => setDraft({ ...draft, baseband_filter_bandwidth_hz: Number(e.target.value) })}>{device.capabilities.baseband_filter_bandwidths_hz.map(hz => <option key={hz} value={hz}>{hz / 1e6} MHz</option>)}</select></label>
        <button disabled={busy} type="submit">Apply receiver settings</button>
        <p>Frequency: {device.capabilities.frequency_hz.min.toLocaleString()}–{device.capabilities.frequency_hz.max.toLocaleString()} Hz. Sample rate: {device.capabilities.sample_rate_hz.min / 1e6}–{device.capabilities.sample_rate_hz.max / 1e6} MS/s. Filter must not exceed sample rate. Values show accepted settings, not hardware readback.</p>
      </form>}
    </>}
    {warnings.map(w => <p role="status" key={w}>{w}</p>)}
    {error && <p role="alert">{error}</p>}
  </section>;
}
