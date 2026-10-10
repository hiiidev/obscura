// Included only with real graphics support. Standalone ownership never creates
// a hidden DOM element, and construction itself allocates no graphics backend.
// The same private closure owns ImageBitmap, WebGL and OffscreenCanvas. Never
// Reuse the native Blob constructor retained by the ImageBitmap adapter.
function offscreenPngBytes(width,height,bytes) {
  const dataURL = _encodePNG(width,height,bytes);
  const encoded = dataURL.split(',',2)[1];
  if (!encoded) throw new DOMException('Canvas encoding failed','EncodingError');
  const binary = atob(encoded), output = new Uint8Array(binary.length);
  for(let i=0;i<binary.length;i++) output[i]=binary.charCodeAt(i);
  return output;
}
function _canvas2DTake(context) {
  const pixels = _canvas2DPixels(context);
  const result = {...pixels,bytes:pixels.bytes.slice()};
  // Bitmap transfer consumes pixels, not the drawing state. A subsequent
  // resize is what resets styles and the transformation stack.
  context._buf.fill(0);
  if (context._offscreenOptions?.alpha === false) {
    for(let i=3;i<context._buf.length;i+=4)context._buf[i]=255;
  }
  context._originClean = true;
  schedulePlaceholder(context.canvas);
  return result;
}
function offscreenState(receiver) {
  const value=offscreens.get(receiver);
  if(!value)throw new TypeError('Illegal invocation');
  if(value.detached)throw new DOMException('The OffscreenCanvas is detached','InvalidStateError');
  return value;
}
function offscreenDimension(value) {
  const number=Math.trunc(+value);
  if(!Number.isFinite(number)||number<0||number>=18446744073709551616)throw new TypeError('Canvas dimensions are outside unsigned long long');
  return number;
}
function offscreenAllocation(width,height) {
  return width<=32767&&height<=32767&&width*height<=16777216;
}
function offscreenPixels(canvas,sourceColor=false) {
  const s=offscreenState(canvas);
  if(!offscreenAllocation(s.width,s.height))throw new DOMException('Canvas allocation is unavailable','InvalidStateError');
  if(s.mode==='2d')return _canvas2DPixels(s.context);
  if(s.mode==='webgl'||s.mode==='webgl2'){
    const pixels=_webglReadback(canvas,sourceColor);
    if(!pixels)throw new DOMException('Canvas rendering context is lost','InvalidStateError');
    return pixels;
  }
  return {width:s.width,height:s.height,bytes:new Uint8Array(s.width*s.height*4),originClean:true};
}
const Offscreen2D=illegal('OffscreenCanvasRenderingContext2D');
Object.setPrototypeOf(Offscreen2D.prototype,_Canvas2D.prototype);
const offscreen2DOwners=new WeakMap();
Object.defineProperty(Offscreen2D.prototype,'getContextAttributes',{enumerable:true,configurable:true,writable:true,value:function getContextAttributes(){
  const s=offscreen2DOwners.get(this);if(!s)throw new TypeError('Illegal invocation');
  return {...s.attributes};
}});
_markNative(Offscreen2D.prototype.getContextAttributes);
const encodeOffscreenPNG=_encodePNG;
class OffscreenCanvas {
  constructor(width,height) {
    if(arguments.length<2)throw new TypeError('OffscreenCanvas requires width and height');
    const nativeEpoch = __obscuraCore.ops.op_canvas_document_epoch?.(_realmFrameId);
    const epoch = Number.isInteger(nativeEpoch) && nativeEpoch !== 4294967295
      ? nativeEpoch : _canvasDocumentEpoch;
    offscreens.set(this,{width:offscreenDimension(width),height:offscreenDimension(height),mode:'none',context:null,detached:false,handlers:new Map(),frame:_realmFrameId,epoch,placeholder:null,presentationQueued:false});
  }
  get width(){return offscreenState(this).width;}
  set width(value){resizeOffscreen(this,'width',value);}
  get height(){return offscreenState(this).height;}
  set height(value){resizeOffscreen(this,'height',value);}
  getContext(type,options=null) {
    const s=offscreenState(this);
    if(arguments.length===0)throw new TypeError('Context type is required');
    type=text(type);
    if(!['2d','bitmaprenderer','webgl','webgl2','webgpu'].includes(type))throw new TypeError('Invalid OffscreenRenderingContextId');
    if(s.mode!=='none')return s.mode===type?s.context:null;
    if(type==='webgl'||type==='webgl2'){
      const context=_webglCreate(this,type,options);
      if(context){s.context=context;s.mode=type;schedulePlaceholder(this);}
      return context;
    }
    if(type!=='2d')return null;
    if(options!=null&&typeof options!=='object'&&typeof options!=='function')throw new TypeError('Context options must be a dictionary');
    const requestedAlpha=options?.alpha,requestedColorSpace=options?.colorSpace;
    const alpha=requestedAlpha===undefined?true:Boolean(requestedAlpha);
    const colorSpace=requestedColorSpace===undefined?'srgb':text(requestedColorSpace);
    if(!['srgb','display-p3'].includes(colorSpace))throw new TypeError('Invalid PredefinedColorSpace');
    const requestedColorType=options?.colorType;
    const colorType=requestedColorType===undefined?'unorm8':text(requestedColorType);
    if(!['unorm8','float16'].includes(colorType))throw new TypeError('Invalid CanvasColorType');
    Boolean(options?.desynchronized);
    const willReadFrequently=Boolean(options?.willReadFrequently);
    // Do not silently label sRGB bytes as P3 or byte storage as float16.
    // Unsupported CPU formats leave the context mode available for retry.
    if(colorSpace!=='srgb'||colorType!=='unorm8'||!offscreenAllocation(s.width,s.height))return null;
    try {
      const context=new _Canvas2D(this,{alpha,dimensions:()=>s,changed:()=>schedulePlaceholder(this)});
      Object.setPrototypeOf(context,Offscreen2D.prototype);
      s.attributes={alpha,colorSpace,colorType,desynchronized:false,willReadFrequently};
      offscreen2DOwners.set(context,s);s.context=context;s.mode='2d';schedulePlaceholder(this);
      return context;
    }catch(_error){return null;}
  }
  transferToImageBitmap() {
    const s=offscreenState(this);
    if(s.mode==='none')throw new DOMException('The canvas has no rendering context','InvalidStateError');
    if(!offscreenAllocation(s.width,s.height))throw new DOMException('Canvas allocation is unavailable','InvalidStateError');
    // Allocate the wrapper before consuming the old bitmap. If allocation
    // fails, the previous drawing buffer remains owned by its context.
    const bitmap=newBitmap(null);
    let pixels;
    if(s.mode==='2d')pixels=_canvas2DTake(s.context);
    else {
      const context=canvases.get(this),{width,height}=context.buffer;
      const bytes=new Uint8Array(width*height*4);
      if(!call(context,'transferBitmap',undefined,bytes).value)
        throw new DOMException('Canvas bitmap transfer failed','InvalidStateError');
      pixels={width,height,bytes,originClean:true,colorSpace:context.drawingColorSpace};
    }
    bitmaps.set(bitmap,{...pixels,premultiplied:false});
    return bitmap;
  }
  convertToBlob(options={}) {
    let snapshot;
    const frame=_realmFrameId;
    try {
      const s=offscreenState(this);
      if(options!=null&&typeof options!=='object'&&typeof options!=='function')throw new TypeError('ImageEncodeOptions must be a dictionary');
      const requestedType=options?.type,requestedQuality=options?.quality;
      if(requestedType!==undefined)text(requestedType);
      if(requestedQuality!==undefined)+requestedQuality;
      if(s.mode==='2d'&&!_canvas2DPixels(s.context).originClean)throw new DOMException('The canvas is not origin-clean','SecurityError');
      if(!s.width||!s.height)throw new DOMException('The canvas has no pixels','IndexSizeError');
      const pixels=offscreenPixels(this);
      snapshot={...pixels,bytes:pixels.bytes.slice()};
    }catch(error){return Promise.reject(error);}
    return new Promise((resolve,reject)=>queueContextTask(frame,()=>{
      try {
        // PNG is the required fallback when a requested format is unsupported.
        resolve(new NativeBlob([offscreenPngBytes(snapshot.width,snapshot.height,snapshot.bytes)],{type:'image/png'}));
      }catch(_error){reject(new DOMException('Canvas encoding failed','EncodingError'));}
    }));
  }
  addEventListener(type,callback,options){offscreenState(this);_eventTargetAdd(this,type,callback,options);}
  removeEventListener(type,callback,options){offscreenState(this);_eventTargetRemove(this,type,callback,options);}
  dispatchEvent(event){offscreenState(this);return _eventTargetDispatch(this,event);}
}
function resizeOffscreen(canvas,name,value) {
  const s=offscreenState(canvas),size=offscreenDimension(value);
  const previous=s[name];
  s[name]=size;
  if(s.mode==='2d'){
    try{s.context._resizeFromCanvas();}
    catch(error){s[name]=previous;throw error;}
  }
  else if(s.mode==='webgl'||s.mode==='webgl2')_webglResize(canvas);
  schedulePlaceholder(canvas);
}
Object.setPrototypeOf(OffscreenCanvas.prototype,EventTarget.prototype);
Object.defineProperty(OffscreenCanvas.prototype,Symbol.toStringTag,{value:'OffscreenCanvas',configurable:true});
for(const type of ['contextlost','contextrestored']){
  Object.defineProperty(OffscreenCanvas.prototype,'on'+type,{enumerable:true,configurable:true,
    get(){return offscreenState(this).handlers.get(type)?.callback||null;},
    set(callback){
      const s=offscreenState(this);const previous=s.handlers.get(type);
      if(typeof callback!=='function')callback=null;
      if(previous){previous.callback=callback;if(!callback){_eventTargetRemove(this,type,previous.listener);s.handlers.delete(type);}return;}
      if(callback){const handler={callback};handler.listener=event=>{if(handler.callback.call(this,event)===false)event.preventDefault();};s.handlers.set(type,handler);_eventTargetAdd(this,type,handler.listener);}
    }});
}
for(const name of Object.getOwnPropertyNames(OffscreenCanvas.prototype)){
  if(name==='constructor')continue;
  const descriptor=Object.getOwnPropertyDescriptor(OffscreenCanvas.prototype,name);
  for(const key of ['value','get','set'])if(typeof descriptor[key]==='function')_markNative(descriptor[key]);
  Object.defineProperty(OffscreenCanvas.prototype,name,{...descriptor,enumerable:true});
}
global('OffscreenCanvas',OffscreenCanvas);

// A placeholder owns only its last native snapshot. Its offscreen canvas
// refers back weakly, so neither posted tasks nor graphics keep a DOM wrapper
// alive. IDs are never reused across document/frame replacement.
const placeholders=new WeakMap();
const placeholderFinalizer=new FinalizationRegistry(record=>{
  __obscuraCore.ops.op_canvas_placeholder(record.frame,record.epoch,{kind:'retire',id:record.id},empty);
});
function placeholderOp(s,request,bytes=empty) {
  return __obscuraCore.ops.op_canvas_placeholder(s.frame,s.epoch,request,bytes);
}
const pendingPresentations=new Set();
_runCanvasPresentation=()=>{
  const batch=Array.from(pendingPresentations);
  pendingPresentations.clear();_canvasPresentationPending=false;
  for(const reference of batch){
    try{presentPlaceholder(reference);}catch(_error){
      const canvas=reference.deref(),state=canvas&&offscreens.get(canvas);
      if(state){state.presentationQueued=false;state.presentationFailure='Canvas presentation failed';}
    }
  }
};
function schedulePlaceholder(canvas) {
  const s=offscreens.get(canvas);
  if(!s?.placeholder||s.mode==='none'||s.presentationQueued||!s.placeholder.target.deref())return;
  s.presentationQueued=true;
  const reference=new NativeWeakRef(canvas);
  // The posted wake also works for a synchronous embedder which has not
  // entered Tokio yet. Publication itself belongs to the shared frame phase.
  queueContextTask(s.frame,()=>{
    const current=reference.deref(),state=current&&offscreens.get(current);
    if(!state?.placeholder?.target.deref())return;
    pendingPresentations.add(reference);_canvasPresentationPending=true;
    _scheduleRenderingOpportunity();
  });
}
// Only this native adapter can publish the private WebGL presentation hook.
scheduleOffscreenPresentation = schedulePlaceholder;
// The outer Canvas2D renderer knows only this private callback, not resource
// maps or page-owned object properties.
_canvasPrivateImageSource = source => {
  const offscreen=offscreens.get(source);
  if(offscreen)return offscreenPixels(source,true);
  const bitmap=bitmaps.get(source);
  if(bitmap){
    if(!bitmap.bytes)throw new DOMException('Detached ImageBitmap','InvalidStateError');
    return bitmap;
  }
  return null;
};
function presentPlaceholder(reference) {
  const canvas=reference.deref(),current=canvas&&offscreens.get(canvas);
  if(!current)return;
  current.presentationQueued=false;
  const target=current.placeholder?.target.deref();
  if(!target||!_canvasDOMOwner(target))return;
  // An empty offscreen bitmap cannot publish a frame. Keep the last presented
  // pixels and author attributes until a later nonempty resize or draw.
  if(!current.width||!current.height)return;
  let result;
  if(current.mode==='2d'){
    const pixels=_canvas2DPixels(current.context);
    const bytes=new Uint8Array(pixels.bytes.buffer,pixels.bytes.byteOffset,pixels.bytes.byteLength);
    result=placeholderOp(current,{kind:'presentCpu',id:current.placeholder.id,width:pixels.width,height:pixels.height,origin_clean:pixels.originClean},bytes);
  }else if(current.mode==='webgl'||current.mode==='webgl2'){
    result=placeholderOp(current,{kind:'presentGl',id:current.placeholder.id,context:canvases.get(canvas).id});
  }
  // Failure preserves the last frame. Retry only after further context
  // activity or resize, avoiding a task loop during backend recovery.
  if(result?.status==='ready'){
    _reflectCanvasDimensions(target,result.width,result.height);
    delete current.presentationFailure;
  }else if(result)current.presentationFailure=result.reason;
}

_placeholderHas=canvas=>placeholders.has(canvas);
_transferOffscreen=function transferControlToOffscreen(){
  if(!(this instanceof HTMLCanvasElement)||_cache.get(this._nid)!==this)throw new TypeError('Illegal invocation');
  const owner=_requireCanvasOwner(this);
  if(placeholders.has(this)||_canvas2DContext(this)||_webglHas(this))throw new DOMException('The canvas has a rendering context','InvalidStateError');
  const canvas=new OffscreenCanvas(this.width,this.height),s=offscreens.get(canvas);
  const reply=placeholderOp(s,{kind:'register',node:owner.node,width:s.width,height:s.height});
  if(reply.status!=='ready')throw new DOMException(reply.reason,'InvalidStateError');
  const record={frame:s.frame,epoch:s.epoch,id:reply.id};
  try{
    placeholderFinalizer.register(this,record);
    s.placeholder={id:reply.id,target:new NativeWeakRef(this)};
    placeholders.set(this,record);
  }catch(error){placeholderOp(s,{kind:'retire',id:reply.id});throw error;}
  return canvas;
};
_placeholderPixels=(canvas,allowTainted,sourceColor=false)=>{
  const record=placeholders.get(canvas);
  if(!record)throw new TypeError('Illegal invocation');
  const info=placeholderOp(record,{kind:'info',id:record.id});
  if(info.status!=='ready')throw new DOMException('Canvas snapshot is unavailable','InvalidStateError');
  if(!allowTainted&&!info.originClean)throw new DOMException('The canvas is not origin-clean','SecurityError');
  if(!offscreenAllocation(info.width,info.height))throw new DOMException('Canvas snapshot allocation is unavailable','InvalidStateError');
  const bytes=new Uint8Array(info.width*info.height*4);
  const snapshot=placeholderOp(record,{kind:sourceColor?'sourcePixels':'pixels',id:record.id,revision:info.revision},bytes);
  if(snapshot.status!=='ready')throw new DOMException('Canvas snapshot changed','InvalidStateError');
  if(!allowTainted&&!snapshot.originClean)throw new DOMException('The canvas is not origin-clean','SecurityError');
  return {width:info.width,height:info.height,bytes,originClean:snapshot.originClean,colorSpace:snapshot.colorSpace||'srgb'};
};
_placeholderBlob=(canvas,callback,type,quality)=>{
  if(typeof callback!=='function')throw new TypeError('A Blob callback is required');
  if(type!==undefined)text(type);
  let pixels;try{pixels=_placeholderPixels(canvas,false);}catch(error){if(error.name==='SecurityError')throw error;}
  const record=placeholders.get(canvas);
  queueContextTask(record.frame,()=>{
    let blob=null;
    try{if(pixels?.width&&pixels.height)blob=new NativeBlob([offscreenPngBytes(pixels.width,pixels.height,pixels.bytes)],{type:'image/png'});}
    catch(_error){}
    callback(blob);
  });
};
