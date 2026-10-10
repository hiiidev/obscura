// Driver-independent tests for WebIDL conversion, private ownership and lifecycle.
// Execute separately from V8 tests: node crates/obscura-js/tests/webgl_bindings.cjs
const {test} = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const path = require('node:path');
const source = fs.readFileSync(path.join(__dirname,'../js/webgl.js'),'utf8')
  .replace('/* @obscura-imagedata */',fs.readFileSync(path.join(__dirname,'../js/imagedata.js'),'utf8'))
  .replace('/* @obscura-imagebitmap */',fs.readFileSync(path.join(__dirname,'../js/imagebitmap.js'),'utf8'));
function fixture(options={}) {
  const imageDataStore=options.imageDataStore||new WeakMap();
  const calls=[],tasks=[],events=[],native=new Map(),metadata=new Map(),states=new Set(),registrations=[],lossWatchers=new Map();
  let nextContext=1,nextObject=1,documentGeneration=1;
  // Deterministic lifetime model: these tests inspect private retention edges
  // and explicitly deliver finalizers. Real V8 GC remains an integration gate.
  class TrackedWeakMap extends WeakMap {
    set(key,value){if(value?.owner&&value?.references)metadata.set(key,value);if(value?.bindings)states.add(value);return super.set(key,value);}
  }
  class ModelWeakRef {constructor(target){this.target=target;}deref(){return this.target;}}
  class ModelFinalizationRegistry {constructor(callback){this.callback=callback;}register(target,record){registrations.push({target,record,callback:this.callback});}}
  function retained(value,extraRoots=[]) {
    const pending=[...extraRoots],seen=new Set();
    for(const state of states)pending.push(...state.bindings.values(),...state.defaultVertexArray.values(),...state.defaultTransformFeedback.values());
    while(pending.length){const next=pending.pop();if(next===value)return true;if(seen.has(next))continue;seen.add(next);pending.push(...(metadata.get(next)?.references.values()||[]));}
    return false;
  }
  function finalize(value,extraRoots=[]) {
    if(retained(value,extraRoots))return false;
    for(const {target,record,callback} of registrations)if(target===value){record.reference.target=undefined;callback(record);}
    return true;
  }
  class Event {constructor(type,init={}){this.type=type;this.cancelable=!!init.cancelable;this.defaultPrevented=false;}preventDefault(){if(this.cancelable)this.defaultPrevented=true;}}
  class Canvas {constructor(){this.width=4;this.height=3;this._nid=nextContext;this.listener=null;}dispatchEvent(event){events.push(event.type);this.listener?.(event);return !event.defaultPrevented;}}
  class Image {constructor(){this._nid=42;}}
  class Video {}
  const sandbox={HTMLCanvasElement:Canvas,HTMLImageElement:Image,HTMLVideoElement:Video,DOMException,Event,Blob,console,Uint8Array,Uint8ClampedArray,Uint16Array,Float32Array,Int32Array,Uint32Array,ArrayBuffer,Map,WeakMap:TrackedWeakMap,WeakRef:ModelWeakRef,FinalizationRegistry:ModelFinalizationRegistry,Set,Symbol,
    // This standalone VM fixture tests API binding semantics without the
    // bootstrap's pixel fingerprint shader/noise helpers. Full profile
    // variance remains an application-level graphics acceptance gate.
    _fpRenderVariance:()=>false,_fpGpuVariance:()=>{},
    _hostState:{documentGeneration:1},
    _canvasBitmapDraw:null,_placeholderHas:()=>false,
    _canvas2DContext:canvas=>canvas?._ctx,
    _canvasDOMOwner:canvas=>({frame:9,epoch:1,node:canvas._nid}),
    _canvas2DPixels:context=>context?{width:context._w,height:context._h,bytes:context._buf,originClean:context.clean!==false}:null,
    _canvas2DTaint:context=>{context.clean=false;},
    _realmFrameId:9,_scheduleAfter:(_,fn)=>{if(options.noTimers)throw Error('timer unavailable');tasks.push(fn);},_markNative:()=>{},_webglCreate:null,_webglHas:null,_webglResize:null,_webglReadback:null,
    __obscuraCore:{ops:{
      op_webgl_image_data(object,data){if(data){if(options.imageDataAllocationFails)return null;imageDataStore.set(object,data);}return imageDataStore.get(object)||null;},
      op_posted_task(frame,callback){tasks.push(()=>callback(documentGeneration));return documentGeneration;},
      op_webgl_watch_loss(frame,id,callback){if(options.watchFails)return false;const generation=documentGeneration;lossWatchers.set(id,()=>{if(generation===documentGeneration)callback();});return true;},
      op_webgl_release(frame,id){lossWatchers.delete(id);return native.delete(id);},
      op_webgl_image_info(frame,node,allowTainted){calls.push({imageInfo:true,frame,node,allowTainted});return{status:options.imageStatus||'ready',width:2,height:1,originClean:options.originClean!==false};},
      op_webgl_image_pixels(frame,node,pixels,allowTainted){calls.push({imagePixels:true,frame,node,allowTainted});pixels.set([255,0,0,255,0,255,0,128]);return options.imagePixelsResult||0;},
      op_webgl_convert_color(source,target,premultiplied,bytes){calls.push({convertColor:true,source,target,premultiplied});if(options.colorFails)return false;if(options.convertedPixels)bytes.set(options.convertedPixels);return true;},
      op_webgl_decode_info(bytes){return options.decodeFails?{status:'invalid'}:{status:'ready',width:1,height:1};},
      op_webgl_decode_pixels(bytes,pixels){pixels.set([17,33,65,128]);return !options.decodePixelsFail;},
      op_webgl_create(frame,epoch,node,version,width,height,attributes){calls.push({create:true,frame,epoch,node,version,width,height,attributes});if(options.fail)return{status:'failed',reason:'Missing graphics bundle'};const id=nextContext++;native.set(id,{lost:false,error:0,version,attributes,canvasSize:{width,height},buffer:{width,height,format:attributes.alpha===false?0x8051:0x8058}});return{status:'ready',id,width,height,attributes};},
      op_webgl_call(frame,id,operation,data){calls.push({frame,id,operation,data:Array.from(data),sharedBacking:Object.prototype.toString.call(data.buffer)==='[object SharedArrayBuffer]'});const s=native.get(id);let value={type:'none'};const {kind,value:arg}=operation;
        if(!s||(s.lost&&options.emptyLossReply&&kind!=='restore'))return{lost:true,value};
        if(kind==='create')value={type:'number',value:nextObject++};
        if(kind==='error')s.error=arg.error;
        if(kind==='getError'){value={type:'number',value:s.error};s.error=0;}
        if(kind==='lose')s.lost=true;
        if(kind==='restore'){s.lost=false;s.buffer={...s.canvasSize,format:s.attributes.alpha===false?0x8051:0x8058};value={type:'boolean',value:true};}
        if(kind==='resize')s.canvasSize={width:arg.width,height:arg.height};
        if((kind==='drawingBufferStorage'||kind==='resize')&&!s.lost){
          if(options.storageError&&kind==='drawingBufferStorage')s.error=options.storageError;
          else {s.buffer={...s.buffer,...arg};value={type:'drawingBuffer',value:s.buffer};}
        }
        if(kind==='drawingBufferInfo'&&!s.lost)value={type:'drawingBuffer',value:s.buffer};
        if(kind==='isLost')value={type:'boolean',value:s.lost};
        if(kind==='attributes')value={type:'attributes',value:s.lost?null:s.attributes};
        if(kind==='extension'){
          const extensions=options.extensions||['WEBGL_lose_context','WEBGL_debug_renderer_info','OES_element_index_uint','OES_standard_derivatives','OES_texture_float','OES_texture_float_linear','OES_texture_half_float','OES_texture_half_float_linear','OES_vertex_array_object','ANGLE_instanced_arrays','EXT_texture_filter_anisotropic','EXT_color_buffer_float','EXT_color_buffer_half_float','WEBGL_depth_texture'];
          value={type:'text',value:s.lost?null:extensions.find(name=>name.toLowerCase()===arg.name.toLowerCase())||null};
        }
        if(kind==='supportedExtensions')value={type:'extensions',value:s.lost?null:['WEBGL_lose_context']};
        if(kind==='query'||kind==='advanced')value={type:'query',value:options.query?.(arg)||{type:'null'}};
        if(kind==='query'&&arg.method==='getUniformLocation')value={type:'query',value:{type:'uint',value:nextObject++}};
        if(kind==='readback'||kind==='sourceReadback'){data.fill(42);value={type:'boolean',value:true};}
        const dataReply=options.dataReply?.(operation,data);if(dataReply)value=dataReply;
        return{lost:s.lost,accepted:!options.reject?.(operation),dirty:options.dirty?.(operation)===true,value};
      }
    }}
  };
  if(options.lateSharedBuffer)sandbox.SharedArrayBuffer=undefined;
  // Runtime stealth flag and the bootstrap's seeded fingerprint accessor.
  if(options.stealth){sandbox.__obscura_stealth=true;sandbox._fp=key=>options.stealth[key];}
  if(options.failSharedCopy)sandbox.Uint8Array=new Proxy(Uint8Array,{construct(ctor,args){if(ArrayBuffer.isView(args[0]))throw new RangeError('fixture copy allocation');return Reflect.construct(ctor,args);}});
  vm.createContext(sandbox);vm.runInContext(source,sandbox);
  if(options.lateSharedBuffer)sandbox.SharedArrayBuffer=SharedArrayBuffer;
  const canvas=new Canvas();const gl=sandbox._webglCreate(canvas,'webgl',{});
  return {sandbox,canvas,gl,calls,tasks,events,native,metadata,states,registrations,retained,finalize,advanceDocument(){sandbox._hostState.documentGeneration=++documentGeneration;},loseNative(id=1){native.get(id).lost=true;tasks.push(lossWatchers.get(id));},drain(){for(let i=0;tasks.length&&i<100;i++)tasks.shift()();assert.equal(tasks.length,0);}};
}
test('dirty HTML WebGL commands tolerate absent optional Offscreen presentation hook',()=>{
  const f=fixture({dirty:operation=>operation.kind==='command'&&operation.value?.method==='viewport'});
  assert.doesNotThrow(()=>f.gl.viewport(0,0,4,3));
  assert.equal(f.calls.at(-1).operation.kind,'command');
  assert.equal(f.calls.at(-1).operation.value.method,'viewport');
  const scheduled=[];
  f.sandbox.schedulePlaceholder=canvas=>scheduled.push(canvas);
  f.gl.viewport(0,0,4,3);
  assert.deepEqual(scheduled,[f.canvas],'installed Offscreen presentation hook still receives dirty canvas');
  delete f.sandbox.schedulePlaceholder;
  assert.doesNotThrow(()=>f.gl.viewport(0,0,4,3));
});
test('context identity and canvas context exclusivity',()=>{
  const {sandbox,canvas,gl}=fixture();
  assert.equal(sandbox._webglCreate(canvas,'experimental-webgl',{}),gl);
  assert.equal(sandbox._webglCreate(canvas,'webgl2',{}),null);
  assert.equal(gl.canvas,canvas);assert.equal(gl.drawingBufferWidth,4);
  assert.equal(Object.keys(gl).length,0);
  assert.equal(Object.getOwnPropertySymbols(gl).length,0);
  assert.throws(()=>new sandbox.WebGLRenderingContext(),/Illegal constructor/);
  assert.throws(()=>gl.clear.call({},0),/Illegal invocation/);
  assert.equal(typeof gl.bindVertexArray,'undefined');
  assert.equal(typeof sandbox.WebGL2RenderingContext.prototype.bindVertexArray,'function');
  assert.equal(gl.clear.length,1);assert.equal(Object.getOwnPropertyDescriptor(Object.getPrototypeOf(gl),'clear').enumerable,true);
});
test('all object state remains private and another context cannot bind an object',()=>{
  const f=fixture();const otherCanvas=new f.sandbox.HTMLCanvasElement();const other=f.sandbox._webglCreate(otherCanvas,'webgl',{});
  const buffer=f.gl.createBuffer();assert.equal(Object.keys(buffer).length,0);
  assert.throws(()=>f.gl.bindBuffer(f.gl.ARRAY_BUFFER,{}),/Expected WebGL/);
  other.bindBuffer(other.ARRAY_BUFFER,buffer);assert.equal(other.getError(),other.INVALID_OPERATION);
  assert.equal(f.calls.filter(c=>c.operation?.kind==='resource').length,0);
  f.gl.bindBuffer(f.gl.ARRAY_BUFFER,buffer);
  assert.equal(f.calls.at(-1).operation.value.command.method,'bindBuffer');
});
test('byte views retain offsets and uploads do not include unrelated bytes',()=>{
  const f=fixture();const backing=new Uint8Array([90,1,2,3,4,91]);
  f.gl.bufferData(f.gl.ARRAY_BUFFER,backing.subarray(1,5),f.gl.STATIC_DRAW);
  assert.deepEqual(f.calls.at(-1).data,[1,2,3,4]);
  f.gl.bufferSubData(f.gl.ARRAY_BUFFER,2,backing,2,2);
  assert.deepEqual(f.calls.at(-1).data,[2,3]);
  f.gl.bufferSubData(f.gl.ARRAY_BUFFER,0,backing,99);
  assert.equal(f.gl.getError(),f.gl.INVALID_VALUE);
});
test('WebIDL dictionaries preserve defaults and reject invalid power preferences',()=>{
  const f=fixture();const canvas=new f.sandbox.HTMLCanvasElement();
  f.sandbox._webglCreate(canvas,'webgl2',{alpha:0,antialias:1,powerPreference:'low-power'});
  assert.deepEqual(JSON.parse(JSON.stringify(f.calls.at(-1).attributes)),{alpha:false,antialias:true,powerPreference:'low-power'});
  assert.throws(()=>f.sandbox._webglCreate(new f.sandbox.HTMLCanvasElement(),'webgl',{powerPreference:'fastest'}),/powerPreference/);
});
test('context loss is queued once and restoration requires preventDefault',()=>{
  const f=fixture();const ext=f.gl.getExtension('WEBGL_lose_context');const old=f.gl.createBuffer();
  ext.loseContext();assert.equal(f.gl.isContextLost(),true);assert.equal(f.events.length,0);
  ext.restoreContext();f.drain();assert.deepEqual(f.events,['webglcontextlost']);assert.equal(f.gl.isContextLost(),true);
  // A separate context with a handler opts into restoration.
  const g=fixture();g.canvas.listener=event=>event.preventDefault();const e=g.gl.getExtension('WEBGL_lose_context');const stale=g.gl.createBuffer();
  e.loseContext();g.drain();e.restoreContext();g.drain();assert.equal(g.gl.isContextLost(),false);
  assert.deepEqual(g.events,['webglcontextlost','webglcontextrestored']);
  g.gl.bindBuffer(g.gl.ARRAY_BUFFER,stale);assert.equal(g.gl.getError(),g.gl.INVALID_OPERATION);
  assert.ok(old);
});
test('failed creation returns null, emits an error task, and does not lock the canvas',()=>{
  const f=fixture({fail:true});assert.equal(f.gl,null);assert.equal(f.sandbox._webglHas(f.canvas),false);
  assert.equal(f.events.length,0);f.drain();assert.deepEqual(f.events,['webglcontextcreationerror']);
});
test('uniform vector offsets and matrix dimensions reach the typed native boundary',()=>{
  const f=fixture();
  // null locations remain explicit no-ops at the native boundary.
  f.gl.uniformMatrix4fv(null,false,new Float32Array(16));
  const command=f.calls.at(-1).operation.value;
  assert.equal(command.location,0);assert.equal(command.columns,4);assert.equal(command.rows,4);assert.equal(command.values.values.length,16);
  assert.throws(()=>f.gl.uniform4f(null,1),/requires/);
});
test('native readback fills the exact canvas-sized destination',()=>{
  const f=fixture();const image=f.sandbox._webglReadback(f.canvas);
  assert.equal(image.bytes.length,48);assert.equal(image.bytes[47],42);
  // This bare Canvas stub needs the resize notification sent by the DOM setter.
  f.canvas.width=0;f.sandbox._webglResize(f.canvas);
  const empty=f.sandbox._webglReadback(f.canvas);assert.equal(empty.bytes.length,0);
});
test('pixel views must match the GL type before the driver sees their bytes',()=>{
  const f=fixture();f.gl.readPixels(0,0,1,1,f.gl.RGBA,f.gl.UNSIGNED_BYTE,new Float32Array(4));
  assert.equal(f.gl.getError(),f.gl.INVALID_OPERATION);
  assert.equal(f.calls.filter(c=>c.operation?.kind==='readPixels').length,0);
  f.gl.readPixels(0,0,1,1,f.gl.RGBA,f.gl.UNSIGNED_BYTE,new Uint8Array(4));
  assert.equal(f.calls.at(-1).operation.kind,'readPixels');
});
test('WebGL 2 pixel-buffer offsets are explicit and never treated as JS pointers',()=>{
  const f=fixture();const gl=f.sandbox._webglCreate(new f.sandbox.HTMLCanvasElement(),'webgl2',{});
  gl.readPixels(0,0,1,1,gl.RGBA,gl.UNSIGNED_BYTE,16);
  assert.equal(f.calls.at(-1).operation.kind,'readPixelsBuffer');assert.equal(f.calls.at(-1).operation.value.offset,16);
  gl.texImage2D(gl.TEXTURE_2D,0,gl.RGBA8,1,1,0,gl.RGBA,gl.UNSIGNED_BYTE,32);
  assert.equal(f.calls.at(-1).operation.value.buffer_offset,32);assert.equal(f.calls.at(-1).data.length,0);
  gl.readPixels(0,0,1,1,gl.RGBA,gl.UNSIGNED_BYTE,-1);assert.equal(gl.getError(),gl.INVALID_VALUE);
  gl.readPixels(0,0,1,1,gl.RGBA,gl.UNSIGNED_BYTE,2**40);assert.equal(gl.getError(),gl.INVALID_VALUE);
});
test('unadvertised compressed formats report INVALID_ENUM',()=>{
  const f=fixture();f.gl.compressedTexImage2D(f.gl.TEXTURE_2D,0,0x83F1,4,4,0,new Uint8Array(8));
  assert.equal(f.gl.getError(),f.gl.INVALID_ENUM);
});
test('DOM image uploads use native source conversion, including packed and float formats',()=>{
  const f=fixture();const image=new f.sandbox.HTMLImageElement();
  f.gl.texImage2D(f.gl.TEXTURE_2D,0,f.gl.RGB,f.gl.RGB,f.gl.UNSIGNED_SHORT_5_6_5,image);
  assert.equal(f.calls.at(-1).operation.kind,'textureSource');
  assert.equal(f.calls.at(-1).operation.value.image.width,2);
  assert.equal(f.calls.at(-1).operation.value.image.data_type,f.gl.UNSIGNED_SHORT_5_6_5);
  assert.deepEqual(f.calls.at(-1).data,[255,0,0,255,0,255,0,128]);
  assert.equal(f.calls.find(c=>c.imageInfo).frame,9);
  f.gl.texSubImage2D(f.gl.TEXTURE_2D,0,1,2,f.gl.RGBA,f.gl.FLOAT,new f.sandbox.ImageData(new Uint8ClampedArray([1,2,3,4]),1,1));
  assert.equal(f.calls.at(-1).operation.kind,'textureSource');
  assert.equal(f.calls.at(-1).operation.value.image.x,1);
});
test('unreadable or changed image sources never reach the graphics upload',()=>{
  for(const options of [{imageStatus:'security'},{imagePixelsResult:2}]){
    const f=fixture(options);
    assert.throws(()=>f.gl.texImage2D(f.gl.TEXTURE_2D,0,f.gl.RGBA,f.gl.RGBA,f.gl.UNSIGNED_BYTE,new f.sandbox.HTMLImageElement()),{name:'SecurityError'});
    assert.equal(f.calls.filter(c=>c.operation?.kind==='textureSource').length,0);
  }
  for(const options of [{imageStatus:'unavailable'},{imageStatus:'invalid'},{imagePixelsResult:1}]){
    const f=fixture(options);f.gl.texImage2D(f.gl.TEXTURE_2D,0,f.gl.RGBA,f.gl.RGBA,f.gl.UNSIGNED_BYTE,new f.sandbox.HTMLImageElement());
    assert.equal(f.gl.getError(),f.gl.INVALID_VALUE);
    assert.equal(f.calls.filter(c=>c.operation?.kind==='textureSource').length,0);
  }
});
test('empty, detached and incomplete texture sources generate errors',()=>{
  const f=fixture();
  for(const data of [null,new f.sandbox.HTMLVideoElement()]){
    f.gl.texImage2D(f.gl.TEXTURE_2D,0,f.gl.RGBA,f.gl.RGBA,f.gl.UNSIGNED_BYTE,data);
    assert.equal(f.gl.getError(),f.gl.INVALID_VALUE);
  }
  const detached=new f.sandbox.ImageData(1,1);structuredClone(detached.data.buffer,{transfer:[detached.data.buffer]});
  assert.throws(()=>f.gl.texImage2D(f.gl.TEXTURE_2D,0,f.gl.RGBA,f.gl.RGBA,f.gl.UNSIGNED_BYTE,detached),{name:'InvalidStateError'});
  assert.equal(f.calls.filter(c=>c.operation?.kind==='textureSource').length,0);
});
test('WebGL 2 explicit source rectangles and 3D source dimensions stay distinct',()=>{
  const f=fixture();const gl=f.sandbox._webglCreate(new f.sandbox.HTMLCanvasElement(),'webgl2',{});
  const image=new f.sandbox.HTMLImageElement();
  gl.texImage2D(gl.TEXTURE_2D,0,gl.RGBA8,1,1,0,gl.RGBA,gl.UNSIGNED_BYTE,image);
  assert.equal(f.calls.at(-1).operation.value.width,2);
  assert.equal(f.calls.at(-1).operation.value.image.width,1);
  gl.texImage3D(gl.TEXTURE_3D,0,gl.RGBA8,1,1,2,0,gl.RGBA,gl.UNSIGNED_BYTE,image);
  assert.equal(f.calls.at(-1).operation.value.image.depth,2);
  assert.equal(f.calls.at(-1).operation.value.image.three_dimensional,true);
});
test('pointer and element offsets do not wrap to valid small offsets',()=>{
  const f=fixture();
  for(const offset of [-1,2**32,2**40,Infinity]){
    f.gl.drawElements(f.gl.TRIANGLES,3,f.gl.UNSIGNED_SHORT,offset);assert.equal(f.gl.getError(),f.gl.INVALID_VALUE);
    f.gl.vertexAttribPointer(0,2,f.gl.FLOAT,false,0,offset);assert.equal(f.gl.getError(),f.gl.INVALID_VALUE);
    f.gl.bufferSubData(f.gl.ARRAY_BUFFER,0,new Uint8Array(4),offset);assert.equal(f.gl.getError(),f.gl.INVALID_VALUE);
  }
  assert.equal(f.calls.filter(c=>c.operation?.kind==='command'||c.operation?.kind==='resource').length,0);
});
test('context events use the WebGL interface and keep statusMessage readonly',()=>{
  const f=fixture();const e=new f.sandbox.WebGLContextEvent('webglcontextlost',{statusMessage:123,cancelable:true});
  assert.equal(e.statusMessage,'123');assert.equal(e instanceof f.sandbox.Event,true);
  assert.equal(Object.hasOwn(e,'statusMessage'),false);
  assert.equal(Object.getOwnPropertyDescriptor(f.sandbox.WebGLContextEvent.prototype,'statusMessage').set,undefined);
  assert.throws(()=>Object.getOwnPropertyDescriptor(f.sandbox.WebGLContextEvent.prototype,'statusMessage').get.call({}),/Illegal invocation/);
  const seen=[];f.canvas.listener=e=>{seen.push(e);e.preventDefault();};
  const extension=f.gl.getExtension('WEBGL_lose_context');extension.loseContext();f.drain();extension.restoreContext();f.drain();
  assert.equal(seen.length,2);assert.ok(seen.every(e=>e instanceof f.sandbox.WebGLContextEvent));
  assert.equal(seen[1].statusMessage,'');
});

test('unused wrappers are collectible but accepted bindings retain identity',()=>{
  const f=fixture(),gl=f.gl,unused=gl.createBuffer(),bound=gl.createBuffer();
  assert.equal(f.finalize(unused),true);
  assert.equal(f.calls.at(-1).operation.kind,'collect');
  gl.bindBuffer(gl.ARRAY_BUFFER,bound);
  assert.equal(f.finalize(bound),false);
  gl.bindBuffer(gl.ARRAY_BUFFER,null);
  assert.equal(f.finalize(bound),true);
  const count=f.calls.filter(c=>c.operation?.kind==='collect').length;
  f.finalize(bound);
  assert.equal(f.calls.filter(c=>c.operation?.kind==='collect').length,count);
});

test('rejected reference changes do not release or acquire resources',()=>{
  let reject=false;
  const f=fixture({reject:op=>reject&&['resource','delete'].includes(op.kind)}),gl=f.gl;
  const first=gl.createBuffer(),second=gl.createBuffer();
  gl.bindBuffer(gl.ARRAY_BUFFER,first);reject=true;
  gl.bindBuffer(gl.ARRAY_BUFFER,second);gl.deleteBuffer(first);
  assert.equal(f.retained(first),true);assert.equal(f.retained(second),false);
  assert.equal(f.metadata.get(first).deleted,false);
  reject=false;gl.bindBuffer(gl.ARRAY_BUFFER,null);assert.equal(f.retained(first),false);
});

test('vertex arrays own element and attribute buffers while unbound',()=>{
  const f=fixture(),gl=f.sandbox._webglCreate(new f.sandbox.HTMLCanvasElement(),'webgl2',{});
  const vao=gl.createVertexArray(),element=gl.createBuffer(),attribute=gl.createBuffer();
  gl.bindVertexArray(vao);gl.bindBuffer(gl.ELEMENT_ARRAY_BUFFER,element);
  gl.bindBuffer(gl.ARRAY_BUFFER,attribute);gl.vertexAttribPointer(0,2,gl.FLOAT,false,0,0);
  gl.bindBuffer(gl.ARRAY_BUFFER,null);gl.bindVertexArray(null);
  assert.equal(f.retained(element),false);assert.equal(f.retained(attribute,[vao]),true);
  assert.equal(f.finalize(element,[vao]),false);
  gl.bindVertexArray(vao);gl.deleteBuffer(attribute);
  assert.equal(f.retained(attribute,[vao]),false);assert.equal(f.retained(element),true);
  gl.deleteVertexArray(vao);assert.equal(f.retained(element,[vao]),false);
});

test('default vertex arrays and OES vertex arrays have separate retention',()=>{
  const f=fixture(),gl=f.gl,ext=gl.getExtension('OES_vertex_array_object');
  const base=gl.createBuffer(),other=gl.createBuffer(),vao=ext.createVertexArrayOES();
  gl.bindBuffer(gl.ELEMENT_ARRAY_BUFFER,base);ext.bindVertexArrayOES(vao);
  gl.bindBuffer(gl.ELEMENT_ARRAY_BUFFER,other);ext.bindVertexArrayOES(null);
  assert.equal(f.retained(base),true);assert.equal(f.retained(other,[vao]),true);
  ext.deleteVertexArrayOES(vao);assert.equal(f.retained(other,[vao]),false);
  gl.bindBuffer(gl.ELEMENT_ARRAY_BUFFER,null);assert.equal(f.retained(base),false);
});

test('texture units, samplers and unbound framebuffer attachments retain exact objects',()=>{
  let recycled=0;
  const f=fixture({query:q=>q.method==='getFramebufferAttachmentParameter'?{type:'object',value:{kind:'texture',id:recycled}}:null});
  const gl=f.sandbox._webglCreate(new f.sandbox.HTMLCanvasElement(),'webgl2',{});
  const texture=gl.createTexture(),fbo=gl.createFramebuffer(),sampler=gl.createSampler();
  gl.activeTexture(gl.TEXTURE0+1);gl.bindTexture(gl.TEXTURE_2D,texture);gl.bindSampler(1,sampler);
  gl.bindFramebuffer(gl.FRAMEBUFFER,fbo);gl.framebufferTexture2D(gl.FRAMEBUFFER,gl.COLOR_ATTACHMENT0,gl.TEXTURE_2D,texture,0);
  gl.bindFramebuffer(gl.FRAMEBUFFER,null);gl.deleteTexture(texture);
  assert.equal(f.retained(texture),false);assert.equal(f.retained(texture,[fbo]),true);
  const replacement=gl.createTexture();recycled=f.metadata.get(replacement).id;
  gl.bindFramebuffer(gl.FRAMEBUFFER,fbo);
  assert.equal(gl.getFramebufferAttachmentParameter(gl.FRAMEBUFFER,gl.COLOR_ATTACHMENT0,gl.FRAMEBUFFER_ATTACHMENT_OBJECT_NAME),texture);
  gl.deleteTexture(texture); // An already-deleted handle must have no further effect.
  assert.equal(f.retained(texture),true);
  gl.deleteFramebuffer(fbo);assert.equal(f.retained(texture,[fbo]),false);
  gl.deleteSampler(sampler);assert.equal(f.retained(sampler),false);
});

test('depth and stencil attachment aliases retain and release independently',()=>{
  const f=fixture(),gl=f.gl,fbo=gl.createFramebuffer(),buffer=gl.createRenderbuffer();
  gl.bindFramebuffer(gl.FRAMEBUFFER,fbo);
  gl.framebufferRenderbuffer(gl.FRAMEBUFFER,gl.DEPTH_STENCIL_ATTACHMENT,gl.RENDERBUFFER,buffer);
  gl.framebufferRenderbuffer(gl.FRAMEBUFFER,gl.DEPTH_ATTACHMENT,gl.RENDERBUFFER,null);
  assert.equal(f.retained(buffer),true);
  gl.framebufferRenderbuffer(gl.FRAMEBUFFER,gl.STENCIL_ATTACHMENT,gl.RENDERBUFFER,null);
  assert.equal(f.retained(buffer),false);
});

test('programs retain deleted attached shaders until detached or actually released',()=>{
  const f=fixture(),gl=f.gl,p=gl.createProgram(),shader=gl.createShader(gl.VERTEX_SHADER);
  gl.attachShader(p,shader);gl.deleteShader(shader);
  assert.equal(f.retained(shader,[p]),true);
  gl.useProgram(p);gl.deleteProgram(p);
  assert.equal(f.retained(p),true);assert.equal(f.retained(shader),true);
  gl.useProgram(null);assert.equal(f.retained(shader,[p]),false);
  const p2=gl.createProgram(),s2=gl.createShader(gl.FRAGMENT_SHADER);
  gl.attachShader(p2,s2);gl.detachShader(p2,s2);assert.equal(f.retained(s2,[p2]),false);
});

test('uniform locations are weakly cached and retain their owning program',()=>{
  const f=fixture(),gl=f.gl,p=gl.createProgram(),location=gl.getUniformLocation(p,'color');
  assert.ok(location);assert.equal(f.retained(p,[location]),true);
  assert.equal(f.finalize(location),true);assert.equal(f.calls.at(-1).operation.kind,'collectLocation');
});

test('indexed buffers belong to their transform feedback object or context slot',()=>{
  const f=fixture(),gl=f.sandbox._webglCreate(new f.sandbox.HTMLCanvasElement(),'webgl2',{});
  const tf=gl.createTransformFeedback(),buffer=gl.createBuffer(),uniform=gl.createBuffer();
  gl.bindTransformFeedback(gl.TRANSFORM_FEEDBACK,tf);
  gl.bindBufferBase(gl.TRANSFORM_FEEDBACK_BUFFER,0,buffer);
  gl.bindBuffer(gl.TRANSFORM_FEEDBACK_BUFFER,null);
  gl.bindTransformFeedback(gl.TRANSFORM_FEEDBACK,null);
  assert.equal(f.retained(buffer,[tf]),true);assert.equal(f.retained(buffer),false);
  gl.bindBufferRange(gl.UNIFORM_BUFFER,1,uniform,0,16);gl.bindBuffer(gl.UNIFORM_BUFFER,null);
  assert.equal(f.retained(uniform),true);
  gl.bindBufferBase(gl.UNIFORM_BUFFER,1,null);assert.equal(f.retained(uniform),false);
  gl.deleteTransformFeedback(tf);assert.equal(f.retained(buffer,[tf]),false);
});

test('active queries are retained until ended or explicitly deleted',()=>{
  const f=fixture(),gl=f.sandbox._webglCreate(new f.sandbox.HTMLCanvasElement(),'webgl2',{});
  const first=gl.createQuery(),second=gl.createQuery();
  gl.beginQuery(gl.ANY_SAMPLES_PASSED,first);assert.equal(f.retained(first),true);
  gl.endQuery(gl.ANY_SAMPLES_PASSED);assert.equal(f.retained(first),false);
  gl.beginQuery(gl.ANY_SAMPLES_PASSED_CONSERVATIVE,second);gl.deleteQuery(second);
  assert.equal(f.retained(second),false);
});

test('loss clears retention and stale finalizers cannot collect a restored generation',()=>{
  const f=fixture(),gl=f.gl,buffer=gl.createBuffer(),ext=gl.getExtension('WEBGL_lose_context');
  gl.bindBuffer(gl.ARRAY_BUFFER,buffer);f.canvas.listener=e=>e.preventDefault();
  ext.loseContext();f.drain();assert.equal(f.retained(buffer),false);
  ext.restoreContext();f.drain();
  const before=f.calls.filter(c=>c.operation?.kind==='collect').length;
  f.finalize(buffer);assert.equal(f.calls.filter(c=>c.operation?.kind==='collect').length,before);
});

test('paused transform feedback retains its resources and program while unbound',()=>{
  const f=fixture(),gl=f.sandbox._webglCreate(new f.sandbox.HTMLCanvasElement(),'webgl2',{});
  const feedback=gl.createTransformFeedback(),program=gl.createProgram(),buffer=gl.createBuffer();
  const shader=gl.createShader(gl.VERTEX_SHADER);gl.attachShader(program,shader);gl.deleteShader(shader);
  gl.useProgram(program);gl.bindTransformFeedback(gl.TRANSFORM_FEEDBACK,feedback);
  gl.bindBufferBase(gl.TRANSFORM_FEEDBACK_BUFFER,0,buffer);
  gl.bindBuffer(gl.TRANSFORM_FEEDBACK_BUFFER,null);
  gl.beginTransformFeedback(gl.POINTS);gl.pauseTransformFeedback();
  gl.bindTransformFeedback(gl.TRANSFORM_FEEDBACK,null);gl.deleteProgram(program);gl.useProgram(null);
  assert.equal(f.retained(feedback),true);assert.equal(f.retained(program),true);assert.equal(f.retained(buffer),true);assert.equal(f.retained(shader),true);
  gl.bindTransformFeedback(gl.TRANSFORM_FEEDBACK,feedback);gl.endTransformFeedback();
  gl.bindTransformFeedback(gl.TRANSFORM_FEEDBACK,null);
  assert.equal(f.retained(feedback),false);assert.equal(f.retained(program,[feedback]),false);assert.equal(f.retained(shader,[program]),false);
  assert.equal(f.retained(buffer,[feedback]),true);
});

test('rejected texture-unit changes preserve the previous unit and bound objects',()=>{
  const f=fixture({reject:op=>op.kind==='command'&&op.value.method==='activeTexture'&&op.value.args.texture===0}),gl=f.gl;
  const first=gl.createTexture(),second=gl.createTexture();
  gl.bindTexture(gl.TEXTURE_2D,first);gl.activeTexture(0);gl.bindTexture(gl.TEXTURE_2D,second);
  assert.equal(f.retained(first),false);assert.equal(f.retained(second),true);
  gl.activeTexture(gl.TEXTURE0+1);gl.bindTexture(gl.TEXTURE_2D,first);
  gl.bindTexture(gl.TEXTURE_2D,null);assert.equal(f.retained(first),false);assert.equal(f.retained(second),true);
});

test('late finalizers cannot collect a re-created wrapper for the same native object',()=>{
  let id=0;
  const f=fixture({query:q=>q.method==='getParameter'?{type:'object',value:{kind:'buffer',id}}:null}),gl=f.gl;
  const original=gl.createBuffer();id=f.metadata.get(original).id;
  const registration=f.registrations.find(r=>r.target===original);
  registration.record.reference.target=undefined;
  const replacement=gl.getParameter(gl.ARRAY_BUFFER_BINDING);
  assert.notEqual(replacement,original);
  const before=f.calls.filter(c=>c.operation?.kind==='collect').length;
  registration.callback(registration.record);
  assert.equal(f.calls.filter(c=>c.operation?.kind==='collect').length,before);
  // A live wrapper also prevents a premature cleanup callback.
  const fresh=f.registrations.find(r=>r.target===replacement);fresh.callback(fresh.record);
  assert.equal(f.calls.filter(c=>c.operation?.kind==='collect').length,before);
});

test('native compositor loss reaches the page without another GL call or timer',()=>{
  const f=fixture({noTimers:true}),gl=f.gl,buffer=gl.createBuffer();
  gl.bindBuffer(gl.ARRAY_BUFFER,buffer);const calls=f.calls.length;
  f.loseNative();assert.deepEqual(f.events,[]);f.drain();
  assert.deepEqual(f.events,['webglcontextlost']);assert.equal(gl.drawingBufferWidth,0);
  assert.equal(f.calls.length,calls);assert.equal(f.retained(buffer),false);
  f.loseNative();f.drain();assert.deepEqual(f.events,['webglcontextlost']);
});

test('native and synchronous loss paths coalesce and restoration can lose again',()=>{
  const f=fixture({noTimers:true}),ext=f.gl.getExtension('WEBGL_lose_context');
  f.canvas.listener=e=>e.preventDefault();f.loseNative();ext.loseContext();f.drain();
  assert.deepEqual(f.events,['webglcontextlost']);
  ext.restoreContext();f.drain();f.loseNative();f.drain();
  assert.deepEqual(f.events,['webglcontextlost','webglcontextrestored','webglcontextlost']);
});

test('navigation discards queued native notifications and creation/loss/restoration events',()=>{
  const native=fixture();native.loseNative();native.advanceDocument();native.drain();assert.deepEqual(native.events,[]);
  const loss=fixture();loss.gl.getExtension('WEBGL_lose_context').loseContext();loss.advanceDocument();loss.drain();assert.deepEqual(loss.events,[]);
  const failed=fixture({fail:true});failed.advanceDocument();failed.drain();assert.deepEqual(failed.events,[]);
  const restore=fixture();restore.canvas.listener=e=>e.preventDefault();const ext=restore.gl.getExtension('WEBGL_lose_context');
  ext.loseContext();restore.drain();ext.restoreContext();restore.advanceDocument();restore.drain();
  assert.deepEqual(restore.events,['webglcontextlost']);
});

test('failed lifecycle registration frees the native context and canvas type',()=>{
  const f=fixture({watchFails:true,noTimers:true});assert.equal(f.gl,null);
  assert.equal(f.native.size,0);assert.equal(f.sandbox._webglHas(f.canvas),false);
  f.drain();assert.deepEqual(f.events,['webglcontextcreationerror']);
});

const bitmapSource=f=>new f.sandbox.ImageData(new Uint8ClampedArray([
  255,0,0,255, 0,255,0,255, 0,0,255,128, 17,33,65,0
]),2);
async function bitmap(f,...args) {
  const pending=f.sandbox.createImageBitmap(...args);
  // Install a handler immediately while this fixture pumps the host task queue.
  pending.catch(()=>{});
  for(let i=0;i<4;i++){await new Promise(setImmediate);f.drain();}
  return pending;
}
function bitmapUpload(f,image) {
  f.gl.texImage2D(f.gl.TEXTURE_2D,0,f.gl.RGBA,f.gl.RGBA,f.gl.UNSIGNED_BYTE,image);
  return f.calls.filter(c=>c.operation?.kind==='textureSource').at(-1);
}
test('ImageBitmap owns a snapshot with private readonly dimensions and releases on close',async()=>{
  const f=fixture(),input=bitmapSource(f),pending=bitmap(f,input);input.data.fill(7);
  const image=await pending;
  assert.equal(image.width,2);assert.equal(image.height,2);assert.deepEqual(Object.keys(image),[]);assert.deepEqual(Object.getOwnPropertySymbols(image),[]);
  assert.throws(()=>new f.sandbox.ImageBitmap(),/Illegal constructor/);
  const get=Object.getOwnPropertyDescriptor(f.sandbox.ImageBitmap.prototype,'width').get;
  assert.throws(()=>get.call({}),/Illegal invocation/);
  assert.throws(()=>image.close.call({}),/Illegal invocation/);
  assert.deepEqual(bitmapUpload(f,image).data,Array.from(bitmapSource(f).data));
  assert.equal(bitmapUpload(f,image).operation.value.bitmap,true);
  image.close();image.close();assert.equal(image.width,0);assert.equal(image.height,0);
  const before=f.calls.filter(c=>c.operation?.kind==='textureSource').length;
  bitmapUpload(f,image);assert.equal(f.gl.getError(),f.gl.INVALID_VALUE);
  assert.equal(f.calls.filter(c=>c.operation?.kind==='textureSource').length,before);
  await assert.rejects(f.sandbox.createImageBitmap(image),{name:'InvalidStateError'});
});
test('ImageBitmap crop normalizes negative sizes and pads outside pixels transparently',async()=>{
  const f=fixture();
  const padded=await bitmap(f,bitmapSource(f),-1,0,2,1);
  assert.deepEqual(bitmapUpload(f,padded).data,[0,0,0,0,255,0,0,255]);
  const negative=await bitmap(f,bitmapSource(f),2,1,-2,-1);
  assert.deepEqual(bitmapUpload(f,negative).data,[255,0,0,255,0,255,0,255]);
  const flipped=await bitmap(f,bitmapSource(f),{imageOrientation:'flipY'});
  assert.deepEqual(bitmapUpload(f,flipped).data,[0,0,255,128,17,33,65,0,255,0,0,255,0,255,0,255]);
});
test('ImageBitmap alpha options apply once and a clone survives closing the original',async()=>{
  const f=fixture(),input=new f.sandbox.ImageData(new Uint8ClampedArray([128,64,32,128]),1,1);
  const image=await bitmap(f,input,{premultiplyAlpha:'premultiply'});
  assert.deepEqual(bitmapUpload(f,image).data,[64,32,16,128]);
  const clone=await bitmap(f,image,{premultiplyAlpha:'none'});image.close();
  assert.deepEqual(bitmapUpload(f,clone).data,[128,64,32,128]);
  f.gl.pixelStorei(f.gl.UNPACK_FLIP_Y_WEBGL,true);f.gl.pixelStorei(f.gl.UNPACK_PREMULTIPLY_ALPHA_WEBGL,true);
  assert.equal(bitmapUpload(f,clone).operation.value.bitmap,true);
});
test('ImageBitmap resizes with preserved aspect ratio and pixelated integer scaling',async()=>{
  const f=fixture(),input=new f.sandbox.ImageData(new Uint8ClampedArray([255,0,0,255,0,255,0,255]),2,1);
  const image=await bitmap(f,input,{resizeWidth:4,resizeQuality:'pixelated'});
  assert.equal(image.width,4);assert.equal(image.height,2);
  assert.deepEqual(bitmapUpload(f,image).data.slice(0,16),[255,0,0,255,255,0,0,255,0,255,0,255,0,255,0,255]);
  const tall=await bitmap(f,input,{resizeHeight:2});assert.equal(tall.width,4);assert.equal(tall.height,2);
  const smooth=await bitmap(f,input,{resizeWidth:1,resizeHeight:1,resizeQuality:'high'});
  assert.deepEqual(bitmapUpload(f,smooth).data,[128,128,0,255]);
});
test('ImageBitmap smooth scaling uses alpha weighted colors without transparent fringes',async()=>{
  const f=fixture(),input=new f.sandbox.ImageData(new Uint8ClampedArray([255,0,0,255,0,255,0,0]),2,1);
  const image=await bitmap(f,input,{resizeWidth:1,resizeHeight:1});
  assert.deepEqual(bitmapUpload(f,image).data,[255,0,0,128]);
  const premul=await bitmap(f,input,{resizeWidth:1,resizeHeight:1,premultiplyAlpha:'premultiply'});
  assert.deepEqual(bitmapUpload(f,premul).data,[128,0,0,128]);
});
test('ImageBitmap option conversion rejects invalid and excessive allocations',async()=>{
  const f=fixture();
  for(const options of [7,{imageOrientation:'bad'},{premultiplyAlpha:'bad'},{colorSpaceConversion:'bad'},
    {resizeQuality:'bad'},{resizeWidth:-1},{resizeHeight:Infinity},{resizeWidth:4294967296},{resizeWidth:1n}])
    await assert.rejects(f.sandbox.createImageBitmap(bitmapSource(f),options),{name:'TypeError'});
  for(const options of [{resizeWidth:0},{resizeHeight:0},{resizeWidth:32768},{resizeWidth:4097,resizeHeight:4096}])
    await assert.rejects(f.sandbox.createImageBitmap(bitmapSource(f),options),{name:'InvalidStateError'});
  await assert.rejects(f.sandbox.createImageBitmap(bitmapSource(f),0,0,0,1),{name:'RangeError'});
  await assert.rejects(f.sandbox.createImageBitmap(bitmapSource(f),0,0),{name:'TypeError'});
  await assert.rejects(f.sandbox.createImageBitmap(),{name:'TypeError'});
  await assert.rejects(f.sandbox.createImageBitmap({}),{name:'TypeError'});
  const detached=new f.sandbox.ImageData(1,1);structuredClone(detached.data.buffer,{transfer:[detached.data.buffer]});
  await assert.rejects(f.sandbox.createImageBitmap(detached),{name:'InvalidStateError'});
});
test('ImageBitmap retains taint including an image changing origin permission during decode',async()=>{
  for(const options of [{originClean:false},{imagePixelsResult:3}]) {
    const f=fixture(options),image=await bitmap(f,new f.sandbox.HTMLImageElement());
    assert.equal(image.width,2);assert.throws(()=>bitmapUpload(f,image),{name:'SecurityError'});
    const clone=await bitmap(f,image);assert.throws(()=>bitmapUpload(f,clone),{name:'SecurityError'});
    assert(f.calls.filter(c=>c.imagePixels).every(c=>c.allowTainted));
  }
});
test('ImageBitmap copies 2D and WebGL canvas pixels without retaining a live source',async()=>{
  const f=fixture(),canvas=new f.sandbox.HTMLCanvasElement();
  canvas._ctx={_w:1,_h:1,_buf:new Uint8ClampedArray([11,22,33,44])};
  const image=await bitmap(f,canvas);canvas._ctx._buf.fill(0);
  assert.deepEqual(bitmapUpload(f,image).data,[11,22,33,44]);
  const webgl=await bitmap(f,f.canvas);assert(bitmapUpload(f,webgl).data.every(v=>v===42));
  const empty=new f.sandbox.HTMLCanvasElement();empty.width=0;
  await assert.rejects(f.sandbox.createImageBitmap(empty),{name:'InvalidStateError'});
});
test('ImageBitmap Blob decoding rejects both metadata and pixel failures',async()=>{
  const f=fixture(),image=await bitmap(f,new Blob(['encoded image']));
  assert.deepEqual(bitmapUpload(f,image).data,[17,33,65,128]);
  for(const options of [{decodeFails:true},{decodePixelsFail:true}]) {
    const f=fixture(options),pending=bitmap(f,new Blob(['invalid']));
    await assert.rejects(pending,{name:'InvalidStateError'});
  }
});
test('ImageBitmap resolution waits for its task and navigation discards stale completion',async()=>{
  const f=fixture();let resolved=false;
  const pending=f.sandbox.createImageBitmap(bitmapSource(f)).then(()=>{resolved=true;});
  await Promise.resolve();assert.equal(resolved,false);f.drain();await pending;assert.equal(resolved,true);
  let stale=false;f.sandbox.createImageBitmap(bitmapSource(f)).then(()=>{stale=true;});
  f.advanceDocument();await Promise.resolve();f.drain();await Promise.resolve();assert.equal(stale,false);
});

function bitmapDestination(width=2,height=2) {
  return {_w:width,_h:height,_buf:new Uint8ClampedArray(width*height*4),globalAlpha:1,globalCompositeOperation:'source-over',damage:0,_markPaintDamage(){this.damage++;}};
}
test('ImageBitmap drawing writes real 2D pixels with alpha and crop clipping',async()=>{
  const f=fixture(),image=await bitmap(f,bitmapSource(f)),destination=bitmapDestination();
  assert.equal(f.sandbox._canvasBitmapDraw(destination,image,[0,0]),true);
  assert.deepEqual(Array.from(destination._buf).slice(0,12),[255,0,0,255,0,255,0,255,0,0,255,128]);
  assert.equal(destination.damage,1);
  const crop=bitmapDestination(1,1);f.sandbox._canvasBitmapDraw(crop,image,[1,0,1,1,0,0,1,1]);
  assert.deepEqual(Array.from(crop._buf),[0,255,0,255]);
  const huge=bitmapDestination(1,1);f.sandbox._canvasBitmapDraw(huge,image,[-1e10,-1e10,2e10,2e10]);assert.equal(huge.damage,1);
});
test('ImageBitmap drawing converts premultiplied input and applies source-over and multiply',async()=>{
  const f=fixture(),image=await bitmap(f,new f.sandbox.ImageData(new Uint8ClampedArray([128,64,32,128]),1,1),{premultiplyAlpha:'premultiply'});
  for(const mode of ['source-over','multiply','copy']) {
    const destination=bitmapDestination(1,1);destination.globalCompositeOperation=mode;
    f.sandbox._canvasBitmapDraw(destination,image,[0,0]);
    assert.deepEqual(Array.from(destination._buf),[128,64,32,128]);
  }
  const destination=bitmapDestination(1,1);destination._buf.set([255,255,255,255]);destination.globalAlpha=.5;
  f.sandbox._canvasBitmapDraw(destination,image,[0,0]);
  assert.equal(destination._buf[3],255);assert(destination._buf[0]>128&&destination._buf[0]<255);
});
test('ImageBitmap drawing preserves taint and validates closed sources and overloads',async()=>{
  const f=fixture({originClean:false}),image=await bitmap(f,new f.sandbox.HTMLImageElement()),destination=bitmapDestination();
  f.sandbox._canvasBitmapDraw(destination,image,[0,0]);assert.equal(destination.clean,false);
  assert.equal(f.sandbox._canvasBitmapDraw(destination,{},[0,0]),false);
  assert.throws(()=>f.sandbox._canvasBitmapDraw(destination,image,[0]),{name:'TypeError'});
  image.close();assert.throws(()=>f.sandbox._canvasBitmapDraw(destination,image,[0,0]),{name:'InvalidStateError'});
});
test('ImageBitmap drawing handles negative extents and does no work for empty or invalid rectangles',async()=>{
  const f=fixture(),image=await bitmap(f,bitmapSource(f)),destination=bitmapDestination();
  f.sandbox._canvasBitmapDraw(destination,image,[2,2,-2,-2]);assert.equal(destination.damage,1);
  assert.deepEqual(Array.from(destination._buf).slice(0,4),[255,0,0,255]);
  const before=Array.from(destination._buf);
  for(const args of [[NaN,0],[0,0,0,1],[0,0,1,0],[0,0,0,1,0,0,1,1]])f.sandbox._canvasBitmapDraw(destination,image,args);
  assert.deepEqual(Array.from(destination._buf),before);assert.equal(destination.damage,1);
});

test('resource predicates reject foreign and invalidated objects without changing GL errors',()=>{
  const f=fixture({query:()=>({type:'bool',value:true})});
  const other=f.sandbox._webglCreate(new f.sandbox.HTMLCanvasElement(),'webgl',{});
  for(const suffix of ['Buffer','Texture','Program','Shader','Framebuffer','Renderbuffer']){
    const value=f.gl['create'+suffix](f.gl.VERTEX_SHADER);
    assert.equal(f.gl['is'+suffix](value),true);
    f.native.get(1).error=f.gl.INVALID_ENUM;
    assert.equal(other['is'+suffix](value),false);
    assert.equal(other.getError(),other.NO_ERROR);
    assert.equal(f.gl.getError(),f.gl.INVALID_ENUM);
    assert.equal(f.gl['is'+suffix](null),false);
    assert.throws(()=>f.gl['is'+suffix]({}),/Expected WebGL/);
  }
  const old=f.gl.createBuffer(),loss=f.gl.getExtension('WEBGL_lose_context');
  f.canvas.listener=event=>event.preventDefault();loss.loseContext();f.drain();
  assert.equal(f.gl.isBuffer(old),false);loss.restoreContext();f.drain();
  assert.equal(f.gl.isBuffer(old),false);assert.equal(f.gl.getError(),f.gl.NO_ERROR);
  assert.throws(()=>f.gl.isTexture(f.gl.createBuffer()),/Expected WebGL/);
});
test('WebGL two and OES predicates use the same context ownership rules',()=>{
  const f=fixture({query:arg=>arg.method==='fenceSync'?{type:'uint',value:700}:{type:'bool',value:true}});
  const a=f.sandbox._webglCreate(new f.sandbox.HTMLCanvasElement(),'webgl2',{});
  const b=f.sandbox._webglCreate(new f.sandbox.HTMLCanvasElement(),'webgl2',{});
  for(const suffix of ['VertexArray','Query','Sampler','TransformFeedback']){
    const value=a['create'+suffix]();assert.equal(a['is'+suffix](value),true);
    assert.equal(b['is'+suffix](value),false);assert.equal(b.getError(),b.NO_ERROR);
  }
  const sync=a.fenceSync(a.SYNC_GPU_COMMANDS_COMPLETE,0);
  assert.equal(a.isSync(sync),true);assert.equal(b.isSync(sync),false);
  assert.equal(b.isSync(null),false);assert.equal(b.getError(),b.NO_ERROR);
  const other=f.sandbox._webglCreate(new f.sandbox.HTMLCanvasElement(),'webgl',{});
  const first=f.gl.getExtension('OES_vertex_array_object'),second=other.getExtension('OES_vertex_array_object');
  const array=first.createVertexArrayOES();assert.equal(first.isVertexArrayOES(array),true);
  assert.equal(second.isVertexArrayOES(array),false);assert.equal(other.getError(),other.NO_ERROR);
});
test('native loss returns a boolean false from object predicates',()=>{
  const f=fixture({query:()=>({type:'null'})}),buffer=f.gl.createBuffer();
  f.loseNative();assert.equal(f.gl.isBuffer(buffer),false);f.drain();
  assert.deepEqual(f.events,['webglcontextlost']);
});
test('half float extension constants are complete readonly and cached',()=>{
  const f=fixture(),ext=f.gl.getExtension('EXT_color_buffer_half_float');
  for(const [name,value] of Object.entries({RGBA16F_EXT:0x881A,RGB16F_EXT:0x881B,FRAMEBUFFER_ATTACHMENT_COMPONENT_TYPE_EXT:0x8211,UNSIGNED_NORMALIZED_EXT:0x8C17})){
    assert.equal(ext[name],value);const d=Object.getOwnPropertyDescriptor(ext,name);
    assert.equal(d.writable,false);assert.equal(d.configurable,false);assert.equal(d.enumerable,true);
    assert.equal(Reflect.set(ext,name,0),false);
  }
  assert.equal(f.gl.getExtension('ext_COLOR_buffer_HALF_float'),ext);
  assert.equal(f.gl.getExtension('not_a_real_extension'),null);
  const loss=f.gl.getExtension('WEBGL_lose_context');loss.loseContext();
  assert.equal(f.gl.getExtension('EXT_color_buffer_half_float'),null);
});

test('lost queries keep scalar return types even when native replies have no payload',()=>{
  for(const retired of [false,true]){
    const f=fixture({emptyLossReply:true});
    const gl=f.sandbox._webglCreate(new f.sandbox.HTMLCanvasElement(),'webgl2',{}),program=gl.createProgram();
    if(retired)f.native.delete(2);else f.native.get(2).lost=true;
    // First query discovers loss; subsequent object queries use stale wrappers.
    assert.equal(gl.getAttribLocation(program,'position'),-1);
    assert.equal(gl.isContextLost(),true);assert.equal(gl.isEnabled(gl.BLEND),false);
    assert.equal(gl.checkFramebufferStatus(0),gl.FRAMEBUFFER_UNSUPPORTED);
    assert.equal(gl.getVertexAttribOffset(999,0),0);
    assert.equal(gl.getUniformBlockIndex(program,'Block'),0);
    assert.equal(gl.getFragDataLocation(program,'color'),-1);
    assert.equal(gl.getParameter(gl.VERSION),null);assert.equal(gl.getUniformLocation(program,'value'),null);
    assert.equal(gl.getContextAttributes(),null);assert.equal(gl.getSupportedExtensions(),null);
    assert.equal(gl.getExtension('OES_texture_float'),null);assert.equal(gl.createBuffer(),null);
    assert.equal(gl.isProgram(program),false);assert.equal(gl.fenceSync(gl.SYNC_GPU_COMMANDS_COMPLETE,0),null);
    assert.equal(gl.getError(),gl.CONTEXT_LOST_WEBGL);assert.equal(gl.getError(),gl.NO_ERROR);
    f.drain();assert.deepEqual(f.events,['webglcontextlost']);
  }
});

test('each loss reports one error including callback first discovery and restoration',()=>{
  for(const callbackFirst of [false,true]){
    const f=fixture({emptyLossReply:true}),gl=f.gl;
    f.canvas.listener=event=>event.preventDefault();const extension=gl.getExtension('WEBGL_lose_context');
    for(let round=0;round<2;round++){
      if(callbackFirst){f.loseNative();f.drain();}else f.native.get(1).lost=true;
      assert.equal(gl.getError(),gl.CONTEXT_LOST_WEBGL);assert.equal(gl.getError(),gl.NO_ERROR);
      gl.enable(0);assert.equal(gl.getError(),gl.NO_ERROR);
      f.drain();extension.restoreContext();f.drain();
      assert.equal(gl.isContextLost(),false);assert.equal(gl.getError(),gl.NO_ERROR);
    }
    assert.deepEqual(f.events,['webglcontextlost','webglcontextrestored','webglcontextlost','webglcontextrestored']);
  }
});

test('first lost advanced queries have typed fallbacks and wait failures',()=>{
  for(const method of ['getVertexAttribOffset','getUniformBlockIndex','getFragDataLocation','clientWaitSync']){
    const f=fixture({emptyLossReply:true,query:request=>request.method==='fenceSync'?{type:'uint',value:101}:null});
    const gl=f.sandbox._webglCreate(new f.sandbox.HTMLCanvasElement(),'webgl2',{});
    const program=gl.createProgram(),sync=gl.fenceSync(gl.SYNC_GPU_COMMANDS_COMPLETE,0);
    f.native.get(2).lost=true;
    const calls={getVertexAttribOffset:()=>gl.getVertexAttribOffset(0,gl.VERTEX_ATTRIB_ARRAY_POINTER),
      getUniformBlockIndex:()=>gl.getUniformBlockIndex(program,'Block'),
      getFragDataLocation:()=>gl.getFragDataLocation(program,'color'),clientWaitSync:()=>gl.clientWaitSync(sync,0,0)};
    assert.equal(calls[method](),method==='clientWaitSync'?gl.WAIT_FAILED:method==='getFragDataLocation'?-1:0);
    assert.equal(gl.getError(),gl.CONTEXT_LOST_WEBGL);assert.equal(gl.getError(),gl.NO_ERROR);
  }
});

test('fragment output queries preserve version branding ownership and argument conversion',()=>{
  const f=fixture({query:request=>({type:'int',value:request.args.name==='color'?2:-1})});
  assert.equal(typeof f.gl.getFragDataLocation,'undefined');
  const gl=f.sandbox._webglCreate(new f.sandbox.HTMLCanvasElement(),'webgl2',{}),program=gl.createProgram();
  assert.equal(gl.getFragDataLocation.length,2);assert.equal(gl.getFragDataLocation(program,'color'),2);
  assert.equal(f.calls.at(-1).operation.kind,'advanced');assert.equal(f.calls.at(-1).operation.value.method,'getFragDataLocation');
  assert.equal(gl.getFragDataLocation(program,{toString:()=> 'absent'}),-1);assert.equal(gl.getError(),gl.NO_ERROR);
  assert.throws(()=>gl.getFragDataLocation({},'color'),/Expected WebGL/);
  assert.throws(()=>gl.getFragDataLocation(program,Symbol()),/Symbol/);
  assert.throws(()=>gl.getFragDataLocation(program),/requires/);
  assert.throws(()=>gl.getFragDataLocation.call({},program,'color'),/Illegal invocation/);
  assert.equal(gl.getFragDataLocation(f.gl.createProgram(),'color'),-1);assert.equal(gl.getError(),gl.INVALID_OPERATION);
  assert.equal(gl.getUniformBlockIndex(f.gl.createProgram(),'Block'),0);assert.equal(gl.getError(),gl.INVALID_OPERATION);
});

test('live scalar queries preserve successful values and existing GL errors',()=>{
  const values={isEnabled:{type:'bool',value:true},checkFramebufferStatus:{type:'uint',value:0x8CD5},
    getVertexAttribOffset:{type:'number',value:32},getUniformBlockIndex:{type:'uint',value:0xFFFFFFFF}};
  const f=fixture({query:request=>values[request.method]});
  const gl=f.sandbox._webglCreate(new f.sandbox.HTMLCanvasElement(),'webgl2',{});
  gl.bindBuffer(gl.ARRAY_BUFFER,f.gl.createBuffer());
  assert.equal(gl.isEnabled(gl.BLEND),true);assert.equal(gl.checkFramebufferStatus(gl.FRAMEBUFFER),gl.FRAMEBUFFER_COMPLETE);
  assert.equal(gl.getVertexAttribOffset(0,gl.VERTEX_ATTRIB_ARRAY_POINTER),32);
  assert.equal(gl.getUniformBlockIndex(gl.createProgram(),'absent'),gl.INVALID_INDEX);
  assert.equal(gl.getError(),gl.INVALID_OPERATION);assert.equal(gl.getError(),gl.NO_ERROR);
});

test('drawing storage receipts update dimensions and readback without changing canvas size',()=>{
  const f=fixture(),{gl,canvas,sandbox}=f;
  assert.equal(gl.drawingBufferFormat,0x8058);assert.equal(gl.drawingBufferStorage.length,3);
  assert.equal(gl.drawingBufferStorage(0x881A,2,5),undefined);
  assert.deepEqual([gl.drawingBufferWidth,gl.drawingBufferHeight,gl.drawingBufferFormat],[2,5,0x881A]);
  assert.deepEqual([canvas.width,canvas.height],[4,3]);
  const image=sandbox._webglReadback(canvas);assert.equal(image.width,2);assert.equal(image.height,5);assert.equal(image.bytes.length,40);
  assert.equal(Object.getOwnPropertyDescriptor(sandbox.WebGLRenderingContext.prototype,'drawingBufferFormat').set,undefined);
  assert.throws(()=>gl.drawingBufferStorage.call({},0x8058,2,2),/Illegal invocation/);
  assert.throws(()=>gl.drawingBufferStorage(0x8058,2),{name:'TypeError'});
  assert.throws(()=>gl.drawingBufferStorage(0x8058,1n,2),{name:'TypeError'});
});
test('rejected storage does not update cached size format or serialization allocation',()=>{
  const f=fixture({storageError:0x0505});f.gl.drawingBufferStorage(0x881A,12,9);
  assert.deepEqual([f.gl.drawingBufferWidth,f.gl.drawingBufferHeight,f.gl.drawingBufferFormat],[4,3,0x8058]);
  assert.equal(f.gl.getError(),0x0505);assert.equal(f.sandbox._webglReadback(f.canvas).bytes.length,48);
});
test('canvas resize retains storage format and restoration uses canvas intrinsic dimensions',()=>{
  const f=fixture(),{gl,canvas,sandbox}=f;
  gl.drawingBufferStorage(0x881A,7,9);canvas.width=6;canvas.height=2;sandbox._webglResize(canvas);
  assert.deepEqual([gl.drawingBufferWidth,gl.drawingBufferHeight,gl.drawingBufferFormat],[6,2,0x881A]);
  gl.drawingBufferStorage(0x8058,2,2);canvas.listener=event=>event.preventDefault();
  const extension=gl.getExtension('WEBGL_lose_context');extension.loseContext();f.drain();
  assert.deepEqual([gl.drawingBufferWidth,gl.drawingBufferHeight,gl.drawingBufferFormat],[0,0,0]);
  gl.drawingBufferStorage(0x881A,30,30);extension.restoreContext();f.drain();
  assert.deepEqual([gl.drawingBufferWidth,gl.drawingBufferHeight,gl.drawingBufferFormat],[6,2,0x8058]);
});
test('storage WebIDL conversion and state stay private to each context',()=>{
  const f=fixture(),second=new f.sandbox.HTMLCanvasElement();
  const other=f.sandbox._webglCreate(second,'webgl2',{alpha:false});
  f.gl.drawingBufferStorage(0x8058,2.9,3.8);
  const call=f.calls.findLast(call=>call.operation?.kind==='drawingBufferStorage');
  assert.deepEqual(JSON.parse(JSON.stringify(call.operation.value)),{format:0x8058,width:2,height:3});
  assert.deepEqual([other.drawingBufferWidth,other.drawingBufferHeight,other.drawingBufferFormat],[4,3,0x8051]);
  assert.deepEqual(Object.keys(f.gl),[]);assert.deepEqual(Object.getOwnPropertySymbols(f.gl),[]);
});

test('color attributes validate enums and keep setters privately branded',()=>{
  const f=fixture(),{gl,sandbox}=f;assert.equal(gl.drawingBufferColorSpace,'srgb');assert.equal(gl.unpackColorSpace,'srgb');
  gl.drawingBufferColorSpace={toString:()=> 'display-p3'};gl.unpackColorSpace='display-p3';
  assert.equal(gl.drawingBufferColorSpace,'display-p3');assert.equal(gl.unpackColorSpace,'display-p3');
  const before=f.calls.length;gl.drawingBufferColorSpace='display-p3';assert.equal(f.calls.length,before);
  for(const name of ['drawingBufferColorSpace','unpackColorSpace']){
    const descriptor=Object.getOwnPropertyDescriptor(sandbox.WebGLRenderingContext.prototype,name);
    assert.equal(descriptor.enumerable,true);assert.equal(descriptor.configurable,true);
    assert.throws(()=>descriptor.get.call({}),/Illegal invocation/);assert.throws(()=>descriptor.set.call({},'srgb'),/Illegal invocation/);
    for(const value of ['',null,undefined,'p3','SRGB',Symbol()])assert.throws(()=>{gl[name]=value;},{name:'TypeError'});
    assert.equal(gl[name],'display-p3');
  }
  assert.equal(f.calls.length,before);assert.deepEqual(Object.keys(gl),[]);
});
test('lost color attributes retain valid values through context restoration',()=>{
  const f=fixture();f.canvas.listener=event=>event.preventDefault();
  const ext=f.gl.getExtension('WEBGL_lose_context');ext.loseContext();f.drain();
  f.gl.drawingBufferColorSpace='display-p3';f.gl.unpackColorSpace='display-p3';
  assert.equal(f.gl.drawingBufferColorSpace,'display-p3');assert.equal(f.gl.unpackColorSpace,'display-p3');
  ext.restoreContext();f.drain();assert.equal(f.gl.drawingBufferColorSpace,'display-p3');assert.equal(f.gl.unpackColorSpace,'display-p3');
});
test('source uploads carry color and alpha metadata while typed arrays remain numeric',async()=>{
  const f=fixture(),{gl}=f,input=new f.sandbox.ImageData(new Uint8ClampedArray([128,0,0,128]),1,1, {colorSpace:'display-p3'});
  gl.unpackColorSpace='display-p3';gl.texImage2D(gl.TEXTURE_2D,0,gl.RGBA,gl.RGBA,gl.UNSIGNED_BYTE,input);
  let upload=f.calls.findLast(c=>c.operation?.kind==='textureSource');assert.equal(upload.operation.value.source_space,'display-p3');assert.equal(upload.operation.value.source_premultiplied,false);
  const image=await bitmap(f,input,{premultiplyAlpha:'premultiply'});upload=bitmapUpload(f,image);
  assert.equal(upload.operation.value.source_space,'display-p3');assert.equal(upload.operation.value.source_premultiplied,true);
  gl.texImage2D(gl.TEXTURE_2D,0,gl.RGBA,1,1,0,gl.RGBA,gl.UNSIGNED_BYTE,new Uint8Array([255,0,0,255]));
  assert.equal(f.calls.at(-1).operation.kind,'textureImage');assert(!f.calls.some(call=>call.convertColor));
  image.close();
});
test('WebGL canvas sources request raw color pixels and keep wide gamut in bitmaps',async()=>{
  const f=fixture();f.gl.drawingBufferColorSpace='display-p3';
  const image=await bitmap(f,f.canvas);const upload=bitmapUpload(f,image);
  assert.equal(upload.operation.value.source_space,'display-p3');assert(f.calls.some(c=>c.operation?.kind==='sourceReadback'));
  const browser=f.sandbox._webglReadback(f.canvas);assert.equal(browser.colorSpace,'srgb');
  const raw=f.sandbox._webglReadback(f.canvas,true);assert.equal(raw.colorSpace,'display-p3');
  assert(!f.calls.some(call=>call.convertColor));image.close();
});
test('integer WebIDL arguments reject BigInt before native operations',()=>{
  const f=fixture();const before=f.calls.length;
  for(const action of [()=>f.gl.enable(1n),()=>f.gl.viewport(1n,0,1,1),()=>f.gl.drawingBufferStorage(f.gl.RGBA8,1n,1)])
    assert.throws(action,{name:'TypeError'});
  assert.equal(f.calls.length,before);
});

test('ImageData validates dimensions settings and supported storage before allocation',()=>{
  const f=fixture(),Image=f.sandbox.ImageData;
  assert.throws(()=>new Image(),{name:'TypeError'});assert.throws(()=>Image(1,1),{name:'TypeError'});
  for(const args of [[0,1],[1,0],[NaN,1],[1,Infinity]])assert.throws(()=>new Image(...args),{name:'IndexSizeError'});
  for(const args of [[-1,1],[32768,1],[4097,4096]])assert.throws(()=>new Image(...args),{name:'RangeError'});
  for(const value of [1n,Symbol()])assert.throws(()=>new Image(value,1),{name:'TypeError'});
  for(const settings of [1,{colorSpace:'p3'},{pixelFormat:'invalid'},{colorSpace:Symbol()}])assert.throws(()=>new Image(1,1,settings),{name:'TypeError'});
  assert.throws(()=>new Image(1,1,{pixelFormat:'rgba-float16'}),{name:'NotSupportedError'});
  const image=new Image('2.9',1,null);assert.equal(image.width,2);assert.equal(image.data.length,8);
  assert.equal(image.colorSpace,'srgb');assert.equal(image.pixelFormat,'rgba-unorm8');assert(image.data.every(v=>v===0));
  const failed=fixture({imageDataAllocationFails:true});assert.throws(()=>new failed.sandbox.ImageData(1,1),{name:'RangeError'});
});
test('ImageData preserves supplied views and rejects malformed detached or shared buffers',()=>{
  const f=fixture(),Image=f.sandbox.ImageData,buffer=new ArrayBuffer(16),data=new Uint8ClampedArray(buffer,4,8);
  const image=new Image(data,1,undefined,{colorSpace:'display-p3'});assert.equal(image.data,data);assert.equal(image.height,2);assert.equal(image.colorSpace,'display-p3');
  for(const args of [[new Uint8ClampedArray(0),1],[new Uint8ClampedArray(3),1]])assert.throws(()=>new Image(...args),{name:'InvalidStateError'});
  for(const args of [[data,3],[data,1,3],[data,0]])assert.throws(()=>new Image(...args),{name:'IndexSizeError'});
  assert.throws(()=>new Image(new Uint8Array(4),1),{name:'TypeError'});
  assert.throws(()=>new Image(new Uint8ClampedArray(new SharedArrayBuffer(4)),1),{name:'TypeError'});
  if(Object.getOwnPropertyDescriptor(ArrayBuffer.prototype,'resizable'))assert.throws(()=>new Image(new Uint8ClampedArray(new ArrayBuffer(4,{maxByteLength:8})),1),{name:'TypeError'});
  structuredClone(buffer,{transfer:[buffer]});assert.throws(()=>new Image(data,1),{name:'InvalidStateError'});
  assert.throws(()=>bitmapUpload(f,image),{name:'InvalidStateError'});assert.equal(image.width,1);assert.equal(image.height,2);assert.equal(image.data,data);
});
test('ImageData readonly metadata and native source branding ignore page-owned shadows',()=>{
  const f=fixture(),Image=f.sandbox.ImageData,image=new Image(new Uint8ClampedArray([255,0,0,255]),1,1,{colorSpace:'display-p3'});
  assert.deepEqual(Object.keys(image),[]);assert.deepEqual(Object.getOwnPropertySymbols(image),[]);assert.equal(Object.prototype.toString.call(image),'[object ImageData]');
  for(const name of ['width','height','data','colorSpace','pixelFormat']){
    const descriptor=Object.getOwnPropertyDescriptor(Image.prototype,name);assert(descriptor.enumerable&&descriptor.configurable);assert.equal(descriptor.set,undefined);
    assert.throws(()=>descriptor.get.call({}),{name:'TypeError'});assert.equal(Reflect.set(image,name,0),false);
    Object.defineProperty(image,name,{get(){throw Error('page shadow must not run');}});
  }
  const upload=bitmapUpload(f,image);assert.deepEqual(upload.data,[255,0,0,255]);assert.equal(upload.operation.value.source_space,'display-p3');
  assert.throws(()=>bitmapUpload(f,{width:1,height:1,data:new Uint8ClampedArray(4)}),{name:'TypeError'});
  assert.throws(()=>bitmapUpload(f,Object.create(Image.prototype)),{name:'TypeError'});
});
test('ImageData from another realm retains private branding and real typed-array offsets',async()=>{
  const store=new WeakMap(),first=fixture({imageDataStore:store}),second=fixture({imageDataStore:store});
  const data=vm.runInNewContext('new Uint8ClampedArray(new ArrayBuffer(12),4,4)');data.set([13,27,41,255]);
  const image=new first.sandbox.ImageData(data,1,1,{colorSpace:'display-p3'});
  Object.defineProperty(image,'data',{get(){throw Error('foreign shadow');}});
  const upload=bitmapUpload(second,image);assert.deepEqual(upload.data,[13,27,41,255]);assert.equal(upload.operation.value.source_space,'display-p3');
  const copy=await bitmap(second,image);assert.deepEqual(bitmapUpload(second,copy).data,[13,27,41,255]);copy.close();
});

test('bufferData numeric overload follows WebIDL coercion and propagates exceptions',()=>{
  const f=fixture();
  for(const version of [1,2]){
    const gl=version===1?f.gl:f.sandbox._webglCreate(new f.sandbox.HTMLCanvasElement(),'webgl2',{});
    for(const [value,size] of [['4',4],['5.8',5],[[42],42],[[42,64],0],[{},0],[true,1],[NaN,0],[Infinity,0],[-Infinity,0],[2**64,0],[-(2**64),0],[{valueOf(){return 6;}},6]]){
      gl.bufferData(gl.ARRAY_BUFFER,value,gl.STATIC_DRAW);
      assert.equal(f.calls.at(-1).operation.value.command.args.size,size);
      assert.deepEqual(f.calls.at(-1).data,[]);
    }
    const before=f.calls.length;gl.bufferData(gl.ARRAY_BUFFER,2**63,gl.STATIC_DRAW);
    assert.equal(gl.getError(),gl.INVALID_VALUE);
    assert.equal(f.calls.slice(before).filter(call=>call.operation?.kind==='resource').length,0);
    gl.bufferData(gl.ARRAY_BUFFER,2**64-2048,gl.STATIC_DRAW);
    assert.equal(f.calls.at(-1).operation.value.command.args.size,-2048);
    const order=[];gl.bufferData({valueOf(){order.push('target');return gl.ARRAY_BUFFER;}},{valueOf(){order.push('data');return 4;}},{valueOf(){order.push('usage');return gl.STATIC_DRAW;}});
    assert.deepEqual(order,['target','data','usage']);
    for(const value of [Symbol(),4n])assert.throws(()=>gl.bufferData(gl.ARRAY_BUFFER,value,gl.STATIC_DRAW),{name:'TypeError'});
    const failure=new RangeError('fixture coercion');assert.throws(()=>gl.bufferData(gl.ARRAY_BUFFER,{valueOf(){throw failure;}},gl.STATIC_DRAW),error=>error===failure);
    for(const value of [null,undefined]){gl.bufferData(gl.ARRAY_BUFFER,value,gl.STATIC_DRAW);assert.equal(gl.getError(),gl.INVALID_VALUE);}
  }
});
test('bufferData accepts intrinsic cross-realm and shared buffers without trusting tags',()=>{
  const f=fixture();
  const foreign=vm.runInNewContext('new Uint8Array([9,8,7]).buffer');
  const shared=new SharedArrayBuffer(3);new Uint8Array(shared).set([6,5,4]);
  for(const value of [foreign,shared,new Uint8Array(shared),new DataView(shared)]){
    f.gl.bufferData(f.gl.ARRAY_BUFFER,value,f.gl.STATIC_DRAW);
    assert.deepEqual(f.calls.at(-1).data,Array.from(new Uint8Array(value.buffer||value,value.byteOffset||0,value.byteLength)));
    f.gl.bufferSubData(f.gl.ARRAY_BUFFER,0,value);assert.equal(f.calls.at(-1).data.length,3);
  }
  f.gl.bufferData(f.gl.ARRAY_BUFFER,{[Symbol.toStringTag]:'ArrayBuffer',valueOf(){return 4;}},f.gl.STATIC_DRAW);
  assert.equal(f.calls.at(-1).operation.value.command.args.size,4);assert.deepEqual(f.calls.at(-1).data,[]);
});
test('bufferData selects WebGL2 range overload by argument count and WebGL1 ignores extras',()=>{
  const f=fixture(),g=f.sandbox._webglCreate(new f.sandbox.HTMLCanvasElement(),'webgl2',{});
  const bytes=new Uint8Array([1,2,3,4]);
  for(const value of [4,'4',bytes.buffer,bytes]){
    f.gl.bufferData(f.gl.ARRAY_BUFFER,value,f.gl.STATIC_DRAW,1,1);
    assert.equal(f.calls.at(-1).operation.value.command.args.size,4);
  }
  for(const value of [4,'4',bytes.buffer,null,undefined]){
    assert.throws(()=>g.bufferData(g.ARRAY_BUFFER,value,g.STATIC_DRAW,undefined),{name:'TypeError'});
  }
  g.bufferData(g.ARRAY_BUFFER,bytes,g.STATIC_DRAW,1,2);assert.deepEqual(f.calls.at(-1).data,[2,3]);
  g.bufferData(g.ARRAY_BUFFER,bytes,g.STATIC_DRAW,undefined);assert.deepEqual(f.calls.at(-1).data,[1,2,3,4]);
  g.bufferData(g.ARRAY_BUFFER,bytes,g.STATIC_DRAW,99);assert.equal(g.getError(),g.INVALID_VALUE);
  const order=[];
  g.bufferData({valueOf(){order.push('target');return g.ARRAY_BUFFER;}},bytes,{valueOf(){order.push('usage');return g.STATIC_DRAW;}},{valueOf(){order.push('offset');return 1;}},{valueOf(){order.push('length');return 2;}});
  assert.deepEqual(order,['target','usage','offset','length']);assert.deepEqual(f.calls.at(-1).data,[2,3]);
  assert.throws(()=>g.bufferData(g.ARRAY_BUFFER,bytes,g.STATIC_DRAW,1n),{name:'TypeError'});
  g.bufferData(g.ARRAY_BUFFER,bytes,g.STATIC_DRAW,2**64);assert.deepEqual(f.calls.at(-1).data,[1,2,3,4]);
  g.bufferData(g.ARRAY_BUFFER,bytes,g.STATIC_DRAW,-1);assert.equal(g.getError(),g.INVALID_VALUE);
});

test('shared buffer brands remain intrinsic when constructor exposure follows bootstrap',()=>{
  const f=fixture({lateSharedBuffer:true});
  const shared=new SharedArrayBuffer(5);new Uint8Array(shared).set([9,8,7,6,5]);
  for(const value of [shared,new Uint8Array(shared,1,3),new DataView(shared,1,3)]){
    f.gl.bufferData(f.gl.ARRAY_BUFFER,value,f.gl.STATIC_DRAW);
    const call=f.calls.at(-1);assert.equal(call.operation.value.command.args.size,value.byteLength);
    assert.equal(call.sharedBacking,false);assert.deepEqual(call.data,Array.from(new Uint8Array(value.buffer||value,value.byteOffset||0,value.byteLength)));
  }
  let coercions=0;f.gl.bufferData(f.gl.ARRAY_BUFFER,{[Symbol.toStringTag]:'SharedArrayBuffer',valueOf(){coercions++;return 3;}},f.gl.STATIC_DRAW);
  assert.equal(coercions,1);assert.equal(f.calls.at(-1).operation.value.command.args.size,3);
});
test('shared readback copies only successful written ranges and bounds staging allocation',()=>{
  const shared=new SharedArrayBuffer(40),destination=new Uint8Array(shared);destination.fill(9);
  let succeed=false;
  const f=fixture({dataReply(operation,data){
    if(!['readPixels','bufferRead'].includes(operation.kind))return;
    assert.notEqual(Object.prototype.toString.call(data.buffer),'[object SharedArrayBuffer]');
    destination.fill(7); // Model another agent writing while native work runs.
    data.fill(1);
    if(operation.kind==='bufferRead')return{type:'boolean',value:succeed};
    return{type:'pixelRead',value:succeed?{start:4,stride:12,rows:2,row_bytes:8}:null};
  }});
  f.gl.readPixels(0,0,2,2,f.gl.RGBA,f.gl.UNSIGNED_BYTE,destination);
  assert.deepEqual(Array.from(destination),Array(40).fill(7));
  succeed=true;f.gl.readPixels(0,0,2,2,f.gl.RGBA,f.gl.UNSIGNED_BYTE,destination);
  for(let i=0;i<40;i++)assert.equal(destination[i],i>=4&&i<12||i>=16&&i<24?1:7);
  const gl=f.sandbox._webglCreate(new f.sandbox.HTMLCanvasElement(),'webgl2',{});
  succeed=false;gl.getBufferSubData(gl.ARRAY_BUFFER,0,destination,4,8);assert.deepEqual(Array.from(destination),Array(40).fill(7));
  succeed=true;gl.getBufferSubData(gl.ARRAY_BUFFER,0,destination,4,8);
  for(let i=0;i<40;i++)assert.equal(destination[i],i>=4&&i<12?1:7);
  const allocation=fixture({failSharedCopy:true});allocation.gl.bufferData(allocation.gl.ARRAY_BUFFER,destination,allocation.gl.STATIC_DRAW);
  assert.equal(allocation.gl.getError(),allocation.gl.OUT_OF_MEMORY);assert.equal(allocation.calls.some(row=>row.operation?.kind==='resource'),false);
  // Shared buffers reserve virtual address space; the limit must reject before
  // any ordinary copy or native op attempts to touch this oversized range.
  const huge=new Uint8Array(new SharedArrayBuffer(256*1024*1024+1));
  const bounded=fixture();bounded.gl.bufferData(bounded.gl.ARRAY_BUFFER,huge,bounded.gl.STATIC_DRAW);
  assert.equal(bounded.gl.getError(),bounded.gl.OUT_OF_MEMORY);assert.equal(bounded.calls.some(row=>row.operation?.kind==='resource'),false);
});
test('runtime stealth reports the seeded GPU profile and Chrome version strings in both versions',()=>{
  const driver={7936:'WebKit',7937:'WebKit WebGL',7938:'WebGL 1.0 (OpenGL ES 2.0)',35724:'WebGL GLSL ES 1.00',
    0x9245:'Google Inc. (Google)',0x9246:'ANGLE (Google, Vulkan 1.3.0 (SwiftShader Device (Subzero) (0x0000C0DE)), SwiftShader driver)'};
  const profile={gpuVendor:'Google Inc. (NVIDIA)',gpu:'ANGLE (NVIDIA, NVIDIA GeForce RTX 3060 Direct3D11 vs_5_0 ps_5_0, D3D11)'};
  function setup(stealth){
    const debug={enabled:false};
    // Native replies model the driver: unmasked strings exist only after the
    // page enables WEBGL_debug_renderer_info; numeric limits are not strings.
    const query=({method,args})=>{
      if(method!=='getParameter')return {type:'null'};
      if(args.name===0x0D33)return {type:'int',value:8192};
      if((args.name===0x9245||args.name===0x9246)&&!debug.enabled)return {type:'null'};
      return args.name in driver?{type:'string',value:driver[args.name]}:{type:'null'};
    };
    const f=fixture({query,stealth});
    return {...f,debug,gl2:f.sandbox._webglCreate(new f.sandbox.HTMLCanvasElement(),'webgl2',{})};
  }
  const read=gl=>[gl.VENDOR,gl.RENDERER,gl.VERSION,gl.SHADING_LANGUAGE_VERSION,0x9245,0x9246,0x0D33].map(name=>gl.getParameter(name));
  const plain=setup();
  assert.equal(plain.gl.getParameter(0x9246),null);
  assert.equal(plain.gl.getExtension('WEBGL_debug_renderer_info').UNMASKED_RENDERER_WEBGL,0x9246);plain.debug.enabled=true;
  for(const gl of [plain.gl,plain.gl2])assert.deepEqual(read(gl),[driver[7936],driver[7937],driver[7938],driver[35724],driver[0x9245],driver[0x9246],8192]);
  const masked=setup(profile);
  assert.equal(masked.gl.getParameter(0x9246),null);assert.equal(masked.gl2.getParameter(0x9245),null);
  const ext=masked.gl.getExtension('WEBGL_debug_renderer_info');
  assert.deepEqual([ext.UNMASKED_VENDOR_WEBGL,ext.UNMASKED_RENDERER_WEBGL],[0x9245,0x9246]);masked.debug.enabled=true;
  const one=['WebKit','WebKit WebGL','WebGL 1.0 (OpenGL ES 2.0 Chromium)','WebGL GLSL ES 1.0 (OpenGL ES GLSL ES 1.0 Chromium)',profile.gpuVendor,profile.gpu,8192];
  const two=['WebKit','WebKit WebGL','WebGL 2.0 (OpenGL ES 3.0 Chromium)','WebGL GLSL ES 3.00 (OpenGL ES GLSL ES 3.0 Chromium)',profile.gpuVendor,profile.gpu,8192];
  assert.deepEqual(read(masked.gl),one);assert.deepEqual(read(masked.gl2),two);
  assert.deepEqual(read(masked.gl),one);assert.deepEqual(read(masked.gl2),two);
  masked.gl.getExtension('WEBGL_lose_context').loseContext();assert.equal(masked.gl.getParameter(0x9246),null);assert.equal(masked.gl.getParameter(masked.gl.VERSION),null);
  assert.deepEqual(read(masked.gl2),two);
});
