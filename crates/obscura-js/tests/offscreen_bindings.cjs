// Actual private CPU canvas and PNG implementation with a controlled native GL
// boundary. Real V8/ANGLE ownership, GC and pixel tests remain separate gates.
const {test}=require('node:test');
const assert=require('node:assert/strict');
const fs=require('node:fs');
const path=require('node:path');
const vm=require('node:vm');
const zlib=require('node:zlib');
const read=name=>fs.readFileSync(path.join(__dirname,'../js',name),'utf8');
const bootstrap=read('bootstrap.js');
const png=bootstrap.slice(bootstrap.indexOf('function _encodePNG('),bootstrap.indexOf('globalThis.__ariaQuerySelector ='));
const canvas=bootstrap.slice(bootstrap.indexOf('const _MAX_CANVAS_DIMENSION'),bootstrap.indexOf('/* @obscura-webgl */'));
// The canvas fixture already contains the HTMLCanvasElement class. Append
// only the methods installed after the optional WebGL injection marker.
const htmlCanvasStart=bootstrap.indexOf('HTMLCanvasElement.prototype.getContext =',bootstrap.indexOf('/* @obscura-webgl */'));
const htmlCanvasEnd=bootstrap.indexOf('Element.prototype.attachShadow =',htmlCanvasStart);
assert.ok(htmlCanvasStart>=0&&htmlCanvasEnd>htmlCanvasStart,'HTML canvas fixture boundaries must include the implementation');
const htmlCanvas=bootstrap.slice(htmlCanvasStart,htmlCanvasEnd);
const bindings=read('webgl.js').replace('/* @obscura-imagedata */',read('imagedata.js')).replace('/* @obscura-imagebitmap */',read('imagebitmap.js')).replace('/* @obscura-offscreen */',read('offscreen.js'));
function fixture(options={}) {
  const imageDataStore=new WeakMap();
  const calls=[],tasks=[],frames=[],native=new Map(),presented=new Map(),listeners=new WeakMap(),cache=new Map();let next=1,nextNode=1,nextPlaceholder=1,generation=1;
  class EventTarget {}
  class Event {constructor(type,init={}){this.type=type;this.cancelable=!!init.cancelable;this.defaultPrevented=false;}preventDefault(){if(this.cancelable)this.defaultPrevented=true;}}
  class HTMLCanvasElement {}
  class Element {
    constructor(){this._nid=nextNode++;this.attributes=new Map();cache.set(this._nid,this);}
    getAttribute(name){return this.attributes.get(String(name).toLowerCase())??null;}
    setAttribute(name,value){this.attributes.set(String(name).toLowerCase(),String(value));}
    removeAttribute(name){this.attributes.delete(String(name).toLowerCase());}
    dispatchEvent(event){return sandbox._eventTargetDispatch(this,event);}
  }
  class HTMLImageElement {constructor(){this._nid=41;}}
  class HTMLVideoElement {}
  const sandbox={console,DOMException,Blob,EventTarget,Event,Element,HTMLCanvasElement,HTMLImageElement,HTMLVideoElement,
    _cache:cache,atob:globalThis.atob,
    Uint8Array,Uint8ClampedArray,Uint32Array,ArrayBuffer,WeakMap,WeakRef,FinalizationRegistry,queueMicrotask,
    _realmFrameId:7,_hostState:{documentGeneration:generation},_markNative:()=>{},
    _domParse(command,node,name){assert.equal(command,'get_attribute');return cache.get(node).getAttribute(name);},
    _canvasPresentationPending:false,_runCanvasPresentation:null,
    _scheduleRenderingOpportunity(){if(!frames.length)frames.push(()=>sandbox._runCanvasPresentation());},
    _eventTargetAdd(target,type,callback){let list=listeners.get(target);if(!list)listeners.set(target,list=[]);list.push({type,callback});},
    _eventTargetRemove(target,type,callback){const list=listeners.get(target)||[];const i=list.findIndex(x=>x.type===type&&x.callback===callback);if(i>=0)list.splice(i,1);},
    _eventTargetDispatch(target,event){event.target=target;for(const item of [...(listeners.get(target)||[])])if(item.type===event.type)item.callback.call(target,event);return !event.defaultPrevented;},
    __obscuraCore:{ops:{
      op_webgl_image_data(object,data){if(data)imageDataStore.set(object,data);return imageDataStore.get(object)||null;},
      op_posted_task(frame,callback){calls.push({task:true,frame});tasks.push(()=>callback(generation));return generation;},
      op_canvas_register_surface(frame,epoch,node,width,height,bytes){calls.push({domSurface:node,frame,epoch,width,height});return epoch===generation;},
      op_canvas_paint_damage(frame,epoch,node){calls.push({domDamage:node,frame,epoch});return epoch===generation;},
      op_canvas_placeholder(frame,epoch,request,bytes){
        calls.push({placeholder:request.kind,frame,request});
        if(epoch!==generation)return{status:'failed',reason:'stale document'};
        if(request.kind==='register'){
          if(options.registerFails)return{status:'failed',reason:'registry allocation'};
          const id=nextPlaceholder++;presented.set(id,{width:request.width,height:request.height,originClean:true,revision:0,bytes:null,colorSpace:'srgb'});return{status:'ready',id};
        }
        const p=presented.get(request.id);if(!p)return{status:'failed',reason:'missing placeholder'};
        if(request.kind==='bind'){
          if(options.bindFails)return{status:'failed',reason:'context ownership'};
          p.context=request.context;return{status:'ready'};
        }
        if(request.kind==='retire'){presented.delete(request.id);return{status:'ready'};}
        if(request.kind==='presentBlank')Object.assign(p,{width:request.width,height:request.height,revision:p.revision+1});
        if(request.kind==='presentCpu'||request.kind==='presentGl'){
          if(options.presentFails)return{status:'failed',reason:'snapshot allocation'};
          let data=bytes,width=request.width,height=request.height,clean=request.origin_clean;
          if(request.kind==='presentGl'){
            const gl=native.get(request.context);if(gl.lost)return{status:'failed',reason:'context lost'};
            if(!gl.dirty)return{status:'ready',...p};
            data=gl.pixels;width=gl.width;height=gl.height;clean=true;gl.dirty=false;
            p.colorSpace=gl.colorSpace||'srgb';p.sourceBytes=p.colorSpace==='srgb'?null:data.slice();
            p.bytes=(p.sourceBytes&&options.presentedPixels?options.presentedPixels:data).slice();if(!gl.attributes.preserveDrawingBuffer)gl.pixels.fill(0);
          }else {p.bytes=data.slice();p.colorSpace='srgb';p.sourceBytes=null;}
          Object.assign(p,{width,height,originClean:clean,revision:p.revision+1});
        }
        if(request.kind==='pixels'||request.kind==='sourcePixels'){
          if(options.snapshotChanges||request.revision!==p.revision||bytes.length!==p.width*p.height*4)return{status:'failed',reason:'snapshot changed'};
          const source=request.kind==='sourcePixels'&&p.sourceBytes?p.sourceBytes:p.bytes;
          if(source)bytes.set(source);else bytes.fill(0);
        }
        return{status:'ready',width:p.width,height:p.height,originClean:p.originClean,revision:p.revision,colorSpace:request.kind==='pixels'?'srgb':p.colorSpace};
      },
      op_webgl_create(frame,epoch,node,version,width,height,attributes){
        calls.push({create:true,frame,epoch,node,version,width,height,attributes});
        if(epoch!==generation)return{status:'failed',reason:'stale document'};
        if(options.fail)return{status:'failed',reason:'No usable ANGLE backend'};
        const id=next++;native.set(id,{pixels:new Uint8Array(width*height*4).fill(42),attributes,version,lost:false,width,height,canvasSize:{width,height},format:attributes.alpha===false?0x8051:0x8058,dirty:true});
        return{status:'ready',id,width,height,attributes};
      },
      op_webgl_convert_color(source,target,premultiplied,bytes){calls.push({convertColor:true,source,target,premultiplied});if(options.colorFails)return false;if(options.convertedPixels)bytes.set(options.convertedPixels);return true;},
      op_webgl_watch_loss(frame,id,callback){native.get(id).callback=callback;return !options.watchFails;},
      op_webgl_release(frame,id){calls.push({release:id});return native.delete(id);},
      op_webgl_call(frame,id,operation,bytes){
        calls.push({id,operation,data:operation.kind==='textureSource'?Array.from(bytes):undefined});const s=native.get(id),arg=operation.value;let value={type:'none'};
        switch(operation.kind){
          case 'colorSpace':if(arg.drawing){s.colorSpace=arg.color_space;s.pixels.fill(0);s.dirty=true;}break;
          case 'readback':case 'sourceReadback':case 'transferBitmap':
            if(!s.lost&&!options.transferFails)bytes.set(s.pixels);
            value={type:'boolean',value:!s.lost&&!options.transferFails};
            if(operation.kind==='transferBitmap'&&value.value){s.pixels.fill(0);s.dirty=true;}
            break;
          case 'resize':s.canvasSize={width:arg.width,height:arg.height};
          case 'drawingBufferStorage':
            if(s.lost)break;
            s.width=arg.width;s.height=arg.height;if(arg.format!==undefined)s.format=arg.format;
            s.pixels=new Uint8Array(arg.width*arg.height*4);s.dirty=true;
            value={type:'drawingBuffer',value:{width:s.width,height:s.height,format:s.format}};break;
          case 'drawingBufferInfo':value={type:'drawingBuffer',value:{width:s.width,height:s.height,format:s.format}};break;
          case 'command':if(['clear','drawArrays','drawElements'].includes(arg.method))s.dirty=true;break;
          case 'lose':s.lost=true;break;
          case 'restore':s.lost=false;s.dirty=true;s.width=s.canvasSize.width;s.height=s.canvasSize.height;s.format=s.attributes.alpha===false?0x8051:0x8058;s.pixels=new Uint8Array(s.width*s.height*4);value={type:'boolean',value:true};break;
          case 'isLost':value={type:'boolean',value:s.lost};break;
          case 'extension':value={type:'text',value:arg.name};break;
          case 'attributes':value={type:'attributes',value:s.attributes};break;
          case 'getError':value={type:'number',value:0};break;
        }
        return{lost:s.lost,dirty:s.dirty,value};
      },
      op_webgl_image_info(){return{status:'ready',width:1,height:1,originClean:false};},
      op_webgl_image_pixels(frame,node,bytes){bytes.set([255,0,0,255]);return 3;},
    }}
  };
  const encoder=options.encodingFails?'function _encodePNG(){throw new RangeError("encoder allocation");}':png;
  vm.createContext(sandbox);vm.runInContext(encoder+'\n'+canvas+'\n'+bindings+'\n'+htmlCanvas+'\n_canvasDocumentEpoch=1;',sandbox);
  return{sandbox,calls,native,presented,tasks,html:(width=2,height=1)=>{const value=new sandbox.HTMLCanvasElement();value.width=width;value.height=height;return value;},create:(...args)=>new sandbox.OffscreenCanvas(...args),
    advance(){sandbox._hostState.documentGeneration=++generation;vm.runInContext('_canvasDocumentEpoch='+generation,sandbox);presented.clear();},
    drainTasks(){for(let i=0;tasks.length&&i<100;i++)tasks.shift()();assert.equal(tasks.length,0);},
    drain(){for(let i=0;(tasks.length||frames.length)&&i<100;i++){
      while(tasks.length)tasks.shift()();if(frames.length)frames.shift()();
    }assert.equal(tasks.length+frames.length,0);}};
}
async function settle(f,promise) {
  promise.catch(()=>{});
  for(let i=0;i<4;i++){await new Promise(resolve=>setImmediate(resolve));f.drain();}
  return promise;
}
function pixels(canvas){return Array.from(canvas.getContext('2d').getImageData(0,0,canvas.width,canvas.height).data);}
test('standalone construction is lazy, private and validates unsigned dimensions',()=>{
  const f=fixture();const canvas=f.create(3.9,'2');
  assert.equal(canvas.width,3);assert.equal(canvas.height,2);assert.deepEqual(f.calls,[]);
  assert.deepEqual(Object.keys(canvas),[]);assert.deepEqual(Object.getOwnPropertySymbols(canvas),[]);
  assert.ok(canvas instanceof f.sandbox.EventTarget);
  assert.equal(Object.prototype.toString.call(canvas),'[object OffscreenCanvas]');
  for(const value of [-1,NaN,Infinity,2**64,1n,Symbol()])assert.throws(()=>f.create(value,2),{name:'TypeError'});
  assert.throws(()=>f.create(1),{name:'TypeError'});
  const enormous=f.create(2**40,2);assert.equal(enormous.getContext('2d'),null);assert.deepEqual(f.calls,[]);
});
test('standalone 2D context keeps identity, pixels and context mode without DOM registration',()=>{
  const f=fixture();const canvas=f.create(2,1),ctx=canvas.getContext('2d');
  assert.equal(ctx.canvas,canvas);assert.ok(ctx instanceof f.sandbox.OffscreenCanvasRenderingContext2D);
  assert.equal(canvas.getContext('2d',{alpha:false}),ctx);assert.equal(canvas.getContext('webgl'),null);
  canvas._nid=123;canvas._ctx={};ctx._buf=new Uint8Array(8).fill(33);
  ctx.fillStyle='#ff0000';ctx.fillRect(0,0,1,1);
  assert.deepEqual(pixels(canvas),[255,0,0,255,0,0,0,0]);assert.deepEqual(f.calls,[]);
  assert.equal(Reflect.set(ctx,'canvas',{}),false);
});
test('2D bitmap transfer moves pixels and taint without resetting drawing state',()=>{
  const f=fixture();const canvas=f.create(1,1),ctx=canvas.getContext('2d');
  ctx.fillStyle='#00ff00';ctx.fillRect(0,0,1,1);ctx.save();ctx.fillStyle='#ff0000';
  const bitmap=canvas.transferToImageBitmap();assert.equal(bitmap.width,1);assert.equal(ctx.fillStyle,'#ff0000');
  assert.deepEqual(pixels(canvas),[0,0,0,0]);ctx.restore();assert.equal(ctx.fillStyle,'#00ff00');
  const destination=f.create(1,1);destination.getContext('2d').drawImage(bitmap,0,0);
  assert.deepEqual(pixels(destination),[0,255,0,255]);bitmap.close();
  ctx.fillRect(0,0,1,1);assert.deepEqual(pixels(canvas),[0,255,0,255]);
});
test('same-dimension assignment resets pixels and drawing state while keeping context identity',()=>{
  const f=fixture();const canvas=f.create(2,1),ctx=canvas.getContext('2d');
  ctx.fillStyle='#ff0000';ctx.fillRect(0,0,1,1);canvas.width=2;
  assert.equal(canvas.getContext('2d'),ctx);assert.equal(ctx.fillStyle,'#000000');assert.deepEqual(pixels(canvas),new Array(8).fill(0));
  ctx.beginPath();ctx.arc(0,0,1,0,6.3);canvas.width=2;ctx.fill();assert.deepEqual(pixels(canvas),new Array(8).fill(0));
  assert.throws(()=>{canvas.height=-1;},{name:'TypeError'});assert.equal(canvas.height,1);
  canvas.width=0;assert.equal(canvas.transferToImageBitmap().width,0);
});
test('opaque 2D stores clear, transfer and reset to opaque black',()=>{
  const f=fixture();const canvas=f.create(1,1),ctx=canvas.getContext('2d',{alpha:false});
  assert.equal(ctx.getContextAttributes().alpha,false);assert.deepEqual(pixels(canvas),[0,0,0,255]);
  ctx.fillStyle='#ff0000';ctx.fillRect(0,0,1,1);ctx.clearRect(0,0,1,1);assert.deepEqual(pixels(canvas),[0,0,0,255]);
  ctx.fillRect(0,0,1,1);const bitmap=canvas.transferToImageBitmap();assert.deepEqual(pixels(canvas),[0,0,0,255]);
  ctx.drawImage(bitmap,0,0);assert.deepEqual(pixels(canvas),[255,0,0,255]);canvas.height=1;
  assert.deepEqual(pixels(canvas),[0,0,0,255]);
  ctx.putImageData(new f.sandbox.ImageData(new Uint8ClampedArray([17,33,65,0]),1),0,0);
  assert.deepEqual(pixels(canvas),[17,33,65,255]);
});
test('convertToBlob snapshots pixels, settles on a task and reports actual PNG fallback type',async()=>{
  const f=fixture();const canvas=f.create(1,1),ctx=canvas.getContext('2d');ctx.fillStyle='#ff0000';ctx.fillRect(0,0,1,1);
  let complete=false;const promise=canvas.convertToBlob({type:'image/jpeg'}).then(value=>{complete=true;return value;});
  ctx.clearRect(0,0,1,1);await Promise.resolve();assert.equal(complete,false);
  const blob=await settle(f,promise);assert.equal(blob.type,'image/png');
  const bytes=Buffer.from(await blob.arrayBuffer());assert.deepEqual([...bytes.subarray(0,8)],[137,80,78,71,13,10,26,10]);
  assert.equal(bytes.readUInt32BE(16),1);assert.equal(bytes.readUInt32BE(20),1);
  const idatLength=bytes.readUInt32BE(33);const raw=zlib.inflateSync(bytes.subarray(41,41+idatLength));
  assert.deepEqual([...raw],[0,255,0,0,255]);assert.deepEqual(pixels(canvas),[0,0,0,0]);
});
test('tainted bitmap transfer preserves restrictions and replacement storage becomes clean',async()=>{
  const f=fixture();const bitmap=await settle(f,f.sandbox.createImageBitmap(new f.sandbox.HTMLImageElement()));
  const canvas=f.create(1,1);canvas.getContext('2d').drawImage(bitmap,0,0);
  await assert.rejects(canvas.convertToBlob(),{name:'SecurityError'});
  const transferred=canvas.transferToImageBitmap();assert.deepEqual(pixels(canvas),[0,0,0,0]);
  const destination=f.create(1,1);destination.getContext('2d').drawImage(transferred,0,0);
  assert.throws(()=>pixels(destination),{name:'SecurityError'});
  destination.width=1;assert.deepEqual(pixels(destination),[0,0,0,0]);
});
test('standalone canvases can be copied, cropped and used as bitmap or GL image sources',async()=>{
  const f=fixture();const source=f.create(2,1),ctx=source.getContext('2d');ctx.fillStyle='#0000ff';ctx.fillRect(0,0,1,1);
  const destination=f.create(2,1);destination.getContext('2d').drawImage(source,0,0);assert.deepEqual(pixels(destination),pixels(source));
  const bitmap=await settle(f,f.sandbox.createImageBitmap(source,0,0,1,1));
  destination.getContext('2d').drawImage(bitmap,1,0);assert.deepEqual(pixels(destination),[0,0,255,255,0,0,255,255]);
  const gl=f.create(1,1).getContext('webgl');gl.texImage2D(gl.TEXTURE_2D,0,gl.RGBA,gl.RGBA,gl.UNSIGNED_BYTE,source);
  assert.equal(f.calls.at(-1).operation.kind,'textureSource');
});
test('self-copy reads a stable snapshot instead of cascading writes',()=>{
  const f=fixture();const canvas=f.create(3,1),ctx=canvas.getContext('2d');ctx.fillStyle='#ff0000';ctx.fillRect(0,0,1,1);
  ctx.drawImage(canvas,0,0,2,1,1,0,2,1);
  assert.deepEqual(pixels(canvas),[255,0,0,255,255,0,0,255,0,0,0,0]);
});
test('WebGL uses standalone native ownership and ignores forged DOM or dimension expandos',()=>{
  const f=fixture();const canvas=f.create(2,3);canvas._nid=9;canvas._ctx={};
  Object.defineProperty(canvas,'width',{value:99});const gl=canvas.getContext('webgl2');
  const creation=f.calls.find(call=>call.create);assert.equal(creation.node,null);assert.equal(creation.width,2);assert.equal(creation.height,3);
  assert.equal(gl.drawingBufferWidth,2);assert.equal(gl.canvas,canvas);assert.equal(canvas.getContext('webgl2'),gl);
  assert.equal(canvas.getContext('webgl'),null);assert.equal(canvas.getContext('2d'),null);
  canvas.height=4;assert.equal(f.calls.at(-1).operation.kind,'resize');
  assert.deepEqual(JSON.parse(JSON.stringify(f.calls.at(-1).operation.value)),{width:2,height:4});
});
test('WebGL bitmap transfer uses the consuming native operation even when preservation is requested',()=>{
  const f=fixture();const canvas=f.create(1,1);canvas.getContext('webgl',{preserveDrawingBuffer:true});
  const bitmap=canvas.transferToImageBitmap();const destination=f.create(1,1);destination.getContext('2d').drawImage(bitmap,0,0);
  assert.deepEqual(pixels(destination),[42,42,42,42]);
  assert.equal(f.calls.filter(c=>c.operation?.kind==='transferBitmap').length,1);
  assert.deepEqual([...f.native.get(1).pixels],[0,0,0,0]);
});
test('failed GL creation or lifecycle setup leaves context choice open and emits queued diagnostics',()=>{
  for(const options of [{fail:true},{watchFails:true}]){
    const f=fixture(options),canvas=f.create(1,1),events=[];
    canvas.addEventListener('webglcontextcreationerror',event=>events.push(event.statusMessage));
    assert.equal(canvas.getContext('webgl'),null);assert.deepEqual(events,[]);f.drain();assert.equal(events.length,1);
    assert.ok(canvas.getContext('2d'));assert.equal(f.native.size,0);
  }
});
test('loss and restoration events use the OffscreenCanvas target without polling',()=>{
  const f=fixture();const canvas=f.create(1,1),gl=canvas.getContext('webgl'),events=[];
  canvas.addEventListener('webglcontextlost',event=>{assert.equal(event.target,canvas);event.preventDefault();events.push(event.type);});
  canvas.addEventListener('webglcontextrestored',event=>events.push(event.type));
  const ext=gl.getExtension('WEBGL_lose_context');ext.loseContext();assert.deepEqual(events,[]);f.drain();ext.restoreContext();f.drain();
  assert.deepEqual(events,['webglcontextlost','webglcontextrestored']);assert.equal(gl.isContextLost(),false);
});
test('pending conversion is discarded after document replacement',async()=>{
  const f=fixture();let settled=false;const canvas=f.create(1,1);
  canvas.convertToBlob().then(()=>{settled=true;});f.advance();f.drain();await Promise.resolve();assert.equal(settled,false);
});
test('conversion, transfer, enums, options and borrowed methods reject invalid states',async()=>{
  const f=fixture();const canvas=f.create(1,1);
  assert.throws(()=>canvas.transferToImageBitmap(),{name:'InvalidStateError'});
  for(const type of [undefined,'experimental-webgl','invalid',Symbol()])assert.throws(()=>canvas.getContext(type),{name:'TypeError'});
  assert.equal(canvas.getContext('bitmaprenderer'),null);assert.equal(canvas.getContext('webgpu'),null);
  assert.throws(()=>canvas.getContext('2d',17),{name:'TypeError'});
  assert.throws(()=>canvas.getContext('2d',{colorSpace:'bad'}),{name:'TypeError'});
  assert.throws(()=>f.sandbox.OffscreenCanvas.prototype.getContext.call({},'2d'),{name:'TypeError'});
  assert.throws(()=>new f.sandbox.OffscreenCanvasRenderingContext2D(),{name:'TypeError'});
  await assert.rejects(f.create(0,2).convertToBlob(),{name:'IndexSizeError'});
  await assert.rejects(canvas.convertToBlob({type:Symbol()}),{name:'TypeError'});
  await assert.rejects(canvas.convertToBlob(12),{name:'TypeError'});
  const failed=fixture({transferFails:true}),glCanvas=failed.create(1,1);glCanvas.getContext('webgl');
  assert.throws(()=>glCanvas.transferToImageBitmap(),{name:'InvalidStateError'});
  assert.deepEqual([...failed.native.get(1).pixels],[42,42,42,42]);
});
test('2D options are converted once and event handler reassignment preserves listener order',()=>{
  const f=fixture();const canvas=f.create(1,1);let reads=0;
  assert.equal(canvas.getContext('2d',{colorSpace:'display-p3'}),null);
  assert.equal(canvas.getContext('2d',{colorType:'float16'}),null);
  assert.throws(()=>canvas.getContext('2d',{colorType:'bad'}),{name:'TypeError'});
  const ctx=canvas.getContext('2d',{get alpha(){reads++;return true;}});
  assert.equal(reads,1);assert.equal(ctx.getContextAttributes().colorSpace,'srgb');
  const seen=[];canvas.oncontextlost=()=>seen.push('old');canvas.addEventListener('contextlost',()=>seen.push('listener'));
  canvas.oncontextlost=()=>seen.push('new');canvas.dispatchEvent(new f.sandbox.Event('contextlost'));
  assert.deepEqual(seen,['new','listener']);canvas.oncontextlost=null;assert.equal(canvas.oncontextlost,null);
});
test('CPU allocation failure does not lock context selection or consume a previous bitmap',()=>{
  const f=fixture();const canvas=f.create(1,1);const original=f.sandbox.Uint8ClampedArray;
  f.sandbox.Uint8ClampedArray=class {constructor(){throw new RangeError('allocation failure');}};
  assert.equal(canvas.getContext('2d'),null);
  f.sandbox.Uint8ClampedArray=original;const ctx=canvas.getContext('2d');ctx.fillStyle='#ff0000';ctx.fillRect(0,0,1,1);
  f.sandbox.Uint8ClampedArray=class {constructor(){throw new RangeError('allocation failure');}};
  assert.throws(()=>canvas.transferToImageBitmap(),{name:'RangeError'});
  assert.throws(()=>{canvas.width=2;},{name:'RangeError'});assert.equal(canvas.width,1);
  f.sandbox.Uint8ClampedArray=original;assert.deepEqual(pixels(canvas),[255,0,0,255]);
});
test('encoder failure rejects its asynchronous result with EncodingError',async()=>{
  const f=fixture({encodingFails:true});
  await assert.rejects(settle(f,f.create(1,1).convertToBlob()),{name:'EncodingError'});
});
test('oversized GL resize loses the context and cannot restore until dimensions are usable',()=>{
  const f=fixture();const canvas=f.create(1,1),gl=canvas.getContext('webgl');
  canvas.addEventListener('webglcontextlost',event=>event.preventDefault());
  const extension=gl.getExtension('WEBGL_lose_context');canvas.width=2**40;
  assert.equal(gl.isContextLost(),true);f.drain();extension.restoreContext();f.drain();assert.equal(gl.isContextLost(),true);
  canvas.width=2;extension.restoreContext();f.drain();assert.equal(gl.isContextLost(),false);assert.equal(gl.drawingBufferWidth,2);
});

test('HTML transfer enforces context exclusivity and protects private ownership from page fields',()=>{
  const f=fixture(),html=f.html();html._ctx={pageOwned:true};
  const before=html.toDataURL(),offscreen=html.transferControlToOffscreen();
  assert.ok(before.startsWith('data:image/png;base64,'));assert.equal(offscreen.width,2);
  assert.equal(html._ctx.pageOwned,true);assert.equal(f.native.size,0);assert.equal(f.presented.get(1).bytes,null);
  assert.throws(()=>html.getContext('2d'),{name:'InvalidStateError'});
  assert.throws(()=>html.transferControlToOffscreen(),{name:'InvalidStateError'});
  for(const axis of ['width','height'])assert.throws(()=>{html[axis]=7;},{name:'InvalidStateError'});
  html.setAttribute('width',90);html.removeAttribute('height');
  assert.equal(offscreen.width,2);assert.equal(offscreen.height,1);assert.equal(html.width,90);
  const occupied=f.html();const ctx=occupied.getContext('2d');delete occupied._ctx;
  assert.equal(Reflect.set(ctx,'canvas',html),false);assert.equal(ctx.canvas,occupied);
  assert.equal(occupied.getContext('2d'),ctx);assert.throws(()=>occupied.transferControlToOffscreen(),{name:'InvalidStateError'});
  assert.throws(()=>f.sandbox.HTMLCanvasElement.prototype.transferControlToOffscreen.call({}),{name:'TypeError'});
  const forged=f.html();forged._nid=html._nid;assert.throws(()=>forged.transferControlToOffscreen(),{name:'TypeError'});
});
test('CPU placeholder tasks coalesce and serialization reads only the last presented frame',()=>{
  const f=fixture(),html=f.html(),offscreen=html.transferControlToOffscreen(),ctx=offscreen.getContext('2d');
  const blank=html.toDataURL();ctx.fillStyle='#ff0000';ctx.fillRect(0,0,2,1);ctx.fillRect(0,0,1,1);
  assert.equal(f.tasks.length,1);assert.equal(html.toDataURL(),blank);f.drainTasks();
  assert.equal(html.toDataURL(),blank,'posted task must not publish before the frame');f.drain();
  assert.deepEqual(Array.from(f.presented.get(1).bytes),[255,0,0,255,255,0,0,255]);
  const red=html.toDataURL();ctx.fillStyle='#0000ff';ctx.fillRect(0,0,2,1);
  assert.equal(html.toDataURL(),red);assert.equal(f.tasks.length,1);f.drain();assert.notEqual(html.toDataURL(),red);
  assert.equal(f.tasks.length,0);assert.equal(f.calls.filter(c=>c.placeholder==='presentCpu').length,2);
  offscreen.width=3;assert.equal(f.presented.get(1).width,2);f.drain();assert.equal(f.presented.get(1).width,3);
  assert.equal(html.getAttribute('width'),'3');assert.ok(!f.calls.some(c=>c.domSurface||c.domDamage));
});
test('placeholder canvas copies and bitmap transfers use the last presented pixels',async()=>{
  const f=fixture(),html=f.html(1,1),offscreen=html.transferControlToOffscreen(),ctx=offscreen.getContext('2d');
  ctx.fillStyle='#ff0000';ctx.fillRect(0,0,1,1);f.drain();
  const target=f.create(1,1),destination=target.getContext('2d');destination.drawImage(html,0,0);
  assert.deepEqual(pixels(target),[255,0,0,255]);
  const bitmapPromise=f.sandbox.createImageBitmap(html);
  ctx.fillStyle='#0000ff';ctx.fillRect(0,0,1,1);
  const bitmap=await settle(f,bitmapPromise);destination.drawImage(bitmap,0,0);
  assert.deepEqual(pixels(target),[255,0,0,255]);
  const previous=html.toDataURL();offscreen.transferToImageBitmap();assert.equal(html.toDataURL(),previous);
  f.drain();assert.deepEqual(Array.from(f.presented.get(1).bytes),[0,0,0,0]);
});
test('placeholder origin state follows each snapshot and propagates through image sources',async()=>{
  const f=fixture(),html=f.html(1,1),offscreen=html.transferControlToOffscreen(),ctx=offscreen.getContext('2d');
  const tainted=await settle(f,f.sandbox.createImageBitmap(new f.sandbox.HTMLImageElement()));
  ctx.drawImage(tainted,0,0);f.drain();assert.throws(()=>html.toDataURL(),{name:'SecurityError'});
  assert.throws(()=>html.toBlob(()=>{}),{name:'SecurityError'});
  const target=f.create(1,1),destination=target.getContext('2d');destination.drawImage(html,0,0);
  assert.throws(()=>destination.getImageData(0,0,1,1),{name:'SecurityError'});
  const gl=f.create(1,1).getContext('webgl');assert.throws(()=>gl.texImage2D(gl.TEXTURE_2D,0,gl.RGBA,gl.RGBA,gl.UNSIGNED_BYTE,html),{name:'SecurityError'});
  offscreen.width=1;assert.throws(()=>html.toDataURL(),{name:'SecurityError'});f.drain();assert.ok(html.toDataURL().startsWith('data:image/png'));
});
test('placeholder Blob serialization snapshots at invocation and queues the real PNG MIME type',async()=>{
  const f=fixture(),html=f.html(1,1),offscreen=html.transferControlToOffscreen(),ctx=offscreen.getContext('2d');
  ctx.fillStyle='#ff0000';ctx.fillRect(0,0,1,1);f.drain();let blob;
  html.toBlob(value=>{blob=value;},'image/jpeg',.8);assert.equal(blob,undefined);
  ctx.fillStyle='#0000ff';ctx.fillRect(0,0,1,1);f.drain();assert.equal(blob.type,'image/png');
  const data=new Uint8Array(await blob.arrayBuffer());assert.deepEqual(Array.from(data.slice(0,8)),[137,80,78,71,13,10,26,10]);
  let offset=8,compressed=[];while(offset<data.length){const length=new DataView(data.buffer).getUint32(offset),name=Buffer.from(data.slice(offset+4,offset+8)).toString();if(name==='IDAT')compressed.push(data.slice(offset+8,offset+8+length));offset+=length+12;}
  assert.deepEqual(Array.from(zlib.inflateSync(Buffer.concat(compressed))).slice(1),[255,0,0,255]);
  assert.throws(()=>html.toBlob(null),{name:'TypeError'});
  const prior=html.toDataURL();offscreen.width=0;f.drain();blob=undefined;
  html.toBlob(value=>{blob=value;});assert.equal(blob,undefined);f.drain();
  assert.equal(blob.type,'image/png');assert.equal(html.toDataURL(),prior);
  const empty=f.html(0,1);blob=undefined;empty.toBlob(value=>{blob=value;});f.drain();assert.equal(blob,null);
});
test('placeholder WebGL binding presents dirty frames once and preserves prior captures',()=>{
  for(const preserveDrawingBuffer of [false,true]){
    const f=fixture(),html=f.html(1,1),offscreen=html.transferControlToOffscreen(),gl=offscreen.getContext('webgl',{preserveDrawingBuffer});
    assert.equal(f.calls.find(c=>c.create).node,null);assert.equal(f.presented.get(1).context,1);
    gl.clear(gl.COLOR_BUFFER_BIT);gl.clear(gl.COLOR_BUFFER_BIT);assert.equal(f.tasks.length,1);f.drain();
    assert.deepEqual(Array.from(f.presented.get(1).bytes),[42,42,42,42]);
    assert.deepEqual(Array.from(f.native.get(1).pixels),preserveDrawingBuffer?[42,42,42,42]:[0,0,0,0]);
    gl.getError();html.toDataURL();f.drain();assert.equal(f.calls.filter(c=>c.placeholder==='presentGl').length,1);
    f.native.get(1).pixels.fill(77);gl.clear(gl.COLOR_BUFFER_BIT);f.drain();
    assert.deepEqual(Array.from(f.presented.get(1).bytes),[77,77,77,77]);
  }
});
test('registration or GL binding failure leaves context selection available and releases native resources',()=>{
  const options={registerFails:true},f=fixture(options),html=f.html();
  assert.throws(()=>html.transferControlToOffscreen(),{name:'InvalidStateError'});assert.equal(f.presented.size,0);
  options.registerFails=false;const offscreen=html.transferControlToOffscreen();options.bindFails=true;
  assert.equal(offscreen.getContext('webgl'),null);assert.equal(f.native.size,0);assert.ok(f.calls.some(c=>c.release));
  assert.ok(offscreen.getContext('2d'));f.drain();assert.equal(f.presented.get(1).revision,1);
});
test('presentation allocation failure preserves the last frame without scheduling a retry loop',()=>{
  const options={},f=fixture(options),html=f.html(1,1),offscreen=html.transferControlToOffscreen(),ctx=offscreen.getContext('2d');
  ctx.fillStyle='#ff0000';ctx.fillRect(0,0,1,1);f.drain();const before=html.toDataURL();
  options.presentFails=true;offscreen.width=2;ctx.fillStyle='#0000ff';ctx.fillRect(0,0,2,1);f.drain();
  assert.equal(html.toDataURL(),before);assert.equal(f.presented.get(1).width,1);assert.equal(f.tasks.length,0);
  options.presentFails=false;ctx.fillRect(0,0,2,1);f.drain();assert.equal(f.presented.get(1).width,2);
  assert.notEqual(html.toDataURL(),before);
});
test('navigation drops queued presentation and prevents old placeholder access to a new document',()=>{
  const f=fixture(),html=f.html(),offscreen=html.transferControlToOffscreen();offscreen.getContext('2d').fillRect(0,0,1,1);
  f.advance();f.drain();assert.equal(f.calls.filter(c=>c.placeholder==='presentCpu').length,0);
  assert.throws(()=>{const target=f.create(2,1);target.getContext('2d').drawImage(html,0,0);},{name:'InvalidStateError'});
});

test('unused offscreen resize leaves placeholder dimensions unchanged until a context exists',()=>{
  const f=fixture(),html=f.html(),offscreen=html.transferControlToOffscreen();
  offscreen.width=3;offscreen.height=2;assert.equal(f.tasks.length,0);f.drain();
  assert.equal(f.presented.get(1).width,2);assert.equal(f.presented.get(1).height,1);
  assert.equal(f.presented.get(1).bytes,null);assert.equal(f.native.size,0);
  assert.equal(html.getAttribute('width'),'2');assert.ok(html.toDataURL().startsWith('data:image/png'));
  assert.ok(offscreen.getContext('webgl'));f.drain();assert.equal(f.presented.get(1).context,1);
});
test('placeholder serialization failure returns empty data or asynchronous null without swallowing origin errors',()=>{
  for(const options of [{encodingFails:true},{snapshotChanges:true}]){
    const f=fixture(options),html=f.html(1,1),offscreen=html.transferControlToOffscreen();
    offscreen.getContext('2d').fillRect(0,0,1,1);f.drain();
    assert.equal(html.toDataURL(),'data:,');let result='pending';
    html.toBlob(value=>{result=value;});assert.equal(result,'pending');f.drain();assert.equal(result,null);
  }
});

test('native canvas creation forwards private frame and document ownership',()=>{
  const f=fixture(),canvas=f.html(1,1),ctx=canvas.getContext('2d');
  const registration=f.calls.find(c=>c.domSurface);
  assert.deepEqual([registration.frame,registration.epoch,registration.domSurface],[7,1,canvas._nid]);
  const other=f.html(1,1),gl=other.getContext('webgl');assert.ok(gl);
  const creation=f.calls.find(c=>c.create);assert.deepEqual([creation.frame,creation.epoch,creation.node],[7,1,other._nid]);
  canvas._nid=other._nid;
  assert.throws(()=>{canvas.width=2;},{name:'InvalidStateError'});
  assert.equal(canvas.getContext('2d'),null);assert.equal(ctx.canvas,canvas);
});
test('old canvases cannot create new document graphics or replace their dimensions',()=>{
  const f=fixture(),canvas=f.html(1,1),offscreen=f.create(1,1);f.advance();
  const creates=f.calls.filter(c=>c.create).length;
  assert.equal(canvas.getContext('webgl'),null);assert.equal(canvas.getContext('2d'),null);
  assert.equal(offscreen.getContext('webgl'),null);assert.equal(offscreen.getContext('webgl2'),null);
  assert.equal(f.calls.filter(c=>c.create).length,creates);
  assert.throws(()=>{canvas.width=4;},{name:'InvalidStateError'});
  assert.throws(()=>canvas.removeAttribute('height'),{name:'InvalidStateError'});
  const current=f.html(1,1);assert.ok(current.getContext('webgl'));
  assert.equal(f.calls.findLast(c=>c.create).epoch,2);
});

test('WebGL bitmap transfer uses custom drawing storage size and follows later canvas resize',()=>{
  const f=fixture(),canvas=f.create(8,8),gl=canvas.getContext('webgl');
  gl.drawingBufferStorage(gl.RGBA8,2,3);f.native.get(1).pixels.fill(42);
  const bitmap=canvas.transferToImageBitmap();assert.equal(bitmap.width,2);assert.equal(bitmap.height,3);
  assert.deepEqual([canvas.width,canvas.height,gl.drawingBufferWidth,gl.drawingBufferHeight],[8,8,2,3]);
  assert.ok([...f.native.get(1).pixels].every(value=>value===0));
  const copy=f.create(2,3);copy.getContext('2d').drawImage(bitmap,0,0);assert.deepEqual(pixels(copy).slice(0,4),[42,42,42,42]);bitmap.close();
  canvas.width=4;canvas.height=1;const resized=canvas.transferToImageBitmap();
  assert.equal(resized.width,4);assert.equal(resized.height,1);resized.close();
});

test('wide gamut bitmap presentation converts at the 2D boundary without rewriting the bitmap',()=>{
  const f=fixture({convertedPixels:new Uint8Array([138,59,21,255])}),canvas=f.create(1,1),gl=canvas.getContext('webgl');
  gl.drawingBufferColorSpace='display-p3';f.native.get(1).pixels.set([128,64,32,255]);
  const bitmap=canvas.transferToImageBitmap(),copy=f.create(1,1);copy.getContext('2d').drawImage(bitmap,0,0);
  assert.deepEqual(pixels(copy),[138,59,21,255]);
  const conversion=f.calls.find(call=>call.convertColor);assert.equal(conversion.source,'display-p3');assert.equal(conversion.target,'srgb');
  const other=f.create(1,1).getContext('webgl');other.texImage2D(other.TEXTURE_2D,0,other.RGBA,other.RGBA,other.UNSIGNED_BYTE,bitmap);
  const upload=f.calls.findLast(call=>call.operation?.kind==='textureSource');assert.equal(upload.operation.value.source_space,'display-p3');
  bitmap.close();
});
test('2D color conversion failures preserve the destination and source ownership',()=>{
  const f=fixture({colorFails:true}),canvas=f.create(1,1),gl=canvas.getContext('webgl');gl.drawingBufferColorSpace='display-p3';
  const bitmap=canvas.transferToImageBitmap(),copy=f.create(1,1),ctx=copy.getContext('2d');
  ctx.fillStyle='#0000ff';ctx.fillRect(0,0,1,1);const before=pixels(copy);
  assert.throws(()=>ctx.drawImage(bitmap,0),{name:'TypeError'});
  ctx.drawImage(bitmap,NaN,0);ctx.drawImage(bitmap,0,0,0,1);
  assert(!f.calls.some(call=>call.convertColor));assert.deepEqual(pixels(copy),before);
  assert.throws(()=>ctx.drawImage(bitmap,0,0),{name:'InvalidStateError'});assert.deepEqual(pixels(copy),before);assert.equal(bitmap.width,1);bitmap.close();
});

test('ImageData creation and reads use private branded results and normalized rectangles',()=>{
  const f=fixture(),ctx=f.create(2,1).getContext('2d');ctx.fillStyle='#ff0000';ctx.fillRect(0,0,1,1);
  const image=ctx.getImageData(1,1,-2,-1);assert(image instanceof f.sandbox.ImageData);assert.deepEqual(Array.from(image.data),[0,0,0,0,255,0,0,255]);
  const source=new f.sandbox.ImageData(2,1,{colorSpace:'display-p3'});const blank=ctx.createImageData(source);
  assert.equal(blank.colorSpace,'display-p3');assert.notEqual(blank.data,source.data);assert(blank.data.every(v=>v===0));
  assert.equal(ctx.createImageData(-2,-1).width,2);
  assert.throws(()=>ctx.getImageData(0,0,0,1),{name:'IndexSizeError'});assert.throws(()=>ctx.createImageData(),{name:'TypeError'});
  assert.throws(()=>ctx.getImageData.call({},0,0,1,1),{name:'TypeError'});assert.throws(()=>ctx.putImageData({},0,0),{name:'TypeError'});
});
test('ImageData dirty writes clip correctly and ignore compositing and page metadata',()=>{
  const f=fixture(),canvas=f.create(2,1),ctx=canvas.getContext('2d'),image=new f.sandbox.ImageData(new Uint8ClampedArray([255,0,0,128,0,255,0,64]),2);
  ctx.globalAlpha=0;ctx.globalCompositeOperation='multiply';Object.defineProperty(image,'width',{value:999});
  ctx.putImageData(image,0,0,2,1,-1,-1);assert.deepEqual(pixels(canvas),[0,0,0,0,0,255,0,64]);
  ctx.putImageData(image,-1,0);assert.deepEqual(pixels(canvas),[0,255,0,64,0,255,0,64]);
  const before=pixels(canvas);ctx.putImageData(image,10,0);assert.deepEqual(pixels(canvas),before);
  assert.throws(()=>ctx.putImageData(image,0,0,1),{name:'TypeError'});
  structuredClone(image.data.buffer,{transfer:[image.data.buffer]});assert.throws(()=>ctx.putImageData(image,0,0),{name:'InvalidStateError'});assert.deepEqual(pixels(canvas),before);
});
test('ImageData color reads and writes convert only the destination and retain failure state',()=>{
  const f=fixture({convertedPixels:new Uint8Array([234,51,35,255])}),canvas=f.create(1,1),ctx=canvas.getContext('2d');ctx.fillStyle='#ff0000';ctx.fillRect(0,0,1,1);
  const image=ctx.getImageData(0,0,1,1,{colorSpace:'display-p3'});assert.equal(image.colorSpace,'display-p3');assert.deepEqual(Array.from(image.data),[234,51,35,255]);assert.deepEqual(pixels(canvas),[255,0,0,255]);
  const converted=fixture({convertedPixels:new Uint8Array([138,59,21,255])}),target=converted.create(1,1),context=target.getContext('2d');
  const wide=new converted.sandbox.ImageData(new Uint8ClampedArray([128,64,32,255]),1,1,{colorSpace:'display-p3'});context.putImageData(wide,0,0);
  assert.deepEqual(pixels(target),[138,59,21,255]);assert.deepEqual(Array.from(wide.data),[128,64,32,255]);
  const failed=fixture({colorFails:true}),output=failed.create(1,1),out=output.getContext('2d');out.fillStyle='#0000ff';out.fillRect(0,0,1,1);
  const input=new failed.sandbox.ImageData(1,1,{colorSpace:'display-p3'}),before=pixels(output);
  out.putImageData(input,2,2);assert(!failed.calls.some(call=>call.convertColor));
  assert.throws(()=>out.putImageData(input,0,0),{name:'InvalidStateError'});assert.deepEqual(pixels(output),before);
  assert.throws(()=>out.getImageData(0,0,1,1,{colorSpace:'display-p3'}),{name:'InvalidStateError'});assert.deepEqual(pixels(output),before);
});

test('ImageData operations observe resize and detachment during argument conversion',()=>{
  const f=fixture(),canvas=f.create(1,1),ctx=canvas.getContext('2d');ctx.fillStyle='#ff0000';ctx.fillRect(0,0,1,1);
  const image=ctx.getImageData(0,0,1,1,{get colorSpace(){canvas.width=2;return 'srgb';}});
  assert.deepEqual(Array.from(image.data),[0,0,0,0]);
  const input=new f.sandbox.ImageData(new Uint8ClampedArray([17,33,65,255]),1);
  ctx.putImageData(input,{valueOf(){canvas.width=1;return 0;}},0);assert.deepEqual(pixels(canvas),[17,33,65,255]);
  const before=pixels(canvas);
  assert.throws(()=>ctx.putImageData(input,0,0,{valueOf(){structuredClone(input.data.buffer,{transfer:[input.data.buffer]});return 0;}},0,1,1),{name:'InvalidStateError'});
  assert.deepEqual(pixels(canvas),before);
});

test('placeholder color snapshots retain source gamut separately from browser serialization',async()=>{
  const f=fixture({presentedPixels:new Uint8Array([138,59,21,255]),convertedPixels:new Uint8Array([138,59,21,255])});
  const html=f.html(1,1),offscreen=html.transferControlToOffscreen(),gl=offscreen.getContext('webgl',{preserveDrawingBuffer:true});
  gl.drawingBufferColorSpace='display-p3';f.native.get(1).pixels.set([128,64,32,255]);gl.clear(gl.COLOR_BUFFER_BIT);f.drain();
  assert.deepEqual(Array.from(f.presented.get(1).sourceBytes),[128,64,32,255]);assert.deepEqual(Array.from(f.presented.get(1).bytes),[138,59,21,255]);
  const png=Buffer.from(html.toDataURL().split(',')[1],'base64'),length=png.readUInt32BE(33);
  assert.deepEqual(Array.from(zlib.inflateSync(png.subarray(41,41+length))).slice(1),[138,59,21,255]);
  const target=f.create(1,1),ctx=target.getContext('2d');ctx.drawImage(html,0,0);assert.deepEqual(pixels(target),[138,59,21,255]);
  const consumer=f.create(1,1).getContext('webgl');consumer.unpackColorSpace='display-p3';
  const upload=image=>{consumer.texImage2D(consumer.TEXTURE_2D,0,consumer.RGBA,consumer.RGBA,consumer.UNSIGNED_BYTE,image);return f.calls.findLast(call=>call.operation?.kind==='textureSource');};
  const direct=upload(html);assert.deepEqual(direct.data,[128,64,32,255]);assert.equal(direct.operation.value.source_space,'display-p3');
  const bitmap=await settle(f,f.sandbox.createImageBitmap(html));const copied=upload(bitmap);
  assert.deepEqual(copied.data,[128,64,32,255]);assert.equal(copied.operation.value.source_space,'display-p3');bitmap.close();
  assert(f.calls.some(call=>call.placeholder==='pixels'));assert(f.calls.some(call=>call.placeholder==='sourcePixels'));
});
test('placeholder source color belongs to the last committed frame across changes and failure',()=>{
  const options={},f=fixture(options),html=f.html(1,1),offscreen=html.transferControlToOffscreen(),gl=offscreen.getContext('webgl');
  gl.drawingBufferColorSpace='display-p3';f.native.get(1).pixels.set([255,0,0,255]);gl.clear(gl.COLOR_BUFFER_BIT);f.drain();
  const consumer=f.create(1,1).getContext('webgl'),upload=()=>{consumer.texImage2D(consumer.TEXTURE_2D,0,consumer.RGBA,consumer.RGBA,consumer.UNSIGNED_BYTE,html);return f.calls.findLast(call=>call.operation?.kind==='textureSource');};
  gl.drawingBufferColorSpace='srgb';assert.equal(upload().operation.value.source_space,'display-p3');
  options.presentFails=true;f.drain();assert.equal(upload().operation.value.source_space,'display-p3');assert.equal(f.tasks.length,0);
  options.presentFails=false;gl.clear(gl.COLOR_BUFFER_BIT);f.drain();assert.equal(upload().operation.value.source_space,'srgb');assert.equal(f.presented.get(1).sourceBytes,null);
  const before=html.toDataURL();gl.getExtension('WEBGL_lose_context').loseContext();f.drain();assert.equal(html.toDataURL(),before);
});
test('placeholder source snapshot failures never submit partial texture pixels',()=>{
  const options={},f=fixture(options),html=f.html(1,1),offscreen=html.transferControlToOffscreen();offscreen.getContext('2d');f.drain();
  const gl=f.create(1,1).getContext('webgl');options.snapshotChanges=true;
  const before=f.calls.filter(call=>call.operation?.kind==='textureSource').length;
  assert.throws(()=>gl.texImage2D(gl.TEXTURE_2D,0,gl.RGBA,gl.RGBA,gl.UNSIGNED_BYTE,html),{name:'InvalidStateError'});
  assert.equal(f.calls.filter(call=>call.operation?.kind==='textureSource').length,before);
});

test('author dimensions preserve snapshots until dirty presentation replaces both attributes',()=>{
  const f=fixture(),html=f.html(2,1),off=html.transferControlToOffscreen(),ctx=off.getContext('2d');
  ctx.fillStyle='#ff0000';ctx.fillRect(0,0,2,1);f.drain();const red=html.toDataURL();
  html.setAttribute('width','90');assert.equal(off.width,2);assert.equal(html.toDataURL(),red);
  ctx.fillStyle='#0000ff';ctx.fillRect(0,0,2,1);f.drainTasks();
  assert.equal(html.width,90);assert.equal(html.toDataURL(),red);
  f.drain();assert.equal(html.width,2);assert.equal(html.height,1);assert.notEqual(html.toDataURL(),red);
});

test('empty offscreen frames retain placeholder pixels and author attributes until nonzero resize',()=>{
  for(const mode of ['2d','webgl','webgl2'])for(const axis of ['width','height']){
    const f=fixture(),html=f.html(2,1),off=html.transferControlToOffscreen(),ctx=off.getContext(mode,{preserveDrawingBuffer:true});
    const draw=()=>{if(mode==='2d'){ctx.fillStyle='#ff0000';ctx.fillRect(0,0,off.width,off.height);}else ctx.clear(ctx.COLOR_BUFFER_BIT);};
    draw();f.drain();const png=html.toDataURL(),revision=f.presented.get(1).revision;
    const calls=f.calls.filter(call=>call.placeholder==='presentCpu'||call.placeholder==='presentGl').length;
    html.setAttribute(axis,'90');off[axis]=0;f.drain();draw();f.drain();
    assert.equal(html.toDataURL(),png);assert.equal(html.getAttribute(axis),'90');
    assert.equal(f.presented.get(1).revision,revision);assert.equal(f.tasks.length,0);
    assert.equal(f.calls.filter(call=>call.placeholder==='presentCpu'||call.placeholder==='presentGl').length,calls);
    off[axis]=3;f.drain();assert.equal(html.getAttribute(axis),'3');
    assert.ok(f.presented.get(1).revision>revision);assert.equal(f.tasks.length,0);
  }
});
