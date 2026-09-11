import { useEffect,useRef,useState } from "react";
import { API } from "./api";
import { decodeAudio } from "./audio-protocol";
import workletUrl from "./audio-worklet.ts?worker&url";
type Health={id:string;buffered:number;underruns:number;overruns:number;discontinuities:number};
export function AudioPlayer({ids}:{ids:string[]}) {
  const [enabled,setEnabled]=useState(false),[error,setError]=useState("");
  const [health,setHealth]=useState<Health[]>([]),[dropped,setDropped]=useState(0);
  const context=useRef<AudioContext|null>(null),node=useRef<AudioWorkletNode|null>(null),socket=useRef<WebSocket|null>(null);
  const activeIds=useRef(ids); activeIds.current=ids;
  const generation=useRef(0),[busy,setBusy]=useState(false);
  const serialized=JSON.stringify(ids);
  useEffect(()=>{node.current?.port.postMessage({type:"configure",ids:JSON.parse(serialized)});},[serialized]);
  function stop() {
    generation.current++; socket.current?.close(); socket.current=null;
    node.current?.disconnect(); node.current?.port.close(); node.current=null;
    void context.current?.close(); context.current=null;
    setEnabled(false); setHealth([]);
  }
  useEffect(()=>()=>{generation.current++;socket.current?.close();node.current?.disconnect();node.current?.port.close();void context.current?.close();},[]);
  async function start() {
    setBusy(true); setError(""); const run=++generation.current;
    try {
      const audio=new AudioContext({sampleRate:48000}); context.current=audio;
      await audio.resume(); await audio.audioWorklet.addModule(workletUrl);
      if(generation.current!==run) { await audio.close(); return; }
      const player=new AudioWorkletNode(audio,"rfscope-receivers",{numberOfInputs:0,numberOfOutputs:1,outputChannelCount:[1]}); node.current=player;
      player.connect(audio.destination); player.port.postMessage({type:"configure",ids:activeIds.current});
      let pending=0,losses=0;
      player.port.onmessage=({data})=>{
        if(data.type==="ack") pending=Math.max(0,pending-1);
        if(data.type==="health") { setHealth(data.streams as Health[]); setDropped(losses); }
      };
      const ws=new WebSocket(API.replace(/^http/,"ws")+"/stream/audio"); socket.current=ws; ws.binaryType="arraybuffer";
      ws.onmessage=({data})=>{
        if(generation.current!==run) return;
        if(pending>=8) { losses++; return; }
        try {
          const frame=decodeAudio(data as ArrayBuffer);
          if(!activeIds.current.includes(frame.id)) return;
          pending++; player.port.postMessage({type:"pcm",...frame},[frame.samples.buffer]);
        } catch(reason) { losses++; setError(String(reason)); }
      };
      ws.onerror=()=>setError("Audio connection failed");
      ws.onclose=()=>{if(generation.current===run){stop();setError("Audio disconnected; enable audio to reconnect");}};
      setEnabled(true);
    } catch(reason) { stop(); setError(String(reason)); }
    finally { setBusy(false); }
  }
  return <section className="device-panel"><button disabled={busy} onClick={()=>enabled?stop():void start()}>{enabled?"Disable audio":"Enable audio"}</button>
    <span>48 kHz mono per receiver · browser buffering 60–160 ms</span>
    {enabled&&<p>Audio queue drops: {dropped} · Underruns: {health.reduce((n,h)=>n+h.underruns,0)} · Overruns: {health.reduce((n,h)=>n+h.overruns,0)} · Discontinuities: {health.reduce((n,h)=>n+h.discontinuities,0)}</p>}
    {error&&<p role="alert">{error}</p>}
  </section>;
}
