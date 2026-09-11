export type AudioFrame = { id:string; epoch:bigint; sequence:bigint; flags:number; samples:Float32Array };
export function decodeAudio(buffer:ArrayBuffer):AudioFrame {
  if (buffer.byteLength<48) throw new Error("truncated audio header");
  const bytes=new Uint8Array(buffer), view=new DataView(buffer);
  if (String.fromCharCode(...bytes.subarray(0,4))!=="RFAU" || view.getUint16(4,true)!==1 || view.getUint16(6,true)!==2) throw new Error("unsupported audio protocol");
  const header=view.getUint32(8,true), rate=view.getUint32(28,true), count=view.getUint32(32,true), idLength=view.getUint16(36,true);
  if (header!==48 || rate!==48000 || count!==960 || idLength<1 || idLength>128 || buffer.byteLength!==header+idLength+count*4) throw new Error("invalid audio frame dimensions");
  const id=new TextDecoder("utf-8",{fatal:true}).decode(bytes.subarray(header,header+idLength));
  const samples=new Float32Array(count);
  for(let i=0;i<count;i++) { const sample=view.getFloat32(header+idLength+i*4,true); if(!Number.isFinite(sample)||Math.abs(sample)>1) throw new Error("invalid PCM sample"); samples[i]=sample; }
  return {id,epoch:view.getBigUint64(20,true),sequence:view.getBigUint64(12,true),flags:view.getUint16(38,true),samples};
}
