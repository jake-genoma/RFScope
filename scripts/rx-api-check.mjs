// Explicit hardware test against an isolated local server. Node 22+, no dependencies.
import assert from 'node:assert/strict';
const base = process.env.RFSCOPE_API ?? 'http://127.0.0.1:8788/api/v1';
async function request(path, body, expected = 200, method = 'PATCH') {
  const response = await fetch(base + path, body === undefined ? undefined : {method, headers: {'content-type':'application/json'}, body: JSON.stringify(body)});
  const text = await response.text();
  assert.equal(response.status, expected, text);
  return expected === 200 ? JSON.parse(text) : text;
}
let frames = 0;
const labels = new Set();
const socket = new WebSocket(base.replace('http', 'ws') + '/stream/spectrum');
socket.binaryType = 'arraybuffer';
let failure;
const createdVfos = [];
socket.onmessage = ({data}) => {
  try {
    const view = new DataView(data);
    assert.equal(Buffer.from(data, 0, 4).toString(), 'RFSP');
    assert.equal(view.getUint16(4, true), 1);
    assert.equal(view.getUint32(8, true), 48);
    assert.equal(data.byteLength, 48 + view.getUint32(40, true)*4);
    for(let i=48;i<data.byteLength;i+=4) assert(Number.isFinite(view.getFloat32(i,true)));
    labels.add(`${view.getBigUint64(28,true)}/${view.getUint32(36,true)}`);
    frames++;
  } catch(error) { failure = error; }
};
const pause = ms => new Promise(resolve => setTimeout(resolve, ms));
try {
  const inventory = await request('/devices');
  const device = inventory.devices.find(d => d.driver === 'hackrf');
  assert(device, 'HackRF not enumerated');
  await request('/device/control', {action:'select', id:device.id});
  await request('/device/control', {action:'open'});
  let status = await request('/device/state', {running:true});
  assert(status.state.running);
  await pause(1000);
  const configuration = status.device.configuration;
  const vfoConfig = {name:'RX verification', frequency_hz:100000000, mode:'am', bandwidth_hz:10000, squelch_dbfs:null, agc:true, volume:1, mute:false, solo:false};
  for (const offset of [0,100000]) {
    const vfo = await request('/vfos', {...vfoConfig, frequency_hz:vfoConfig.frequency_hz+offset}, 200, 'POST');
    createdVfos.push(vfo.id);
  }
  await pause(1000);
  const beforeTune = await request('/status');
  await request(`/vfos/${createdVfos[0]}`, {...vfoConfig, frequency_hz:100050000, bandwidth_hz:12000, mode:'nfm'});
  await pause(500);
  const afterTune = await request('/status');
  assert.deepEqual(afterTune.device.configuration, configuration, 'VFO tuning changed hardware settings');
  assert(afterTune.diagnostics.received_bytes >= beforeTune.diagnostics.received_bytes, 'VFO tuning restarted capture');
  const receivers = await request('/vfos');
  for (const id of createdVfos) {
    const receiver = receivers.find(vfo => vfo.id === id);
    assert(receiver?.processed_samples > 0);
    assert(receiver.demodulated_samples > 0);
    assert(Number.isFinite(receiver.demodulated_peak) && receiver.demodulated_peak <= 1);
  }
  console.log(JSON.stringify({vfoTuningRetainsHardware:true, receivers, diagnostics:afterTune.diagnostics}));
  for (const id of createdVfos.splice(0)) await request(`/vfos/${id}`, {}, 204, 'DELETE');
  await request('/device/state', {center_frequency_hz:101000000});
  await pause(500);
  await request('/device/state', {sample_rate_hz:10000000});
  await pause(500);
  await request('/device/state', {sample_rate_hz:1}, 400);
  status = await request('/status');
  assert.equal(status.state.sample_rate_hz, 10000000);
  assert(status.state.running);
  await request('/device/state', {running:false});
  await pause(100);
  const stoppedFrames = frames;
  await pause(200);
  assert.equal(frames, stoppedFrames, 'frames continue after stop');
  await request('/device/state', {running:true});
  await pause(300);
  status = await request('/status');
  assert(status.diagnostics.received_bytes > 0);
  assert(status.state.running);
  assert(labels.has('100000000/8000000'));
  assert(labels.has('101000000/8000000'));
  assert(labels.has('101000000/10000000'));
  if(failure) throw failure;
  console.log(JSON.stringify({frames, labels:[...labels], diagnostics:status.diagnostics}));
} finally {
  for (const id of createdVfos) await request(`/vfos/${id}`, {}, 204, 'DELETE');
  await request('/device/control', {action:'close'});
  await request('/device/control', {action:'select', id:'mock-0'});
  await request('/device/state', {running:true});
  socket.close();
}
