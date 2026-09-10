import type { Vfo } from "./VfoPanel";
import { vfoBand } from "./vfo-overlay";
export class SpectrumRenderer {
  private context: CanvasRenderingContext2D;
  overlays(center: number, rate: number, vfos: Vfo[]): void {
    const c = this.context, w = this.canvas.width, h = this.canvas.height;
    for (const vfo of vfos) {
      const band = vfoBand(center, rate, vfo.configuration.frequency_hz, vfo.configuration.bandwidth_hz);
      if (!band) continue;
      c.fillStyle = "#f2bc4130"; c.fillRect(band.left*w, 0, Math.max(2, (band.right-band.left)*w), h);
      c.strokeStyle = "#f2bc41"; c.beginPath(); c.moveTo(band.middle*w, 0); c.lineTo(band.middle*w, h); c.stroke();
      c.fillStyle = "#f2bc41"; c.font = `${12*devicePixelRatio}px sans-serif`;
      c.fillText(vfo.configuration.name, Math.max(0, Math.min(w-100*devicePixelRatio, band.middle*w+4)), 18*devicePixelRatio);
    }
  }
  constructor(private canvas: HTMLCanvasElement) { const context=canvas.getContext("2d"); if (!context) throw new Error("Canvas 2D unavailable"); this.context=context; }
  draw(bins: Float32Array): void { const {canvas,context:c}=this; const dpr=devicePixelRatio; const w=Math.floor(canvas.clientWidth*dpr),h=Math.floor(canvas.clientHeight*dpr); if(canvas.width!==w||canvas.height!==h){canvas.width=w;canvas.height=h}c.fillStyle="#071018";c.fillRect(0,0,w,h);c.strokeStyle="#123448";c.lineWidth=1;for(let i=1;i<5;i++){c.beginPath();c.moveTo(0,h*i/5);c.lineTo(w,h*i/5);c.stroke()}c.strokeStyle="#42d9ff";c.lineWidth=1.5*dpr;c.beginPath();for(let i=0;i<bins.length;i++){const x=i*w/(bins.length-1);const y=Math.max(0,Math.min(h,h-(bins[i]+120)*h/120));i?c.lineTo(x,y):c.moveTo(x,y)}c.stroke();}
}
export class WaterfallRenderer {
  private gl: WebGL2RenderingContext; private program: WebGLProgram; private texture: WebGLTexture; private row=0; private width=0; private readonly height=420;
  constructor(private canvas: HTMLCanvasElement){const gl=canvas.getContext("webgl2",{antialias:false});if(!gl)throw new Error("WebGL2 unavailable");this.gl=gl;const compile=(type:number,source:string)=>{const s=gl.createShader(type);if(!s)throw new Error("shader allocation failed");gl.shaderSource(s,source);gl.compileShader(s);if(!gl.getShaderParameter(s,gl.COMPILE_STATUS))throw new Error(gl.getShaderInfoLog(s)??"shader compile failed");return s};const p=gl.createProgram();if(!p)throw new Error("program allocation failed");gl.attachShader(p,compile(gl.VERTEX_SHADER,"#version 300 es\nin vec2 p;out vec2 uv;void main(){uv=p*.5+.5;gl_Position=vec4(p,0,1);}"));gl.attachShader(p,compile(gl.FRAGMENT_SHADER,"#version 300 es\nprecision highp float;uniform sampler2D tex;uniform float offset;in vec2 uv;out vec4 color;void main(){float v=texture(tex,vec2(uv.x,fract(uv.y+offset))).r;color=vec4(smoothstep(.25,.7,v),smoothstep(.05,.5,v),smoothstep(0.,.25,v),1.);}"));gl.linkProgram(p);if(!gl.getProgramParameter(p,gl.LINK_STATUS))throw new Error("WebGL program link failed");this.program=p;this.texture=gl.createTexture()!;const b=gl.createBuffer();gl.bindBuffer(gl.ARRAY_BUFFER,b);gl.bufferData(gl.ARRAY_BUFFER,new Float32Array([-1,-1,1,-1,-1,1,-1,1,1,-1,1,1]),gl.STATIC_DRAW);const loc=gl.getAttribLocation(p,"p");gl.enableVertexAttribArray(loc);gl.vertexAttribPointer(loc,2,gl.FLOAT,false,0,0);}
  push(bins:Float32Array):void{const gl=this.gl;if(this.width!==bins.length){this.width=bins.length;this.row=0;gl.bindTexture(gl.TEXTURE_2D,this.texture);gl.texImage2D(gl.TEXTURE_2D,0,gl.R8,this.width,this.height,0,gl.RED,gl.UNSIGNED_BYTE,null);gl.texParameteri(gl.TEXTURE_2D,gl.TEXTURE_MIN_FILTER,gl.LINEAR);gl.texParameteri(gl.TEXTURE_2D,gl.TEXTURE_MAG_FILTER,gl.LINEAR);gl.texParameteri(gl.TEXTURE_2D,gl.TEXTURE_WRAP_S,gl.CLAMP_TO_EDGE);gl.texParameteri(gl.TEXTURE_2D,gl.TEXTURE_WRAP_T,gl.REPEAT)}const row=new Uint8Array(bins.length);for(let i=0;i<bins.length;i++)row[i]=Math.max(0,Math.min(255,(bins[i]+120)*255/100));gl.bindTexture(gl.TEXTURE_2D,this.texture);gl.pixelStorei(gl.UNPACK_ALIGNMENT,1);gl.texSubImage2D(gl.TEXTURE_2D,0,0,this.row,this.width,1,gl.RED,gl.UNSIGNED_BYTE,row);this.row=(this.row+1)%this.height;const dpr=devicePixelRatio,w=Math.floor(this.canvas.clientWidth*dpr),h=Math.floor(this.canvas.clientHeight*dpr);if(this.canvas.width!==w||this.canvas.height!==h){this.canvas.width=w;this.canvas.height=h}gl.viewport(0,0,w,h);gl.useProgram(this.program);gl.uniform1f(gl.getUniformLocation(this.program,"offset"),this.row/this.height);gl.drawArrays(gl.TRIANGLES,0,6);}
}
