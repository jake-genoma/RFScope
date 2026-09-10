export interface SpectrumFrame { sequence: bigint; timestampNs: bigint; centerHz: bigint; sampleRateHz: number; bins: Float32Array }
export function decodeSpectrum(buffer: ArrayBuffer): SpectrumFrame {
  if (buffer.byteLength < 48) throw new Error("truncated spectrum frame");
  const bytes = new Uint8Array(buffer); if (String.fromCharCode(...bytes.slice(0, 4)) !== "RFSP") throw new Error("invalid spectrum magic");
  const view = new DataView(buffer); if (view.getUint16(4, true) !== 1 || view.getUint16(6, true) !== 1) throw new Error("unsupported spectrum protocol");
  const header = view.getUint32(8, true), count = view.getUint32(40, true); if (header !== 48 || buffer.byteLength !== header + count * 4) throw new Error("invalid spectrum frame length");
  return { sequence:view.getBigUint64(12,true), timestampNs:view.getBigUint64(20,true), centerHz:view.getBigUint64(28,true), sampleRateHz:view.getUint32(36,true), bins:new Float32Array(buffer,header,count) };
}
