// Explicitly limited software WebGL backend for canvas.
const _swGL = {
  NO_ERROR:0, INVALID_ENUM:1280, INVALID_VALUE:1281, INVALID_OPERATION:1282,
  COLOR_BUFFER_BIT:16384, DEPTH_BUFFER_BIT:256, STENCIL_BUFFER_BIT:1024,
  RGBA:6408, UNSIGNED_BYTE:5121, FLOAT:5126,
  TRIANGLES:4, TRIANGLE_STRIP:5, TRIANGLE_FAN:6,
  ARRAY_BUFFER:34962, ELEMENT_ARRAY_BUFFER:34963,
  STATIC_DRAW:35044, DYNAMIC_DRAW:35048,
  VERTEX_SHADER:35633, FRAGMENT_SHADER:35632,
  COMPILE_STATUS:35713, LINK_STATUS:35714,
  ACTIVE_ATTRIBUTES:35721, ACTIVE_UNIFORMS:35718,
  VENDOR:7936, RENDERER:7937, VERSION:7938, SHADING_LANGUAGE_VERSION:35724,
  MAX_TEXTURE_SIZE:3379, MAX_VERTEX_ATTRIBS:34921, MAX_VIEWPORT_DIMS:3386,
  VIEWPORT:2978, COLOR_CLEAR_VALUE:3106, CURRENT_PROGRAM:35725,
};
const _swByte = x => Math.round(Math.min(1,Math.max(0,x))*255);
class _SoftwareWebGL {
  constructor(canvas, version, attributes={}) {
    this.canvas=canvas;
    this._version=version;
    this._attrs={alpha:attributes.alpha!==false,depth:attributes.depth!==false,
      stencil:!!attributes.stencil,antialias:false,premultipliedAlpha:true,
      preserveDrawingBuffer:attributes.preserveDrawingBuffer!==false};
    this._error=0;
    this._clearColor=[0,0,0,0];
    this._program=null;
    this._buffers={array:null,element:null};
    this._attributes=new Map();
    this._viewport=[0,0,canvas.width,canvas.height];
    this._damaged=false;
    this._resizeFromCanvas();
  }
  _fail(err) {if(this._error===0)this._error=err;}
  getError(){const err=this._error;this._error=0;return err;}
  getContextAttributes(){return {...this._attrs};}
  getSupportedExtensions(){return [];}
  getExtension(){return null;}
  isContextLost(){return false;}
  _resizeFromCanvas(){
    const w=this.canvas.width,h=this.canvas.height;
    if(w>_MAX_CANVAS_DIMENSION||h>_MAX_CANVAS_DIMENSION||w*h>_MAX_CANVAS_PIXELS)
      throw new RangeError('WebGL surface allocation limit exceeded');
    this._w=w;this._h=h;
    this.drawingBufferWidth=w;this.drawingBufferHeight=h;
    this._viewport=[0,0,w,h];
    this._buf=new Uint8ClampedArray(w*h*4);
    const register=__obscuraCore.ops.op_canvas_register_surface;
    if(typeof register==='function' && !register(this.canvas._nid,w,h,
      new Uint8Array(this._buf.buffer),_realmFrameId))
      throw new RangeError('WebGL canvas surface unavailable');
  }
  _damage(){
    if(this._damaged)return;
    this._damaged=true;
    queueMicrotask(()=>{
      this._damaged=false;
      const notify=__obscuraCore.ops.op_canvas_paint_damage;
      if(typeof notify==='function')notify(this.canvas._nid,_realmFrameId);
    });
  }
  getParameter(p){
    switch(p){
      case 7936:return 'Obscura';
      case 7937:return 'Obscura Software Rasterizer';
      case 7938:return 'WebGL '+this._version+'.0 (Obscura Software)';
      case 35724:return this._version===2?'WebGL GLSL ES 3.00 (partial)':'WebGL GLSL ES 1.00 (partial)';
      case 3379:return 4096;
      case 34921:return 8;
      case 3386:return new Int32Array([this._w,this._h]);
      case 2978:return new Int32Array(this._viewport);
      case 3106:return new Float32Array(this._clearColor);
      case 35725:return this._program;
      default:this._fail(1280);return null;
    }
  }
  getShaderPrecisionFormat(){return {rangeMin:127,rangeMax:127,precision:23};}
  viewport(x,y,w,h){
    if(w<0||h<0){this._fail(1281);return;}
    this._viewport=[x|0,y|0,w|0,h|0];
  }
  clearColor(r,g,b,a){this._clearColor=[r,g,b,a].map(x=>Math.max(0,Math.min(1,x)));}
  clear(bits){
    if((bits & 16384)===0)return;
    const [r,g,b,a]=this._clearColor.map(_swByte);
    for(let k=0;k<this._buf.length;k+=4){
      this._buf[k]=r;this._buf[k+1]=g;this._buf[k+2]=b;this._buf[k+3]=a;
    }
    this._damage();
  }
  readPixels(x,y,w,h,format,type,dst){
    if(format!==6408||type!==5121||!ArrayBuffer.isView(dst)||w<0||h<0||
       dst.byteLength<w*h*4){this._fail(1282);return;}
    for(let row=0;row<h;row++)for(let col=0;col<w;col++){
      const p=(row*w+col)*4,xx=x+col,yy=y+row;
      if(xx<0||xx>=this._w||yy<0||yy>=this._h){
        dst[p]=0;dst[p+1]=0;dst[p+2]=0;dst[p+3]=0;
      }else{
        const i=((this._h-1-yy)*this._w+xx)*4;
        dst[p]=this._buf[i];dst[p+1]=this._buf[i+1];
        dst[p+2]=this._buf[i+2];dst[p+3]=this._buf[i+3];
      }
    }
  }
  createShader(type){
    if(type!==35633&&type!==35632){this._fail(1280);return null;}
    return {kind:'shader',type,source:'',compiled:false,log:'',parsed:null};
  }
  shaderSource(shader,code){if(shader?.kind==='shader')shader.source=String(code);else this._fail(1282);}
  compileShader(shader){
    if(shader?.kind!=='shader'){this._fail(1282);return;}
    const src=shader.source.replace(/\/\*[\s\S]*?\*\//g,'');
    if(!/\bvoid\s+main\s*\(\s*(?:void)?\s*\)/.test(src)){
      shader.log='Missing main function';shader.compiled=false;return;
    }
    if(shader.type===35633){
      const position=(src.match(/\bgl_Position\s*=\s*(?:vec4\s*\(\s*)?([A-Za-z_]\w*)/)||[])[1];
      const attrs=[...src.matchAll(/\b(?:attribute|in)\s+vec[234]\s+([A-Za-z_]\w*)\s*;/g)].map(m=>m[1]);
      shader.parsed=position&&attrs.includes(position)?{position,attrs}:null;
    }else{
      const outName=(src.match(/\bout\s+vec4\s+([A-Za-z_]\w*)\s*;/)||[])[1]||'gl_FragColor';
      const match=new RegExp('\\b'+outName+'\\s*=\\s*([^;]+);').exec(src);
      const expr=match?.[1]?.trim();
      const uniform=[...src.matchAll(/\buniform\s+vec4\s+([A-Za-z_]\w*)\s*;/g)].map(m=>m[1]);
      const rgba=expr?.match(/^vec4\s*\(\s*([-+.\deE]+)\s*(?:,\s*([-+.\deE]+)\s*,\s*([-+.\deE]+)\s*,\s*([-+.\deE]+)\s*)?\)$/);
      shader.parsed=expr&&uniform.includes(expr)?{uniform:expr}:
        rgba?{color:rgba[2]===undefined?Array(4).fill(Number(rgba[1])):
          rgba.slice(1).map(Number)}:null;
    }
    shader.compiled=!!shader.parsed;
    shader.log=shader.compiled?'':'Unsupported GLSL expression in limited CPU backend';
  }
  getShaderParameter(s,p){return p===35713?!!s?.compiled:false;}
  getShaderInfoLog(s){return s?.log||'';}
  deleteShader(s){if(s)s.deleted=true;}
  createProgram(){return {kind:'program',shaders:[],linked:false,log:'',uniforms:{},attribs:{}};}
  attachShader(p,s){if(p?.kind==='program'&&s?.kind==='shader')p.shaders.push(s);else this._fail(1282);}
  bindAttribLocation(p,index,name){if(p?.kind==='program')p.attribs[name]=index;}
  linkProgram(p){
    if(p?.kind!=='program'){this._fail(1282);return;}
    const vert=p.shaders.find(s=>s.type===35633),frag=p.shaders.find(s=>s.type===35632);
    p.vert=vert?.parsed;p.frag=frag?.parsed;p.linked=!!vert?.compiled&&!!frag?.compiled;
    p.log=p.linked?'':'Missing or unsupported compiled shader';
  }
  getProgramParameter(p,k){return k===35714?!!p?.linked:
    k===35721?(p?.vert?.attrs?.length||0):
    k===35718?(p?.frag?.uniform?1:0):false;}
  getProgramInfoLog(p){return p?.log||'';}
  useProgram(p){if(p===null||p?.linked)this._program=p;else this._fail(1282);}
  getAttribLocation(p,name){
    if(!p?.linked)return -1;
    const i=p.vert.attrs.indexOf(name);
    return i<0?-1:(p.attribs[name]??i);
  }
  getUniformLocation(p,name){return p?.linked&&p.frag.uniform===name?{program:p,name}:null;}
  uniform4f(u,r,g,b,a){if(u?.program===this._program)this._program.uniforms[u.name]=[r,g,b,a];else this._fail(1282);}
  uniform4fv(u,v){if(v?.length>=4)this.uniform4f(u,v[0],v[1],v[2],v[3]);else this._fail(1281);}
  createBuffer(){return {kind:'buffer',bytes:new Uint8Array(0)};}
  bindBuffer(target,b){if(target===34962)this._buffers.array=b;else if(target===34963)this._buffers.element=b;else this._fail(1280);}
  bufferData(target,data){
    const b=target===34962?this._buffers.array:target===34963?this._buffers.element:null;
    if(!b){this._fail(1282);return;}
    const bytes=typeof data==='number'?new Uint8Array(Math.min(Math.max(0,data),16777216)):
      ArrayBuffer.isView(data)?new Uint8Array(data.buffer,data.byteOffset,data.byteLength):
      data instanceof ArrayBuffer?new Uint8Array(data):null;
    if(!bytes||bytes.length>16777216){this._fail(1281);return;}
    b.bytes=new Uint8Array(bytes);
  }
  enableVertexAttribArray(i){const a=this._attributes.get(i)||{};a.enabled=true;this._attributes.set(i,a);}
  vertexAttribPointer(i,size,type,normalized,stride,offset){
    if(!this._buffers.array||type!==5126||size<2||size>4){this._fail(1282);return;}
    const a=this._attributes.get(i)||{};
    Object.assign(a,{buffer:this._buffers.array,size,stride,offset,enabled:a.enabled||false});
    this._attributes.set(i,a);
  }
  _point(index,loc){
    const a=this._attributes.get(loc);
    if(!a?.enabled)return null;
    const at=(a.offset||0)+index*(a.stride||a.size*4);
    if(at<0||at+a.size*4>a.buffer.bytes.length)return null;
    const view=new DataView(a.buffer.bytes.buffer,a.buffer.bytes.byteOffset+at,a.size*4);
    const data=Array.from({length:a.size},(_,n)=>view.getFloat32(n*4,true));
    return [data[0],data[1],data[2]??0,data[3]??1];
  }
  _triangle(aa,bb,cc,color){
    const [vx,vy,vw,vh]=this._viewport;
    const map=p=>[vx+(p[0]/p[3]+1)*vw/2,vy+(p[1]/p[3]+1)*vh/2];
    const a=map(aa),b=map(bb),c=map(cc);
    const area=(b[0]-a[0])*(c[1]-a[1])-(b[1]-a[1])*(c[0]-a[0]);
    if(!Number.isFinite(area)||Math.abs(area)<1e-9)return;
    const x0=Math.max(0,Math.floor(Math.min(a[0],b[0],c[0])));
    const x1=Math.min(this._w-1,Math.ceil(Math.max(a[0],b[0],c[0])));
    const y0=Math.max(0,Math.floor(Math.min(a[1],b[1],c[1])));
    const y1=Math.min(this._h-1,Math.ceil(Math.max(a[1],b[1],c[1])));
    for(let y=y0;y<=y1;y++)for(let x=x0;x<=x1;x++){
      const px=x+.5,py=y+.5;
      const wa=((b[0]-px)*(c[1]-py)-(b[1]-py)*(c[0]-px))/area;
      const wb=((c[0]-px)*(a[1]-py)-(c[1]-py)*(a[0]-px))/area;
      if(wa>=0&&wb>=0&&wa+wb<=1){
        const idx=((this._h-1-y)*this._w+x)*4;
        for(let n=0;n<4;n++)this._buf[idx+n]=color[n];
      }
    }
  }
  drawArrays(mode,first,count){
    const p=this._program;
    if(!p?.linked||first<0||count<0||![4,5,6].includes(mode)){this._fail(1282);return;}
    const color=p.frag.color||p.uniforms[p.frag.uniform];
    if(!color){this._fail(1282);return;}
    const loc=this.getAttribLocation(p,p.vert.position);
    const pts=[];
    for(let i=0;i<count;i++){const pt=this._point(first+i,loc);if(!pt||pt[3]===0){this._fail(1282);return;}pts.push(pt);}
    const rgba=color.map(_swByte);
    for(let i=0;i+2<count;i++){
      if(mode===4&&i%3!==0)continue;
      const tri=mode===6?[pts[0],pts[i+1],pts[i+2]]:[pts[i],pts[i+1],pts[i+2]];
      this._triangle(tri[0],tri[1],tri[2],rgba);
    }
    this._damage();
  }
  flush(){}
  finish(){}
}
for(const [name,value] of Object.entries(_swGL)){
  Object.defineProperty(_SoftwareWebGL.prototype,name,{value,configurable:false});
}
globalThis.WebGLRenderingContext=class WebGLRenderingContext extends _SoftwareWebGL {
  constructor(canvas,options){super(canvas,1,options);}
};
globalThis.WebGL2RenderingContext=class WebGL2RenderingContext extends globalThis.WebGLRenderingContext {
  constructor(canvas,options){super(canvas,options);this._version=2;}
};
