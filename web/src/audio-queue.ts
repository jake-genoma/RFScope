// Used in the worklet and deterministic tests. Storage never grows with playback time.
export class AudioQueue {
  readonly data = new Float32Array(7680);
  private read=0; private count=0; private fraction=0; private primed=false;
  underruns=0; overruns=0; discontinuities=0;
  private epoch:bigint|null=null; private sequence=0n;
  get buffered() { return this.count; }
  clear() { this.read=0; this.count=0; this.fraction=0; this.primed=false; }
  push(samples:Float32Array,epoch:bigint,sequence:bigint) {
    if(this.epoch!==null && (epoch!==this.epoch || sequence!==this.sequence+1n)) { this.discontinuities++; this.clear(); }
    this.epoch=epoch; this.sequence=sequence;
    if(this.count+samples.length>this.data.length) { this.overruns++; this.clear(); }
    const start=Math.max(0,samples.length-this.data.length);
    for(let i=start;i<samples.length;i++) { this.data[(this.read+this.count)%this.data.length]=samples[i]; this.count++; }
    if(this.count>=2880) this.primed=true;
  }
  sample(step=1):number {
    if(!this.primed) return 0;
    if(this.count<Math.max(2,Math.ceil(step)+1)) { this.underruns++; this.primed=false; return 0; }
    const current=this.data[this.read], next=this.data[(this.read+1)%this.data.length];
    const value=current+(next-current)*this.fraction;
    this.fraction+=step;
    const consume=Math.floor(this.fraction); this.fraction-=consume;
    this.read=(this.read+consume)%this.data.length; this.count-=consume;
    return value;
  }
}
