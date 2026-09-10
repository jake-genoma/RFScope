// Local mock validation of the versioned PCM audio stream. Node 22+, no dependencies.
import assert from "node:assert/strict";
const base = process.env.RFSCOPE_API ?? "http://127.0.0.1:8789/api/v1";
async function request(path, method, body) {
  const response = await fetch(base + path, { method, headers: { "content-type": "application/json" }, body: body && JSON.stringify(body) });
  const text = await response.text(); assert(response.ok, text); return text ? JSON.parse(text) : null;
}
const vfo = await request("/vfos", "POST", { name: "Audio smoke", frequency_hz: 100000000, mode: "am", bandwidth_hz: 10000, squelch_dbfs: null, agc: true, volume: 1, mute: false, solo: false });
const socket = new WebSocket(base.replace(/^http/, "ws") + "/stream/audio"); socket.binaryType = "arraybuffer";
let frames = 0, failure;
socket.onmessage = ({ data }) => { try {
  const bytes = new Uint8Array(data), view = new DataView(data);
  assert.equal(new TextDecoder().decode(bytes.subarray(0, 4)), "RFAU"); assert.equal(view.getUint16(4, true), 1); assert.equal(view.getUint16(6, true), 2);
  assert.equal(view.getUint32(8, true), 48); const idLength = view.getUint16(36, true);
  assert.equal(view.getUint32(28, true), 48000); assert.equal(view.getUint32(32, true), 960); assert.equal(data.byteLength, 48 + idLength + 960 * 4);
  for (let offset = 48 + idLength; offset < data.byteLength; offset += 4) assert(Math.abs(view.getFloat32(offset, true)) <= 1);
  frames++;
} catch (error) { failure = error; } };
await new Promise(resolve => setTimeout(resolve, 1500));
assert(frames > 3, `only ${frames} PCM frames received`); assert(!failure, failure);
const state = (await request("/vfos", "GET"))[0]; assert.equal(state.audio_rate_hz, 48000); assert(state.audio_samples > 0 && state.audio_frames > 0);
console.log(JSON.stringify({ frames, audio_samples: state.audio_samples, audio_frames: state.audio_frames, audio_peak: state.audio_peak }));
socket.close(); await request(`/vfos/${vfo.id}`, "DELETE");
