import { AudioQueue } from "./audio-queue";
declare const sampleRate:number;
declare class AudioWorkletProcessor { readonly port:MessagePort; }
declare function registerProcessor(name:string,processor:typeof AudioWorkletProcessor):void;
class ReceiverAudio extends AudioWorkletProcessor {
  private queues=new Map<string,AudioQueue>();
  private frames=0;
  constructor() {
    super();
    this.port.onmessage=({data})=> {
      if(data.type==="configure") {
        const ids=new Set<string>((data.ids as string[]).slice(0,128));
        for(const id of this.queues.keys()) if(!ids.has(id)) this.queues.delete(id);
        for(const id of ids) if(!this.queues.has(id)) this.queues.set(id,new AudioQueue());
      } else if(data.type==="pcm") {
        this.queues.get(data.id)?.push(data.samples,data.epoch,data.sequence);
        this.port.postMessage({type:"ack"});
      }
    };
  }
  process(_inputs:Float32Array[][],outputs:Float32Array[][]):boolean {
    const output=outputs[0]?.[0]; if(!output) return true;
    for(let i=0;i<output.length;i++) {
      let sum=0; for(const queue of this.queues.values()) sum+=queue.sample(48000/sampleRate);
      output[i]=Math.max(-1,Math.min(1,sum));
    }
    this.frames+=output.length;
    if(this.frames>=sampleRate) {
      this.frames=0;
      this.port.postMessage({type:"health",streams:[...this.queues.entries()].map(([id,q])=>({id,buffered:q.buffered,underruns:q.underruns,overruns:q.overruns,discontinuities:q.discontinuities}))});
    }
    return true;
  }
}
registerProcessor("rfscope-receivers",ReceiverAudio);
