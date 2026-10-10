// Inserted inside bootstrap's private closure only for the `webgl` feature.
// Native context IDs and object ownership never become page-owned properties.
(() => {
  const contexts = new WeakMap();
  const canvases = new WeakMap();
  const offscreens = new WeakMap();
  function canvasSize(canvas) { return offscreens.get(canvas) || canvas; }
  const objects = new WeakMap();
  // Finalizers hold a weak context reference: registering a resource must not
  // keep its entire document alive. Bound/attached objects are retained by the
  // private reference graph below, never by the wrapper lookup table itself.
  const NativeWeakRef = WeakRef;
  const resourceFinalizer = new FinalizationRegistry(record => {
    const s = record.owner.deref();
    if (!s || s.lost || s.generation !== record.generation) return;
    if (s.handles.get(record.key) !== record.reference || record.reference.deref()) return;
    s.handles.delete(record.key);
    call(s, record.kind === 'location' ? 'collectLocation' : 'collect', {id:record.id});
  });
  const secret = Object.freeze({});
  const invalid = Symbol();
  const empty = new Uint8Array(0);
  const classes = Object.create(null);
  const uint = value => (+value) >>> 0;
  const int = value => (+value) >> 0;
  const text = value => { if (typeof value === 'symbol') throw new TypeError('Cannot convert a Symbol to a string'); return String(value); };
  function global(name, value) {
    Object.defineProperty(globalThis, name, {value, writable:true, configurable:true});
    _markNative(value);
  }
  function illegal(name, parent) {
    const ctor = {[name]: function(key) { if (key !== secret) throw new TypeError('Illegal constructor'); }}[name];
    if (parent) Object.setPrototypeOf(ctor.prototype, parent.prototype);
    Object.defineProperty(ctor.prototype, Symbol.toStringTag, {value:name, configurable:true});
    global(name, ctor);
    return ctor;
  }
  const WebGLObject = illegal('WebGLObject');
  for (const [kind, suffix] of Object.entries({buffer:'Buffer',texture:'Texture',shader:'Shader',program:'Program',framebuffer:'Framebuffer',renderbuffer:'Renderbuffer',vertexArray:'VertexArrayObject',query:'Query',sampler:'Sampler',transformFeedback:'TransformFeedback',sync:'Sync',location:'UniformLocation',active:'ActiveInfo',precision:'ShaderPrecisionFormat'})) {
    classes[kind] = illegal('WebGL' + suffix, ['location','active','precision'].includes(kind) ? null : WebGLObject);
  }
  const Context1 = illegal('WebGLRenderingContext');
  const Context2 = illegal('WebGL2RenderingContext');
  const eventMessages = new WeakMap();
  class WebGLContextEvent extends Event {
    constructor(type, init = {}) {
      super(type, init);
      eventMessages.set(this, init?.statusMessage === undefined ? '' : text(init.statusMessage));
    }
  }
  const statusMessage = function() {
    if(!eventMessages.has(this))throw new TypeError('Illegal invocation');
    return eventMessages.get(this);
  };
  _markNative(statusMessage);
  Object.defineProperty(WebGLContextEvent.prototype,'statusMessage',{get:statusMessage,enumerable:true,configurable:true});
  Object.defineProperty(WebGLContextEvent.prototype,Symbol.toStringTag,{value:'WebGLContextEvent',configurable:true});
  global('WebGLContextEvent',WebGLContextEvent);
  function state(receiver) {
    const s = contexts.get(receiver);
    if (!s) throw new TypeError('Illegal invocation');
    return s;
  }
  function error(s, code) {
    if (s === null) throw new DOMException('The image source is unavailable','InvalidStateError');
    call(s, 'error', {error:code});
  }
  function queueContextTask(frame, fn) {
    // Native posted tasks also work when a synchronous embedder creates or
    // loses a context before entering Tokio. Timers would silently discard it.
    const generation = __obscuraCore.ops.op_posted_task(frame, current => {
      if (current === generation && current >= 0) fn();
    });
  }
  function lossCallback(reference) {
    return () => { const s = reference.deref(); if (s) notifyLoss(s); };
  }
  function notifyLoss(s) {
    s.lost = true;
    if (s.lossQueued) return;
    s.lossQueued = true;
    s.lossErrorPending = true;
    s.generation++;
    for (const reference of s.handles.values()) {
      const object = reference.deref();
      if (object) objects.get(object).references.clear();
    }
    s.handles.clear();
    s.bindings.clear();
    s.defaultVertexArray.clear();
    s.defaultTransformFeedback.clear();
    s.activeTexture = 0;
    s.extensions.clear();
    queueContextTask(s.frame, () => {
      const event = new WebGLContextEvent('webglcontextlost', {cancelable:true,statusMessage:'WebGL context lost'});
      s.canvas.dispatchEvent(event);
      s.restoreAllowed = event.defaultPrevented;
    });
  }
  function lostReply(s, kind, request) {
    // A retired document or a failed native call may have no result payload.
    // The retained browser wrapper must still obey each API's return type.
    if (kind === 'getError') {
      const value = s.lossErrorPending ? 0x9242 : 0;
      s.lossErrorPending = false;
      return {type:'number',value};
    }
    if (kind === 'isLost') return {type:'boolean',value:true};
    if (kind === 'create') return {type:'number',value:0};
    if (['restore','readback','sourceReadback','transferBitmap'].includes(kind)) return {type:'boolean',value:false};
    if (['attributes','supportedExtensions','extension'].includes(kind)) return {type:'null',value:null};
    if (kind === 'query' || kind === 'advanced' || kind === 'getUniform') {
      let value = {type:'null'};
      switch (request?.method) {
        case 'isEnabled': case 'isObject': value = {type:'bool',value:false}; break;
        case 'getAttribLocation': case 'getFragDataLocation': value = {type:'int',value:-1}; break;
        case 'checkFramebufferStatus': value = {type:'uint',value:0x8CDD}; break;
        case 'getVertexAttribOffset': case 'getUniformBlockIndex': value = {type:'uint',value:0}; break;
        case 'clientWaitSync': value = {type:'uint',value:0x911D}; break;
      }
      return {type:'query',value};
    }
    return {type:'none'};
  }
  function call(s, kind, value, bytes = empty) {
    const operation = value === undefined ? {kind} : {kind, value};
    let sharedDestination = null;
    if (bytes !== empty) {
      try { arrayBufferLength.call(byteBuffer.call(bytes)); }
      catch {
        // Never expose concurrently shared storage as a Rust mutable slice.
        // Check the native budget before allocating; ordinary buffers stay zero-copy.
        if (byteLength.call(bytes) > maxTransferBytes) { error(s,0x0505); return {type:'none'}; }
        if (kind === 'readPixels' || kind === 'bufferRead') sharedDestination = bytes;
        try { bytes = new NativeBytes(bytes); }
        catch { error(s,0x0505); return {type:'none'}; }
      }
    }
    const reply = __obscuraCore.ops.op_webgl_call(s.frame, s.id, operation, bytes);
    if (!reply.lost) gpuVariance(s, kind, value, bytes, reply.value);
    if (sharedDestination && !reply.lost) {
      // A failed read has no copyback, including a concurrent writer's changes.
      // Pixel packing also leaves skipped rows, padding and trailing bytes alone.
      if (kind === 'bufferRead' && reply.value?.type === 'boolean' && reply.value.value === true)
        byteSet.call(sharedDestination,bytes);
      if (kind === 'readPixels' && reply.value?.type === 'pixelRead' && reply.value.value) {
        const {start,stride,rows,row_bytes} = reply.value.value;
        for (let row=0;row<rows;row++) {
          const offset=start+row*stride;
          byteSet.call(sharedDestination,byteSubarray.call(bytes,offset,offset+row_bytes),offset);
        }
      }
    }
    if (reply.lost) notifyLoss(s);
    s.lost = reply.lost;
    if (reply.lost) return lostReply(s, kind, value);
    if (!reply.lost && reply.accepted === true) retainReferences(s, kind, value);
    if (!reply.lost && reply.dirty) schedulePlaceholder(s.canvas);
    return reply.value;
  }
  // Per-profile readback variance (see _fpGpuVariance): RGBA8 reads of
  // rendered pixels, applied before any copy to a shared destination.
  function gpuVariance(s, kind, request, bytes, result) {
    if (!_fpRenderVariance()) return;
    if (kind === 'readPixels') {
      if (request?.format !== 0x1908 || request?.data_type !== 0x1401) return;
      const layout = result?.type === 'pixelRead' ? result.value : null;
      if (!layout || !layout.rows || layout.row_bytes < 4) return;
      _fpGpuVariance(bytes, layout.start, layout.stride, layout.row_bytes / 4, layout.rows, request.x, request.y, true);
    } else if (kind === 'readback' || kind === 'sourceReadback' || kind === 'transferBitmap' || kind === 'transferSourceBitmap') {
      if (result?.type !== 'boolean' || result.value !== true || !s.buffer) return;
      const {width, height} = s.buffer;
      if (bytes.length !== width * height * 4) return;
      _fpGpuVariance(bytes, 0, width * 4, width, height, 0, height - 1, false);
    }
  }
  function rememberDrawingBuffer(s, reply) {
    if(reply?.type==='drawingBuffer')s.buffer=reply.value;
  }
  function colorSpace(value) {
    const name=text(value);
    if(name!=='srgb'&&name!=='display-p3')throw new TypeError('Invalid PredefinedColorSpace');
    return name;
  }
  function pixelsInColorSpace(source,target) {
    const from=source.colorSpace||'srgb';
    if(from===target)return source;
    const bytes=source.bytes.slice();
    if(!__obscuraCore.ops.op_webgl_convert_color(from,target,source.premultiplied===true,bytes))
      throw new DOMException('Image color conversion failed','InvalidStateError');
    return {...source,bytes,colorSpace:target};
  }
  function obj(s, value, kind, nullable = true) {
    if (value == null && nullable) return 0;
    const object = objects.get(value);
    if (!object || object.kind !== kind) throw new TypeError('Expected WebGL ' + kind);
    if (object.owner !== s || object.generation !== s.generation) { error(s, 0x0502); return invalid; }
    return object.id;
  }
  function isObject(s, value, kind) {
    if (value == null) return false;
    const object = objects.get(value);
    if (!object || object.kind !== kind) throw new TypeError('Expected WebGL ' + kind);
    // Predicates return false for foreign or invalidated objects without
    // generating the mutation APIs' INVALID_OPERATION error.
    if (object.owner !== s || object.generation !== s.generation) return false;
    return Boolean(query(s,'isObject',{kind,id:object.id}));
  }
  function wrapper(s, kind, id) {
    if (!id) return null;
    const key = kind + ':' + id;
    const existing = s.handles.get(key)?.deref();
    if (existing) return existing;
    const value = new classes[kind](secret);
    objects.set(value, {owner:s, kind, id, generation:s.generation, references:new Map(), deleted:false, activeFeedback:0});
    const reference = new NativeWeakRef(value);
    s.handles.set(key, reference);
    resourceFinalizer.register(value, {owner:new NativeWeakRef(s), kind, id, key, generation:s.generation, reference});
    return value;
  }
  function setReference(map, key, object) {
    if (object) map.set(key, object); else map.delete(key);
  }
  function containerReferences(s, key, fallback) {
    const object = s.bindings.get(key);
    return object ? objects.get(object).references : fallback;
  }
  function vertexReferences(s) { return containerReferences(s, 'vertexArray', s.defaultVertexArray); }
  function feedbackReferences(s) { return containerReferences(s, 'transformFeedback', s.defaultTransformFeedback); }
  function framebufferReferences(s, target) {
    return containerReferences(s, target === 0x8CA8 ? 'readFramebuffer' : 'drawFramebuffer', null);
  }
  function removeReferences(map, object) {
    if (map) for (const [key, value] of map) if (value === object) map.delete(key);
  }
  function releaseDeletedProgram(s, program) {
    if (!program) return;
    const meta = objects.get(program);
    if (meta.deleted && meta.activeFeedback === 0 && s.bindings.get('program') !== program) meta.references.clear();
  }
  function retainReferences(s, kind, args) {
    const roots = s.bindings;
    if (kind === 'resource') {
      const {method, args:a} = args.command;
      switch (method) {
        case 'bindBuffer':
          setReference(a.target === 0x8893 ? vertexReferences(s) : roots,
            a.target === 0x8893 ? 'element' : 'buffer:' + a.target, wrapper(s,'buffer',a.id)); return;
        case 'bindTexture': setReference(roots,'texture:' + s.activeTexture + ':' + a.target,wrapper(s,'texture',a.id)); return;
        case 'bindSampler': setReference(roots,'sampler:' + a.unit,wrapper(s,'sampler',a.id)); return;
        case 'bindFramebuffer': {
          const object = wrapper(s,'framebuffer',a.id);
          if (a.target !== 0x8CA8) setReference(roots,'drawFramebuffer',object);
          if (a.target !== 0x8CA9) setReference(roots,'readFramebuffer',object);
          return;
        }
        case 'bindRenderbuffer': setReference(roots,'renderbuffer',wrapper(s,'renderbuffer',a.id)); return;
        case 'bindVertexArray': setReference(roots,'vertexArray',wrapper(s,'vertexArray',a.id)); return;
        case 'bindTransformFeedback': setReference(roots,'transformFeedback',wrapper(s,'transformFeedback',a.id)); return;
        case 'bindBufferBase': case 'bindBufferRange': {
          const object = wrapper(s,'buffer',a.id);
          setReference(roots,'buffer:' + a.target,object);
          setReference(a.target === 0x8C8E ? feedbackReferences(s) : roots,'indexed:' + a.target + ':' + a.index,object);
          return;
        }
        case 'framebufferTexture2D': case 'framebufferTextureLayer': case 'framebufferRenderbuffer': {
          const references = framebufferReferences(s,a.target);
          if (!references) return;
          const object = wrapper(s,method === 'framebufferRenderbuffer' ? 'renderbuffer' : 'texture',a.id);
          // DEPTH_STENCIL_ATTACHMENT aliases both independent attachment points.
          for (const point of a.attachment === 0x821A ? [0x8D00,0x8D20] : [a.attachment])
            setReference(references,'attachment:' + point,object);
          return;
        }
        case 'beginQuery': setReference(roots,'query:' + a.target,wrapper(s,'query',a.id)); return;
      }
    } else if (kind === 'command') {
      const a = args.args;
      switch (args.method) {
        case 'activeTexture': s.activeTexture = a.texture - 0x84C0; return;
        case 'vertexAttribPointer': case 'vertexAttribIPointer':
          setReference(vertexReferences(s),'attribute:' + a.index,roots.get('buffer:' + 0x8892)); return;
        case 'endQuery': roots.delete('query:' + a.target); return;
        case 'beginTransformFeedback': {
          const feedback = roots.get('transformFeedback');
          if (feedback) roots.set('activeFeedback:' + objects.get(feedback).id,feedback);
          const program = roots.get('program');
          if (program) objects.get(program).activeFeedback++;
          setReference(feedbackReferences(s),'activeProgram',program);
          return;
        }
        case 'endTransformFeedback': {
          const feedback = roots.get('transformFeedback');
          if (feedback) roots.delete('activeFeedback:' + objects.get(feedback).id);
          const program = feedbackReferences(s).get('activeProgram');
          feedbackReferences(s).delete('activeProgram');
          if (program) objects.get(program).activeFeedback--;
          releaseDeletedProgram(s,program);
          return;
        }
      }
    } else if (kind === 'attachShader') {
      const program = wrapper(s,'program',args.program);
      setReference(objects.get(program).references,'shader:' + args.shader,args.detach ? null : wrapper(s,'shader',args.shader));
    } else if (kind === 'useProgram') {
      const old = roots.get('program');
      const next = wrapper(s,'program',args.id);
      setReference(roots,'program',next);
      if (old !== next) releaseDeletedProgram(s,old);
    } else if (kind === 'delete' && args.id) {
      const object = wrapper(s,args.kind,args.id);
      const meta = objects.get(object);
      if (meta.deleted) return;
      meta.deleted = true;
      if (args.kind === 'program') {
        releaseDeletedProgram(s,object);
        return;
      }
      if (args.kind === 'shader') return; // Native deletion waits for detachment.
      // Deletion clears current bindings, but attachments in unbound containers
      // continue to own the old object even if ANGLE reuses its numeric name.
      if (args.kind === 'buffer') {
        removeReferences(vertexReferences(s),object);
        removeReferences(feedbackReferences(s),object);
      }
      if (args.kind === 'texture' || args.kind === 'renderbuffer') {
        removeReferences(framebufferReferences(s,0x8CA8),object);
        removeReferences(framebufferReferences(s,0x8CA9),object);
      }
      removeReferences(roots,object);
      meta.references.clear();
    }
  }
  function retainedQuery(s, method, args, result) {
    // Driver names may be recycled after deletion. Return the exact retained
    // wrapper for container-owned objects, rather than looking up that name.
    if (result?.type !== 'object') return queryValue(s,result);
    let object;
    if (method === 'getFramebufferAttachmentParameter' && args.name === 0x8CD1)
      object = framebufferReferences(s,args.target)?.get('attachment:' + (args.attachment === 0x821A ? 0x8D00 : args.attachment));
    if (method === 'getVertexAttrib' && args.name === 0x889F)
      object = vertexReferences(s).get('attribute:' + args.index);
    if (method === 'getIndexedParameter' && [0x8A28,0x8C8F].includes(args.target)) {
      const target = args.target === 0x8A28 ? 0x8A11 : 0x8C8E;
      object = (target === 0x8C8E ? feedbackReferences(s) : s.bindings).get('indexed:' + target + ':' + args.index);
    }
    if (method === 'getParameter' && args.name === 0x8895) object = vertexReferences(s).get('element');
    return object || queryValue(s,result);
  }
  function queryValue(s, result) {
    if (!result || result.type === 'null') return null;
    const value = result.value;
    switch (result.type) {
      case 'values': return value.map(item=>queryValue(s,item));
      case 'float32': return new Float32Array(value);
      case 'int32': return new Int32Array(value);
      case 'uint32': return new Uint32Array(value);
      case 'object': return wrapper(s, value.kind, value.id);
      case 'active': {
        const info = new classes.active(secret);
        Object.defineProperties(info, {size:{value:value.size,enumerable:true},type:{value:value.data_type,enumerable:true},name:{value:value.name,enumerable:true}});
        return info;
      }
      case 'precision': {
        const info = new classes.precision(secret);
        Object.defineProperties(info, {rangeMin:{value:value.range_min,enumerable:true},rangeMax:{value:value.range_max,enumerable:true},precision:{value:value.precision,enumerable:true}});
        return info;
      }
      default: return value;
    }
  }
  function query(s, method, args) { return retainedQuery(s, method, args, call(s, 'query', {method,args}).value); }
  function resource(s, method, args, bytes) { call(s, 'resource', {command:{method,args},has_data:bytes !== undefined}, bytes || empty); }
  const arrayBufferLength = Object.getOwnPropertyDescriptor(ArrayBuffer.prototype,'byteLength').get;
  const NativeDataView = DataView, NativeBytes = Uint8Array;
  const bytePrototype = Object.getPrototypeOf(Uint8Array.prototype);
  const byteBuffer = Object.getOwnPropertyDescriptor(bytePrototype,'buffer').get;
  const byteLength = Object.getOwnPropertyDescriptor(bytePrototype,'byteLength').get;
  const byteSet = bytePrototype.set, byteSubarray = bytePrototype.subarray;
  const maxTransferBytes = 256 * 1024 * 1024; // Same bound as native pixels::MAX_TRANSFER_BYTES.
  function isBuffer(value) {
    if (value === null || typeof value !== 'object') return false;
    // Intrinsic getters test internal slots across realms, without trusting
    // instanceof or a page-controlled Symbol.toStringTag.
    try { arrayBufferLength.call(value); return true; } catch {}
    // SharedArrayBuffer may be exposed only after restoring the V8 snapshot.
    // DataView checks buffer internal slots without invoking user coercion.
    try { new NativeDataView(value,0,0); return true; } catch {}
    return false;
  }
  function bufferSize(value, unsigned = false) {
    let size = Math.trunc(+value); // ToNumber must reject Symbol and BigInt.
    if (!Number.isFinite(size)) return 0;
    // GLsizeiptr is a signed WebIDL long long, not an EnforceRange argument.
    if (!Number.isSafeInteger(size) || unsigned && size < 0)
      size = Number((unsigned ? BigInt.asUintN : BigInt.asIntN)(64,BigInt(size)));
    return size;
  }
  function typedBytes(value, allowBuffer = false) {
    if (ArrayBuffer.isView(value)) return new Uint8Array(value.buffer,value.byteOffset,value.byteLength);
    if (allowBuffer && isBuffer(value)) return new Uint8Array(value);
    throw new TypeError('Expected an ArrayBuffer view');
  }
  const typedArrayTag = Object.getOwnPropertyDescriptor(Object.getPrototypeOf(Uint8Array.prototype), Symbol.toStringTag).get;
  function pixelBytes(s,value,type,offset) {
    const expected = ({5120:'Int8Array',5121:'Uint8Array',5122:'Int16Array',5123:'Uint16Array',5124:'Int32Array',5125:'Uint32Array',5126:'Float32Array',5131:'Uint16Array',36193:'Uint16Array',33635:'Uint16Array',32819:'Uint16Array',32820:'Uint16Array',33640:'Uint32Array',35899:'Uint32Array',35902:'Uint32Array',34042:'Uint32Array'})[uint(type)];
    if(!ArrayBuffer.isView(value))throw new TypeError('Expected an ArrayBuffer view');
    const tag=typedArrayTag.call(value);
    if(!expected || tag!==expected && !(expected==='Uint8Array'&&tag==='Uint8ClampedArray')){error(s,0x0502);return null;}
    return elements(s,value,offset);
  }
  function bufferOffset(s,value) {
    const offset=Math.trunc(Number(value));
    if(s.version!==2||!Number.isSafeInteger(offset)||offset<0||offset>4294967295){error(s,0x0501);return null;}
    return offset;
  }
  function elements(s, value, offset, length, allowBuffer = false) {
    const bytes = typedBytes(value, allowBuffer);
    const elementSize = value.BYTES_PER_ELEMENT || 1;
    const count = bytes.byteLength / elementSize;
    const start = offset === undefined ? 0 : Math.trunc(Number(offset));
    const size = length === undefined || uint(length) === 0 ? count - start : uint(length);
    if (!Number.isSafeInteger(start) || start < 0 || start > count || size > count - start) { error(s, 0x0501); return null; }
    return new Uint8Array(bytes.buffer, bytes.byteOffset + start * elementSize, size * elementSize);
  }
  function method(name, length, implementation, version = 1) {
    const fn = {[name]: function(...args) {
      const s = state(this);
      if (args.length < length) throw new TypeError(name + ' requires at least ' + length + ' arguments');
      return implementation(s, ...args);
    }}[name];
    Object.defineProperty(fn, 'length', {value:length, configurable:true});
    _markNative(fn);
    for (const constructor of version === 2 ? [Context2] : [Context1,Context2]) {
      Object.defineProperty(constructor.prototype,name,{value:fn,writable:true,enumerable:true,configurable:true});
    }
  }
  for (const ctor of [Context1,Context2]) {
    for (const [name,get] of Object.entries({canvas:s=>s.canvas,drawingBufferWidth:s=>s.lost?0:Math.max(1,s.buffer.width),drawingBufferHeight:s=>s.lost?0:Math.max(1,s.buffer.height),drawingBufferFormat:s=>s.lost?0:s.buffer.format})) {
      const getter = function() { return get(state(this)); };
      _markNative(getter);
      Object.defineProperty(ctor.prototype,name,{get:getter,enumerable:true,configurable:true});
    }
    for(const [name,key,drawing] of [['drawingBufferColorSpace','drawingColorSpace',true],['unpackColorSpace','unpackColorSpace',false]]) {
      const get=function(){return state(this)[key];};
      const set=function(value){const s=state(this),converted=colorSpace(value);if(s[key]===converted)return;call(s,'colorSpace',{drawing,color_space:converted});s[key]=converted;};
      _markNative(get);_markNative(set);
      Object.defineProperty(ctor.prototype,name,{get,set,enumerable:true,configurable:true});
    }
  }
  method('getContextAttributes',0,s=>call(s,'attributes').value);
  method('drawingBufferStorage',3,(s,format,width,height)=>{
    rememberDrawingBuffer(s,call(s,'drawingBufferStorage',{format:uint(format),width:uint(width),height:uint(height)}));
  });
  method('isContextLost',0,s=>call(s,'isLost').value);
  method('getError',0,s=>call(s,'getError').value);
  method('getSupportedExtensions',0,s=>call(s,'supportedExtensions').value);
  // Runtime stealth presents the page's browser identity, not the host driver.
  // The unmasked strings reuse the document's seeded GPU pool (_fp), shared by
  // both context versions; version strings use Chrome's format. Null replies
  // (disabled extension, lost context) and non-stealth driver strings pass through.
  function identityParameter(s, name, value) {
    if (typeof value !== 'string' || !globalThis.__obscura_stealth) return value;
    switch (name) {
      case 0x9245: return _fp('gpuVendor');
      case 0x9246: return _fp('gpu');
      case 0x1F02: return s.version === 2 ? 'WebGL 2.0 (OpenGL ES 3.0 Chromium)' : 'WebGL 1.0 (OpenGL ES 2.0 Chromium)';
      case 0x8B8C: return s.version === 2 ? 'WebGL GLSL ES 3.00 (OpenGL ES GLSL ES 3.0 Chromium)' : 'WebGL GLSL ES 1.0 (OpenGL ES GLSL ES 1.0 Chromium)';
      default: return value;
    }
  }
  method('getParameter',1,(s,name)=>{const n=uint(name);return identityParameter(s,n,query(s,'getParameter',{name:n}));});
  method('isEnabled',1,(s,cap)=>query(s,'isEnabled',{cap:uint(cap)}));
  for (const [kind,suffix,version] of [['buffer','Buffer',1],['texture','Texture',1],['shader','Shader',1],['program','Program',1],['framebuffer','Framebuffer',1],['renderbuffer','Renderbuffer',1],['vertexArray','VertexArray',2],['query','Query',2],['sampler','Sampler',2],['transformFeedback','TransformFeedback',2]]) {
    method('create'+suffix,kind==='shader'?1:0,(s,type)=>wrapper(s,kind,call(s,'create',{kind,shader_type:kind==='shader'?uint(type):0}).value),version);
    method('delete'+suffix,1,(s,value)=>{const id=obj(s,value,kind);if(id!==invalid)call(s,'delete',{kind,id});},version);
    method('is'+suffix,1,(s,value)=>isObject(s,value,kind),version);
  }
  method('shaderSource',2,(s,shader,source)=>{const id=obj(s,shader,'shader',false);if(id!==invalid)call(s,'shaderSource',{id,source:text(source)});});
  method('compileShader',1,(s,shader)=>{const id=obj(s,shader,'shader',false);if(id!==invalid)call(s,'compileShader',{id});});
  for (const [name,detach] of [['attachShader',false],['detachShader',true]]) method(name,2,(s,p,sh)=>{
    const program=obj(s,p,'program',false),shader=obj(s,sh,'shader',false);
    if(program!==invalid&&shader!==invalid)call(s,'attachShader',{program,shader,detach});
  });
  for (const name of ['linkProgram','useProgram']) method(name,1,(s,value)=>{const id=obj(s,value,'program',name==='useProgram');if(id!==invalid)call(s,name,{id});});
  method('validateProgram',1,(s,value)=>{const program=obj(s,value,'program',false);if(program!==invalid)resource(s,'validateProgram',{program});});
  for (const [name,kind,param] of [['getShaderParameter','shader',true],['getShaderInfoLog','shader',false],['getShaderSource','shader',false],['getProgramParameter','program',true],['getProgramInfoLog','program',false]]) method(name,param?2:1,(s,value,p)=>{
    const id=obj(s,value,kind,false);if(id===invalid)return null;
    return query(s,name,{[kind]:id,...(param?{name:uint(p)}:{})});
  });
  method('getShaderPrecisionFormat',2,(s,type,precision)=>query(s,'getShaderPrecisionFormat',{shader_type:uint(type),precision_type:uint(precision)}));
  for (const name of ['getActiveAttrib','getActiveUniform']) method(name,2,(s,value,index)=>{const program=obj(s,value,'program',false);return program===invalid?null:query(s,name,{program,index:uint(index)});});
  for (const name of ['getAttribLocation','getUniformLocation']) method(name,2,(s,value,n)=>{
    const program=obj(s,value,'program',false);if(program===invalid)return name==='getAttribLocation'?-1:null;
    const result=query(s,name,{program,name:text(n)});
    if (name !== 'getUniformLocation') return result;
    const location = wrapper(s,'location',result);
    if (location) objects.get(location).references.set('program',value);
    return location;
  });
  method('getUniform',2,(s,p,l)=>{const program=obj(s,p,'program',false),location=obj(s,l,'location',false);return program===invalid||location===invalid?null:queryValue(s,call(s,'getUniform',{program,location}).value);});
  method('bindAttribLocation',3,(s,p,index,name)=>{const program=obj(s,p,'program',false);if(program!==invalid)resource(s,'bindAttribLocation',{program,index:uint(index),name:text(name)});});
  for (const [suffix,kind] of [['Buffer','buffer'],['Texture','texture'],['Framebuffer','framebuffer'],['Renderbuffer','renderbuffer']]) method('bind'+suffix,2,(s,target,value)=>{const id=obj(s,value,kind);if(id!==invalid)resource(s,'bind'+suffix,{target:uint(target),id});});
  for (const [name,kind,field] of [['bindSampler','sampler','unit'],['bindTransformFeedback','transformFeedback','target']]) method(name,2,(s,target,value)=>{const id=obj(s,value,kind);if(id!==invalid)resource(s,name,{[field]:uint(target),id});},2);
  method('bindVertexArray',1,(s,value)=>{const id=obj(s,value,'vertexArray');if(id!==invalid)resource(s,'bindVertexArray',{id});},2);
  method('bufferData',3,(s,target,data,usage,...range)=>{
    target=uint(target);
    // Only WebGL2's four/five-argument overload accepts an element range.
    // WebGL1 ignores excess arguments; three arguments select BufferSource
    // by internal brand and otherwise perform numeric WebIDL conversion.
    if(s.version===2 && range.length){
      typedBytes(data);usage=uint(usage);
      const offset=bufferSize(range[0],true),length=range[1]===undefined?0:uint(range[1]);
      const bytes=elements(s,data,offset,length);
      if(bytes)resource(s,'bufferData',{target,size:bytes.byteLength,usage},bytes);
      return;
    }
    if(data==null){uint(usage);error(s,0x0501);return;}
    if(ArrayBuffer.isView(data)||isBuffer(data)){
      const bytes=typedBytes(data,true);usage=uint(usage);
      resource(s,'bufferData',{target,size:bytes.byteLength,usage},bytes);return;
    }
    const size=bufferSize(data);usage=uint(usage);
    if(!Number.isSafeInteger(size)){error(s,0x0501);return;}
    resource(s,'bufferData',{target,size,usage});
  });
  method('bufferSubData',3,(s,target,offset,data,start,length)=>{const bytes=elements(s,data,start,length,true);const n=Math.trunc(Number(offset));if(!Number.isSafeInteger(n)){error(s,0x0501);return;}if(bytes)resource(s,'bufferSubData',{target:uint(target),offset:n},bytes);});
  method('getBufferSubData',3,(s,target,offset,data,start,length)=>{const bytes=elements(s,data,start,length);const n=Math.trunc(Number(offset));if(!Number.isSafeInteger(n)){error(s,0x0501);return;}if(bytes)call(s,'bufferRead',{target:uint(target),offset:n},bytes);},2);
  for (const name of ['getBufferParameter','getRenderbufferParameter','getTexParameter']) method(name,2,(s,target,p)=>query(s,name,{target:uint(target),name:uint(p)}));
  method('checkFramebufferStatus',1,(s,target)=>query(s,'checkFramebufferStatus',{target:uint(target)}));
  method('pixelStorei',2,(s,name,value)=>call(s,'pixelStore',{name:uint(name),value:int(value)}));
  method('framebufferTexture2D',5,(s,target,attachment,textureTarget,texture,level)=>{const id=obj(s,texture,'texture');if(id!==invalid)resource(s,'framebufferTexture2D',{target:uint(target),attachment:uint(attachment),texture_target:uint(textureTarget),id,level:int(level)});});
  method('framebufferTextureLayer',5,(s,target,attachment,texture,level,layer)=>{const id=obj(s,texture,'texture');if(id!==invalid)resource(s,'framebufferTextureLayer',{target:uint(target),attachment:uint(attachment),id,level:int(level),layer:int(layer)});},2);
  method('framebufferRenderbuffer',4,(s,target,attachment,renderbufferTarget,renderbuffer)=>{const id=obj(s,renderbuffer,'renderbuffer');if(id!==invalid)resource(s,'framebufferRenderbuffer',{target:uint(target),attachment:uint(attachment),renderbuffer_target:uint(renderbufferTarget),id});});
  for (const [suffix,type,convert,version] of [['f','float',Number,1],['i','int',int,1],['ui','uint',uint,2]]) for(let count=1;count<=4;count++) {
    method('uniform'+count+suffix,count+1,(s,location,...values)=>{const id=obj(s,location,'location');if(id!==invalid)call(s,'uniform',{location:id,columns:count,rows:1,values:{type,values:values.slice(0,count).map(convert)}});},version);
    method('uniform'+count+suffix+'v',2,(s,location,values,offset=0,length=0)=>{const id=obj(s,location,'location');if(id===invalid)return;const start=uint(offset),all=Array.from(values,convert),size=uint(length)||all.length-start;if(start>all.length||size>all.length-start){error(s,0x0501);return;}call(s,'uniform',{location:id,columns:count,rows:1,values:{type,values:all.slice(start,start+size)}});},version);
  }
  for(let columns=2;columns<=4;columns++)for(let rows=2;rows<=4;rows++)method('uniformMatrix'+columns+(rows===columns?'':'x'+rows)+'fv',3,(s,location,transpose,values,offset=0,length=0)=>{const id=obj(s,location,'location');if(id===invalid)return;const start=uint(offset),all=Array.from(values,Number),size=uint(length)||all.length-start;if(start>all.length||size>all.length-start){error(s,0x0501);return;}call(s,'uniform',{location:id,columns,rows,matrix:true,transpose:Boolean(transpose),values:{type:'float',values:all.slice(start,start+size)}});},columns===rows?1:2);
  method('readPixels',7,(s,x,y,width,height,format,type,data,offset=0)=>{
    const request={x:int(x),y:int(y),width:int(width),height:int(height),format:uint(format),data_type:uint(type)};
    if(typeof data==='number'){const n=bufferOffset(s,data);if(n!==null)call(s,'readPixelsBuffer',{request,offset:n});return;}
    const bytes=pixelBytes(s,data,type,offset);if(bytes)call(s,'readPixels',request,bytes);
  });
  function imageSource(s,source,allowTainted=false) {
    if(source==null){error(s,0x0501);return null;}
    if(bitmaps.has(source)){
      const image=bitmaps.get(source);
      if(!image.bytes){error(s,0x0501);return null;}
      if(!allowTainted&&!image.originClean)throw new DOMException('The image is not origin-clean','SecurityError');
      return {...image,bitmap:true};
    }
    if(offscreens.has(source)){
      const image=offscreenPixels(source,true);
      if(!allowTainted&&!image.originClean)throw new DOMException('The canvas is not origin-clean','SecurityError');
      return image;
    }
    if(source instanceof HTMLCanvasElement){
      if(_placeholderHas(source))return _placeholderPixels(source,allowTainted,true);
      if(canvases.has(source)){const image=_webglReadback(source,true);if(!image)error(s,0x0501);return image;}
      const pixels=_canvas2DPixels(_canvas2DContext(source));
      if(pixels){
        if(!allowTainted&&!pixels.originClean)throw new DOMException('The canvas is not origin-clean','SecurityError');
        return pixels;
      }
      const width=uint(source.width),height=uint(source.height);
      if(width>32767||height>32767||width*height>16777216){error(s,0x0501);return null;}
      return{width,height,bytes:new Uint8Array(width*height*4)};
    }
    if(source instanceof HTMLImageElement){
      const frame=s?s.frame:_realmFrameId;
      const info=__obscuraCore.ops.op_webgl_image_info(frame,source._nid,allowTainted);
      if(info.status==='security')throw new DOMException('The image is not origin-clean','SecurityError');
      if(info.status!=='ready'){error(s,0x0501);return null;}
      const bytes=new Uint8Array(info.width*info.height*4);
      const result=__obscuraCore.ops.op_webgl_image_pixels(frame,source._nid,bytes,allowTainted);
      if(result===2)throw new DOMException('The image is not origin-clean','SecurityError');
      if(result!==0&&!(allowTainted&&result===3)){error(s,0x0501);return null;}
      return{width:info.width,height:info.height,bytes,originClean:info.originClean!==false&&result===0};
    }
    // Media playback does not yet provide decoded video frames in this
    // engine. An empty/incomplete video is not a valid texture source.
    if(source instanceof HTMLVideoElement){error(s,0x0501);return null;}
    const pixels=imageDataSource(source);
    if(pixels)return pixels;
    throw new TypeError('Unsupported texture image source');
  }
  /* @obscura-imagedata */
  /* @obscura-imagebitmap */
  /* @obscura-offscreen */
  function upload(s,image,data,offset,sourceOverload=false){
    if(sourceOverload||data!=null&&typeof data!=='number'&&!ArrayBuffer.isView(data)){
      const source=imageSource(s,data);if(!source)return;
      if(image.width===undefined){image.width=source.width;image.height=source.height;}
      call(s,'textureSource',{image,width:source.width,height:source.height,bitmap:source.bitmap===true,source_space:source.colorSpace||'srgb',source_premultiplied:source.premultiplied===true},source.bytes);return;
    }
    const pbo=typeof data==='number',nativeOffset=pbo?bufferOffset(s,data):null;if(pbo&&nativeOffset===null)return;
    const bytes=data==null||pbo?undefined:pixelBytes(s,data,image.data_type,offset);if(data!=null&&!pbo&&!bytes)return;
    call(s,'textureImage',{image,has_data:bytes!==undefined,buffer_offset:nativeOffset},bytes||empty);
  }
  for(const sub of [false,true])method(sub?'texSubImage2D':'texImage2D',sub?7:6,(s,...args)=>{
    let target,level,internal_format,width,height,border=0,format,data_type,data,x=0,y=0;
    const sourceOverload=args.length<(sub?9:9);
    if(!sub&&sourceOverload){[target,level,internal_format,format,data_type,data]=args;}
    else if(sub&&sourceOverload){[target,level,x,y,format,data_type,data]=args;internal_format=format;}
    else if(!sub){[target,level,internal_format,width,height,border,format,data_type,data]=args;}
    else{[target,level,x,y,width,height,format,data_type,data]=args;internal_format=format;}
    upload(s,{target:uint(target),level:int(level),internal_format:int(internal_format),width:width===undefined?undefined:int(width),height:height===undefined?undefined:int(height),border:int(border),format:uint(format),data_type:uint(data_type),x:int(x),y:int(y),sub_image:sub},data,args[9],sourceOverload);
  });
  for(const sub of [false,true])method(sub?'texSubImage3D':'texImage3D',sub?11:10,(s,...args)=>{
    let target,level,internal_format,width,height,depth,border=0,format,data_type,data,x=0,y=0,z=0,offset;
    if(sub){[target,level,x,y,z,width,height,depth,format,data_type,data,offset]=args;internal_format=format;}
    else{[target,level,internal_format,width,height,depth,border,format,data_type,data,offset]=args;}
    upload(s,{target:uint(target),level:int(level),internal_format:int(internal_format),width:int(width),height:int(height),depth:int(depth),border:int(border),format:uint(format),data_type:uint(data_type),x:int(x),y:int(y),z:int(z),sub_image:sub,three_dimensional:true},data,offset);
  },2);
  // Compressed formats are extension capabilities. This backend bridge does
  // not advertise compression extensions yet, so every compressed format is
  // rejected with INVALID_ENUM rather than silently accepting an upload.
  for(const [name,length,dataIndex,version] of [['compressedTexImage2D',7,6,1],['compressedTexSubImage2D',8,7,1],['compressedTexImage3D',8,7,2],['compressedTexSubImage3D',10,9,2]])method(name,length,(s,...args)=>{
    if(typeof args[dataIndex]==='number'){
      if(s.version!==2||args.length<=dataIndex+1)throw new TypeError('Expected compressed texture data');
      if(bufferOffset(s,args[dataIndex+1])===null)return;
    }else{typedBytes(args[dataIndex]);}
    error(s,0x0500);
  },version);
  method('getExtension',1,(s,name)=>{
    const canonical=call(s,'extension',{name:text(name)}).value;if(!canonical)return null;
    if(s.extensions.has(canonical))return s.extensions.get(canonical);
    const extension={};
    if(canonical==='WEBGL_debug_renderer_info')Object.assign(extension,{UNMASKED_VENDOR_WEBGL:0x9245,UNMASKED_RENDERER_WEBGL:0x9246});
    if(canonical==='OES_standard_derivatives')extension.FRAGMENT_SHADER_DERIVATIVE_HINT_OES=0x8B8B;
    if(canonical==='OES_texture_half_float')extension.HALF_FLOAT_OES=0x8D61;
    if(canonical==='EXT_color_buffer_half_float')Object.assign(extension,{RGBA16F_EXT:0x881A,RGB16F_EXT:0x881B,FRAMEBUFFER_ATTACHMENT_COMPONENT_TYPE_EXT:0x8211,UNSIGNED_NORMALIZED_EXT:0x8C17});
    if(canonical==='WEBGL_depth_texture')extension.UNSIGNED_INT_24_8_WEBGL=0x84FA;
    if(canonical==='EXT_texture_filter_anisotropic')Object.assign(extension,{TEXTURE_MAX_ANISOTROPY_EXT:0x84FE,MAX_TEXTURE_MAX_ANISOTROPY_EXT:0x84FF});
    if(canonical==='WEBGL_lose_context')Object.assign(extension,{
      loseContext(){call(s,'lose');},
      restoreContext(){if(!s.restoreAllowed||!s.lost)return;queueContextTask(s.frame,()=>{if(!s.restoreAllowed||!s.lost)return;const size=canvasSize(s.canvas);if(size.width>32767||size.height>32767||size.width*size.height>16777216)return;const restored=call(s,'restore').value;if(restored){rememberDrawingBuffer(s,call(s,'drawingBufferInfo'));s.lossQueued=false;s.restoreAllowed=false;s.canvas.dispatchEvent(new WebGLContextEvent('webglcontextrestored'));}});},
    });
    if(canonical==='OES_vertex_array_object')Object.assign(extension,{VERTEX_ARRAY_BINDING_OES:0x85B5,
      createVertexArrayOES(){return wrapper(s,'vertexArray',call(s,'create',{kind:'vertexArray',shader_type:0}).value);},
      bindVertexArrayOES(value){const id=obj(s,value,'vertexArray');if(id!==invalid)resource(s,'bindVertexArray',{id});},
      deleteVertexArrayOES(value){const id=obj(s,value,'vertexArray');if(id!==invalid)call(s,'delete',{kind:'vertexArray',id});},
      isVertexArrayOES(value){return isObject(s,value,'vertexArray');},
    });
    if(canonical==='ANGLE_instanced_arrays')Object.assign(extension,{VERTEX_ATTRIB_ARRAY_DIVISOR_ANGLE:0x88FE,
      drawArraysInstancedANGLE(mode,first,count,instances){call(s,'command',{method:'drawArraysInstanced',args:{mode:uint(mode),first:int(first),count:int(count),instances:int(instances)}});},
      drawElementsInstancedANGLE(mode,count,type,offset,instances){const n=Math.trunc(Number(offset));if(!Number.isSafeInteger(n)||n<0||n>2147483647){error(s,0x0501);return;}call(s,'command',{method:'drawElementsInstanced',args:{mode:uint(mode),count:int(count),element_type:uint(type),offset:n,instances:int(instances)}});},
      vertexAttribDivisorANGLE(index,divisor){call(s,'command',{method:'vertexAttribDivisor',args:{index:uint(index),divisor:uint(divisor)}});},
    });
    for(const [name,value] of Object.entries(extension)){
      if(typeof value==='function')_markNative(value);
      else Object.defineProperty(extension,name,{value,writable:false,configurable:false,enumerable:true});
    }
    s.extensions.set(canonical,extension);return extension;
  });
  _webglHas=canvas=>canvases.has(canvas);
  _webglCreate=(canvas,type,options)=>{
    const version=type==='webgl2'?2:1;
    const existing=canvases.get(canvas);if(existing)return existing.version===version?existing.context:null;
    const offscreen=offscreens.get(canvas);
    const owner=offscreen||_canvasDOMOwner(canvas);
    if(!owner||(offscreen&&owner.epoch!==_canvasDocumentEpoch))return null;
    if(offscreen ? offscreen.context : _canvas2DContext(canvas))return null;
    if(options!=null&&typeof options!=='object'&&typeof options!=='function')throw new TypeError('WebGL context options must be a dictionary');
    const attributes={};
    for(const name of ['alpha','depth','stencil','antialias','premultipliedAlpha','preserveDrawingBuffer','failIfMajorPerformanceCaveat','desynchronized'])if(options&&options[name]!==undefined)attributes[name]=Boolean(options[name]);
    if(options&&options.powerPreference!==undefined){attributes.powerPreference=text(options.powerPreference);if(!['default','low-power','high-performance'].includes(attributes.powerPreference))throw new TypeError('Invalid WebGL powerPreference');}
    const frame=owner.frame;
    const size=canvasSize(canvas);
    const valid=size.width<=32767&&size.height<=32767&&size.width*size.height<=16777216;
    const result=valid ? __obscuraCore.ops.op_webgl_create(frame,owner.epoch,offscreen?null:owner.node,version,uint(size.width),uint(size.height),attributes)
      : {status:'failed',reason:'Canvas dimensions exceed the supported allocation'};
    if(result.status!=='ready'){
      queueContextTask(frame,()=>canvas.dispatchEvent(new WebGLContextEvent('webglcontextcreationerror',{statusMessage:result.reason})));
      return null;
    }
    const context=new (version===1?Context1:Context2)(secret);
    const s={canvas,context,frame,id:result.id,version,drawingColorSpace:'srgb',unpackColorSpace:'srgb',buffer:{width:size.width,height:size.height,format:result.attributes.alpha===false?0x8051:0x8058},generation:0,handles:new Map(),bindings:new Map(),defaultVertexArray:new Map(),defaultTransformFeedback:new Map(),activeTexture:0,extensions:new Map(),lost:false,lossQueued:false,lossErrorPending:false,restoreAllowed:false};
    // The native side holds only a weak callback. Retain it through the same
    // private state as the canvas/context, with no native-to-JS ownership cycle.
    s.lossCallback = lossCallback(new NativeWeakRef(s));
    if (!__obscuraCore.ops.op_webgl_watch_loss(frame,s.id,s.lossCallback)) {
      __obscuraCore.ops.op_webgl_release(frame,s.id);
      queueContextTask(frame,()=>canvas.dispatchEvent(new WebGLContextEvent('webglcontextcreationerror',{statusMessage:'WebGL context lifecycle registration failed'})));
      return null;
    }
    if (offscreen?.placeholder && placeholderOp(offscreen, {kind:'bind',id:offscreen.placeholder.id,context:s.id}).status !== 'ready') {
      __obscuraCore.ops.op_webgl_release(frame,s.id);
      queueContextTask(frame,()=>canvas.dispatchEvent(new WebGLContextEvent('webglcontextcreationerror',{statusMessage:'Placeholder context binding failed'})));
      return null;
    }
    contexts.set(context,s);canvases.set(canvas,s);
    return context;
  };
  _webglResize=canvas=>{const s=canvases.get(canvas);if(s){const size=canvasSize(canvas);if(size.width>32767||size.height>32767||size.width*size.height>16777216)call(s,'lose');else rememberDrawingBuffer(s,call(s,'resize',{width:uint(size.width),height:uint(size.height)}));}};
  _webglReadback=(canvas,sourceColor=false)=>{
    const s=canvases.get(canvas);if(!s)return null;
    const {width,height}=s.buffer;
    if(width>32767||height>32767||width*height>16777216)return null;
    const bytes=new Uint8Array(width*height*4);
    return call(s,sourceColor?'sourceReadback':'readback',undefined,bytes).value?{width,height,bytes,originClean:true,colorSpace:sourceColor?s.drawingColorSpace:'srgb'}:null;
  };
  function advanced(s,method,args){return retainedQuery(s,method,args,call(s,'advanced',{method,args}).value);}
  method('getAttachedShaders',1,(s,value)=>{const program=obj(s,value,'program',false);return program===invalid?null:advanced(s,'getAttachedShaders',{program});});
  method('getFramebufferAttachmentParameter',3,(s,target,attachment,name)=>advanced(s,'getFramebufferAttachmentParameter',{target:uint(target),attachment:uint(attachment),name:uint(name)}));
  for(const name of ['getVertexAttrib','getVertexAttribOffset'])method(name,2,(s,index,p)=>advanced(s,name,{index:uint(index),name:uint(p)}));
  for(let count=1;count<=4;count++)method('vertexAttrib'+count+'fv',2,(s,index,input)=>{
    const values=Array.from(input,Number);if(values.length<count){error(s,0x0501);return;}
    const args={index:uint(index)};for(let i=0;i<count;i++)args[['x','y','z','w'][i]]=values[i];
    call(s,'command',{method:'vertexAttrib'+count+'f',args});
  });
  for(const [name,convert,target] of [['vertexAttribI4iv',int,'vertexAttribI4i'],['vertexAttribI4uiv',uint,'vertexAttribI4ui']])method(name,2,(s,index,input)=>{
    const values=Array.from(input,convert);if(values.length<4){error(s,0x0501);return;}
    call(s,'command',{method:target,args:{index:uint(index),x:values[0],y:values[1],z:values[2],w:values[3]}});
  },2);
  for(const name of ['bindBufferBase','bindBufferRange'])method(name,name==='bindBufferBase'?3:5,(s,target,index,value,offset,size)=>{
    const id=obj(s,value,'buffer');if(id===invalid)return;
    const args={target:uint(target),index:uint(index),id};
    if(name==='bindBufferRange'){
      args.offset=Math.trunc(Number(offset));args.size=Math.trunc(Number(size));
      if(!Number.isSafeInteger(args.offset)||!Number.isSafeInteger(args.size)||args.offset>2147483647||args.size>2147483647||args.offset<0||args.size<0){error(s,0x0501);return;}
    }
    resource(s,name,args);
  },2);
  method('copyBufferSubData',5,(s,readTarget,writeTarget,readOffset,writeOffset,size)=>{
    const args={read_target:uint(readTarget),write_target:uint(writeTarget),read_offset:Math.trunc(Number(readOffset)),write_offset:Math.trunc(Number(writeOffset)),size:Math.trunc(Number(size))};
    if([args.read_offset,args.write_offset,args.size].some(v=>!Number.isSafeInteger(v)||v<0||v>2147483647)){error(s,0x0501);return;}
    resource(s,'copyBufferSubData',args);
  },2);
  method('drawBuffers',1,(s,values)=>resource(s,'drawBuffers',{buffers:Array.from(values,uint)}),2);
  method('invalidateFramebuffer',2,(s,target,values)=>resource(s,'invalidateFramebuffer',{target:uint(target),attachments:Array.from(values,uint)}),2);
  method('invalidateSubFramebuffer',6,(s,target,values,x,y,width,height)=>resource(s,'invalidateSubFramebuffer',{target:uint(target),attachments:Array.from(values,uint),x:int(x),y:int(y),width:int(width),height:int(height)}),2);
  method('transformFeedbackVaryings',3,(s,p,names,mode)=>{const program=obj(s,p,'program',false);if(program!==invalid)resource(s,'transformFeedbackVaryings',{program,varyings:Array.from(names,text),mode:uint(mode)});},2);
  method('getTransformFeedbackVarying',2,(s,p,index)=>{const program=obj(s,p,'program',false);return program===invalid?null:advanced(s,'getTransformFeedbackVarying',{program,index:uint(index)});},2);
  method('beginQuery',2,(s,target,value)=>{const id=obj(s,value,'query',false);if(id!==invalid)resource(s,'beginQuery',{target:uint(target),id});},2);
  method('getQuery',2,(s,target,name)=>advanced(s,'getQuery',{target:uint(target),name:uint(name)}),2);
  method('getQueryParameter',2,(s,value,name)=>{const id=obj(s,value,'query',false);return id===invalid?null:advanced(s,'getQueryParameter',{id,name:uint(name)});},2);
  for(const [methodName,convert] of [['samplerParameteri',int],['samplerParameterf',Number]])method(methodName,3,(s,value,name,param)=>{const id=obj(s,value,'sampler',false);if(id!==invalid)resource(s,methodName,{id,name:uint(name),value:convert(param)});},2);
  method('getSamplerParameter',2,(s,value,name)=>{const id=obj(s,value,'sampler',false);return id===invalid?null:advanced(s,'getSamplerParameter',{id,name:uint(name)});},2);
  method('fenceSync',2,(s,condition,flags)=>wrapper(s,'sync',advanced(s,'fenceSync',{condition:uint(condition),flags:uint(flags)})),2);
  method('deleteSync',1,(s,value)=>{const id=obj(s,value,'sync');if(id!==invalid)call(s,'delete',{kind:'sync',id});},2);
  method('isSync',1,(s,value)=>isObject(s,value,'sync'),2);
  for(const methodName of ['clientWaitSync','waitSync'])method(methodName,3,(s,value,flags,timeout)=>{
    const id=obj(s,value,'sync',false);if(id===invalid)return methodName==='clientWaitSync'?0x911D:undefined;
    const n=Number(timeout);
    if(!Number.isSafeInteger(n)||(methodName==='clientWaitSync'&&n<0)){error(s,0x0501);return methodName==='clientWaitSync'?0x911D:undefined;}
    const result=advanced(s,methodName,{id,flags:uint(flags),timeout:n});
    return methodName==='clientWaitSync'?(result===null?0x911D:result):undefined;
  },2);
  method('getSyncParameter',2,(s,value,name)=>{const id=obj(s,value,'sync',false);return id===invalid?null:advanced(s,'getSyncParameter',{id,name:uint(name)});},2);
  method('getIndexedParameter',2,(s,target,index)=>advanced(s,'getIndexedParameter',{target:uint(target),index:uint(index)}),2);
  method('getUniformIndices',2,(s,p,names)=>{const program=obj(s,p,'program',false);return program===invalid?null:advanced(s,'getUniformIndices',{program,names:Array.from(names,text)});},2);
  method('getActiveUniforms',3,(s,p,indices,name)=>{const program=obj(s,p,'program',false);return program===invalid?null:advanced(s,'getActiveUniforms',{program,indices:Array.from(indices,uint),name:uint(name)});},2);
  method('getUniformBlockIndex',2,(s,p,name)=>{const program=obj(s,p,'program',false);return program===invalid?0:advanced(s,'getUniformBlockIndex',{program,name:text(name)});},2);
  method('getFragDataLocation',2,(s,p,name)=>{const program=obj(s,p,'program',false);return program===invalid?-1:advanced(s,'getFragDataLocation',{program,name:text(name)});},2);
  method('getActiveUniformBlockName',2,(s,p,index)=>{const program=obj(s,p,'program',false);return program===invalid?null:advanced(s,'getActiveUniformBlockName',{program,index:uint(index)});},2);
  method('getActiveUniformBlockParameter',3,(s,p,index,name)=>{const program=obj(s,p,'program',false);return program===invalid?null:advanced(s,'getActiveUniformBlockParameter',{program,index:uint(index),name:uint(name)});},2);
  method('uniformBlockBinding',3,(s,p,index,binding)=>{const program=obj(s,p,'program',false);if(program!==invalid)resource(s,'uniformBlockBinding',{program,index:uint(index),binding:uint(binding)});},2);
  method('getInternalformatParameter',3,(s,target,format,name)=>advanced(s,'getInternalformatParameter',{target:uint(target),format:uint(format),name:uint(name)}),2);
  method('drawRangeElements',6,(s,mode,start,end,count,type,offset)=>{
    const n=Math.trunc(Number(offset));if(!Number.isSafeInteger(n)||n<0||n>2147483647){error(s,0x0501);return;}
    advanced(s,'drawRangeElements',{mode:uint(mode),start:uint(start),end:uint(end),count:int(count),data_type:uint(type),offset:n});
  },2);
  for(const [name,request,convert] of [['clearBufferfv','clearBufferFloat',Number],['clearBufferiv','clearBufferInt',int],['clearBufferuiv','clearBufferUint',uint]])method(name,3,(s,buffer,index,input,offset=0)=>{
    const values=Array.from(input,convert),start=uint(offset);if(start>values.length){error(s,0x0501);return;}
    advanced(s,request,{buffer:uint(buffer),index:int(index),values:values.slice(start)});
  },2);
  method('clearBufferfi',4,(s,buffer,index,depth,stencil)=>{advanced(s,'clearBufferDepthStencil',{buffer:uint(buffer),index:int(index),depth:Number(depth),stencil:int(stencil)});},2);

  // Scalar methods and normative interface constants are installed below.
  const scalarMethods = [
    {"name": "activeTexture", "args": [["texture", "u32"]], "version": 1},
    {"name": "blendColor", "args": [["red", "f32"], ["green", "f32"], ["blue", "f32"], ["alpha", "f32"]], "version": 1},
    {"name": "blendEquation", "args": [["mode", "u32"]], "version": 1},
    {"name": "blendEquationSeparate", "args": [["rgb", "u32"], ["alpha", "u32"]], "version": 1},
    {"name": "blendFunc", "args": [["src", "u32"], ["dst", "u32"]], "version": 1},
    {"name": "blendFuncSeparate", "args": [["src_rgb", "u32"], ["dst_rgb", "u32"], ["src_alpha", "u32"], ["dst_alpha", "u32"]], "version": 1},
    {"name": "clear", "args": [["mask", "u32"]], "version": 1},
    {"name": "clearColor", "args": [["red", "f32"], ["green", "f32"], ["blue", "f32"], ["alpha", "f32"]], "version": 1},
    {"name": "clearDepth", "args": [["depth", "f32"]], "version": 1},
    {"name": "clearStencil", "args": [["stencil", "i32"]], "version": 1},
    {"name": "colorMask", "args": [["red", "bool"], ["green", "bool"], ["blue", "bool"], ["alpha", "bool"]], "version": 1},
    {"name": "cullFace", "args": [["mode", "u32"]], "version": 1},
    {"name": "depthFunc", "args": [["func", "u32"]], "version": 1},
    {"name": "depthMask", "args": [["flag", "bool"]], "version": 1},
    {"name": "depthRange", "args": [["near", "f32"], ["far", "f32"]], "version": 1},
    {"name": "disable", "args": [["cap", "u32"]], "version": 1},
    {"name": "enable", "args": [["cap", "u32"]], "version": 1},
    {"name": "finish", "args": [], "version": 1},
    {"name": "flush", "args": [], "version": 1},
    {"name": "frontFace", "args": [["mode", "u32"]], "version": 1},
    {"name": "hint", "args": [["target", "u32"], ["mode", "u32"]], "version": 1},
    {"name": "lineWidth", "args": [["width", "f32"]], "version": 1},
    {"name": "polygonOffset", "args": [["factor", "f32"], ["units", "f32"]], "version": 1},
    {"name": "sampleCoverage", "args": [["value", "f32"], ["invert", "bool"]], "version": 1},
    {"name": "scissor", "args": [["x", "i32"], ["y", "i32"], ["width", "i32"], ["height", "i32"]], "version": 1},
    {"name": "stencilFunc", "args": [["func", "u32"], ["reference", "i32"], ["mask", "u32"]], "version": 1},
    {"name": "stencilFuncSeparate", "args": [["face", "u32"], ["func", "u32"], ["reference", "i32"], ["mask", "u32"]], "version": 1},
    {"name": "stencilMask", "args": [["mask", "u32"]], "version": 1},
    {"name": "stencilMaskSeparate", "args": [["face", "u32"], ["mask", "u32"]], "version": 1},
    {"name": "stencilOp", "args": [["fail", "u32"], ["zfail", "u32"], ["zpass", "u32"]], "version": 1},
    {"name": "stencilOpSeparate", "args": [["face", "u32"], ["fail", "u32"], ["zfail", "u32"], ["zpass", "u32"]], "version": 1},
    {"name": "viewport", "args": [["x", "i32"], ["y", "i32"], ["width", "i32"], ["height", "i32"]], "version": 1},
    {"name": "drawArrays", "args": [["mode", "u32"], ["first", "i32"], ["count", "i32"]], "version": 1},
    {"name": "drawElements", "args": [["mode", "u32"], ["count", "i32"], ["element_type", "u32"], ["offset", "i32"]], "version": 1},
    {"name": "disableVertexAttribArray", "args": [["index", "u32"]], "version": 1},
    {"name": "enableVertexAttribArray", "args": [["index", "u32"]], "version": 1},
    {"name": "vertexAttrib1f", "args": [["index", "u32"], ["x", "f32"]], "version": 1},
    {"name": "vertexAttrib2f", "args": [["index", "u32"], ["x", "f32"], ["y", "f32"]], "version": 1},
    {"name": "vertexAttrib3f", "args": [["index", "u32"], ["x", "f32"], ["y", "f32"], ["z", "f32"]], "version": 1},
    {"name": "vertexAttrib4f", "args": [["index", "u32"], ["x", "f32"], ["y", "f32"], ["z", "f32"], ["w", "f32"]], "version": 1},
    {"name": "vertexAttribPointer", "args": [["index", "u32"], ["size", "i32"], ["data_type", "u32"], ["normalized", "bool"], ["stride", "i32"], ["offset", "i32"]], "version": 1},
    {"name": "generateMipmap", "args": [["target", "u32"]], "version": 1},
    {"name": "texParameterf", "args": [["target", "u32"], ["name", "u32"], ["value", "f32"]], "version": 1},
    {"name": "texParameteri", "args": [["target", "u32"], ["name", "u32"], ["value", "i32"]], "version": 1},
    {"name": "renderbufferStorage", "args": [["target", "u32"], ["format", "u32"], ["width", "i32"], ["height", "i32"]], "version": 1},
    {"name": "copyTexImage2D", "args": [["target", "u32"], ["level", "i32"], ["format", "u32"], ["x", "i32"], ["y", "i32"], ["width", "i32"], ["height", "i32"], ["border", "i32"]], "version": 1},
    {"name": "copyTexSubImage2D", "args": [["target", "u32"], ["level", "i32"], ["xoffset", "i32"], ["yoffset", "i32"], ["x", "i32"], ["y", "i32"], ["width", "i32"], ["height", "i32"]], "version": 1},
    {"name": "drawArraysInstanced", "args": [["mode", "u32"], ["first", "i32"], ["count", "i32"], ["instances", "i32"]], "version": 2},
    {"name": "drawElementsInstanced", "args": [["mode", "u32"], ["count", "i32"], ["element_type", "u32"], ["offset", "i32"], ["instances", "i32"]], "version": 2},
    {"name": "vertexAttribDivisor", "args": [["index", "u32"], ["divisor", "u32"]], "version": 2},
    {"name": "vertexAttribIPointer", "args": [["index", "u32"], ["size", "i32"], ["data_type", "u32"], ["stride", "i32"], ["offset", "i32"]], "version": 2},
    {"name": "vertexAttribI4i", "args": [["index", "u32"], ["x", "i32"], ["y", "i32"], ["z", "i32"], ["w", "i32"]], "version": 2},
    {"name": "vertexAttribI4ui", "args": [["index", "u32"], ["x", "u32"], ["y", "u32"], ["z", "u32"], ["w", "u32"]], "version": 2},
    {"name": "readBuffer", "args": [["source", "u32"]], "version": 2},
    {"name": "renderbufferStorageMultisample", "args": [["target", "u32"], ["samples", "i32"], ["format", "u32"], ["width", "i32"], ["height", "i32"]], "version": 2},
    {"name": "texStorage2D", "args": [["target", "u32"], ["levels", "i32"], ["format", "u32"], ["width", "i32"], ["height", "i32"]], "version": 2},
    {"name": "texStorage3D", "args": [["target", "u32"], ["levels", "i32"], ["format", "u32"], ["width", "i32"], ["height", "i32"], ["depth", "i32"]], "version": 2},
    {"name": "copyTexSubImage3D", "args": [["target", "u32"], ["level", "i32"], ["xoffset", "i32"], ["yoffset", "i32"], ["zoffset", "i32"], ["x", "i32"], ["y", "i32"], ["width", "i32"], ["height", "i32"]], "version": 2},
    {"name": "blitFramebuffer", "args": [["sx0", "i32"], ["sy0", "i32"], ["sx1", "i32"], ["sy1", "i32"], ["dx0", "i32"], ["dy0", "i32"], ["dx1", "i32"], ["dy1", "i32"], ["mask", "u32"], ["filter", "u32"]], "version": 2},
    {"name": "beginTransformFeedback", "args": [["mode", "u32"]], "version": 2},
    {"name": "endTransformFeedback", "args": [], "version": 2},
    {"name": "pauseTransformFeedback", "args": [], "version": 2},
    {"name": "resumeTransformFeedback", "args": [], "version": 2},
    {"name": "endQuery", "args": [["target", "u32"]], "version": 2},
  ];
  for(const entry of scalarMethods)method(entry.name,entry.args.length,(s,...values)=>{
    const args={};for(let i=0;i<entry.args.length;i++){const [name,type]=entry.args[i];if(name==='offset'){const n=Math.trunc(Number(values[i]));if(!Number.isSafeInteger(n)||n<0||n>2147483647){error(s,0x0501);return;}args[name]=n;continue;}args[name]=type==='bool'?Boolean(values[i]):type==='u32'?uint(values[i]):type==='i32'?int(values[i]):Number(values[i]);}
    call(s,'command',{method:entry.name,args});
  },entry.version);

// Copyright (c) 2026 The Khronos Group Inc.
//
// Permission is hereby granted, free of charge, to any person obtaining a
// copy of this software and/or associated documentation files (the
// "Materials"), to deal in the Materials without restriction, including
// without limitation the rights to use, copy, modify, merge, publish,
// distribute, sublicense, and/or sell copies of the Materials, and to
// permit persons to whom the Materials are furnished to do so, subject to
// the following conditions:
//
// The above copyright notice and this permission notice shall be included
// in all copies or substantial portions of the Materials.
//
// THE MATERIALS ARE PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND,
// EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
// MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT.
// IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY
// CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT,
// TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE
// MATERIALS OR THE USE OR OTHER DEALINGS IN THE MATERIALS.
// Constants derived from Khronos WebGL IDL revision 714857a28445e8f5d8d6ae1c78498578009534d8.
  for(const ctor of [Context1,Context2])for(const [name,value] of Object.entries({
    "DEPTH_BUFFER_BIT": 256,
    "STENCIL_BUFFER_BIT": 1024,
    "COLOR_BUFFER_BIT": 16384,
    "POINTS": 0,
    "LINES": 1,
    "LINE_LOOP": 2,
    "LINE_STRIP": 3,
    "TRIANGLES": 4,
    "TRIANGLE_STRIP": 5,
    "TRIANGLE_FAN": 6,
    "ZERO": 0,
    "ONE": 1,
    "SRC_COLOR": 768,
    "ONE_MINUS_SRC_COLOR": 769,
    "SRC_ALPHA": 770,
    "ONE_MINUS_SRC_ALPHA": 771,
    "DST_ALPHA": 772,
    "ONE_MINUS_DST_ALPHA": 773,
    "DST_COLOR": 774,
    "ONE_MINUS_DST_COLOR": 775,
    "SRC_ALPHA_SATURATE": 776,
    "FUNC_ADD": 32774,
    "BLEND_EQUATION": 32777,
    "BLEND_EQUATION_RGB": 32777,
    "BLEND_EQUATION_ALPHA": 34877,
    "FUNC_SUBTRACT": 32778,
    "FUNC_REVERSE_SUBTRACT": 32779,
    "BLEND_DST_RGB": 32968,
    "BLEND_SRC_RGB": 32969,
    "BLEND_DST_ALPHA": 32970,
    "BLEND_SRC_ALPHA": 32971,
    "CONSTANT_COLOR": 32769,
    "ONE_MINUS_CONSTANT_COLOR": 32770,
    "CONSTANT_ALPHA": 32771,
    "ONE_MINUS_CONSTANT_ALPHA": 32772,
    "BLEND_COLOR": 32773,
    "ARRAY_BUFFER": 34962,
    "ELEMENT_ARRAY_BUFFER": 34963,
    "ARRAY_BUFFER_BINDING": 34964,
    "ELEMENT_ARRAY_BUFFER_BINDING": 34965,
    "STREAM_DRAW": 35040,
    "STATIC_DRAW": 35044,
    "DYNAMIC_DRAW": 35048,
    "BUFFER_SIZE": 34660,
    "BUFFER_USAGE": 34661,
    "CURRENT_VERTEX_ATTRIB": 34342,
    "FRONT": 1028,
    "BACK": 1029,
    "FRONT_AND_BACK": 1032,
    "CULL_FACE": 2884,
    "BLEND": 3042,
    "DITHER": 3024,
    "STENCIL_TEST": 2960,
    "DEPTH_TEST": 2929,
    "SCISSOR_TEST": 3089,
    "POLYGON_OFFSET_FILL": 32823,
    "SAMPLE_ALPHA_TO_COVERAGE": 32926,
    "SAMPLE_COVERAGE": 32928,
    "NO_ERROR": 0,
    "INVALID_ENUM": 1280,
    "INVALID_VALUE": 1281,
    "INVALID_OPERATION": 1282,
    "OUT_OF_MEMORY": 1285,
    "CW": 2304,
    "CCW": 2305,
    "LINE_WIDTH": 2849,
    "ALIASED_POINT_SIZE_RANGE": 33901,
    "ALIASED_LINE_WIDTH_RANGE": 33902,
    "CULL_FACE_MODE": 2885,
    "FRONT_FACE": 2886,
    "DEPTH_RANGE": 2928,
    "DEPTH_WRITEMASK": 2930,
    "DEPTH_CLEAR_VALUE": 2931,
    "DEPTH_FUNC": 2932,
    "STENCIL_CLEAR_VALUE": 2961,
    "STENCIL_FUNC": 2962,
    "STENCIL_FAIL": 2964,
    "STENCIL_PASS_DEPTH_FAIL": 2965,
    "STENCIL_PASS_DEPTH_PASS": 2966,
    "STENCIL_REF": 2967,
    "STENCIL_VALUE_MASK": 2963,
    "STENCIL_WRITEMASK": 2968,
    "STENCIL_BACK_FUNC": 34816,
    "STENCIL_BACK_FAIL": 34817,
    "STENCIL_BACK_PASS_DEPTH_FAIL": 34818,
    "STENCIL_BACK_PASS_DEPTH_PASS": 34819,
    "STENCIL_BACK_REF": 36003,
    "STENCIL_BACK_VALUE_MASK": 36004,
    "STENCIL_BACK_WRITEMASK": 36005,
    "VIEWPORT": 2978,
    "SCISSOR_BOX": 3088,
    "COLOR_CLEAR_VALUE": 3106,
    "COLOR_WRITEMASK": 3107,
    "UNPACK_ALIGNMENT": 3317,
    "PACK_ALIGNMENT": 3333,
    "MAX_TEXTURE_SIZE": 3379,
    "MAX_VIEWPORT_DIMS": 3386,
    "SUBPIXEL_BITS": 3408,
    "RED_BITS": 3410,
    "GREEN_BITS": 3411,
    "BLUE_BITS": 3412,
    "ALPHA_BITS": 3413,
    "DEPTH_BITS": 3414,
    "STENCIL_BITS": 3415,
    "POLYGON_OFFSET_UNITS": 10752,
    "POLYGON_OFFSET_FACTOR": 32824,
    "TEXTURE_BINDING_2D": 32873,
    "SAMPLE_BUFFERS": 32936,
    "SAMPLES": 32937,
    "SAMPLE_COVERAGE_VALUE": 32938,
    "SAMPLE_COVERAGE_INVERT": 32939,
    "COMPRESSED_TEXTURE_FORMATS": 34467,
    "DONT_CARE": 4352,
    "FASTEST": 4353,
    "NICEST": 4354,
    "GENERATE_MIPMAP_HINT": 33170,
    "BYTE": 5120,
    "UNSIGNED_BYTE": 5121,
    "SHORT": 5122,
    "UNSIGNED_SHORT": 5123,
    "INT": 5124,
    "UNSIGNED_INT": 5125,
    "FLOAT": 5126,
    "DEPTH_COMPONENT": 6402,
    "ALPHA": 6406,
    "RGB": 6407,
    "RGBA": 6408,
    "LUMINANCE": 6409,
    "LUMINANCE_ALPHA": 6410,
    "UNSIGNED_SHORT_4_4_4_4": 32819,
    "UNSIGNED_SHORT_5_5_5_1": 32820,
    "UNSIGNED_SHORT_5_6_5": 33635,
    "FRAGMENT_SHADER": 35632,
    "VERTEX_SHADER": 35633,
    "MAX_VERTEX_ATTRIBS": 34921,
    "MAX_VERTEX_UNIFORM_VECTORS": 36347,
    "MAX_VARYING_VECTORS": 36348,
    "MAX_COMBINED_TEXTURE_IMAGE_UNITS": 35661,
    "MAX_VERTEX_TEXTURE_IMAGE_UNITS": 35660,
    "MAX_TEXTURE_IMAGE_UNITS": 34930,
    "MAX_FRAGMENT_UNIFORM_VECTORS": 36349,
    "SHADER_TYPE": 35663,
    "DELETE_STATUS": 35712,
    "LINK_STATUS": 35714,
    "VALIDATE_STATUS": 35715,
    "ATTACHED_SHADERS": 35717,
    "ACTIVE_UNIFORMS": 35718,
    "ACTIVE_ATTRIBUTES": 35721,
    "SHADING_LANGUAGE_VERSION": 35724,
    "CURRENT_PROGRAM": 35725,
    "NEVER": 512,
    "LESS": 513,
    "EQUAL": 514,
    "LEQUAL": 515,
    "GREATER": 516,
    "NOTEQUAL": 517,
    "GEQUAL": 518,
    "ALWAYS": 519,
    "KEEP": 7680,
    "REPLACE": 7681,
    "INCR": 7682,
    "DECR": 7683,
    "INVERT": 5386,
    "INCR_WRAP": 34055,
    "DECR_WRAP": 34056,
    "VENDOR": 7936,
    "RENDERER": 7937,
    "VERSION": 7938,
    "NEAREST": 9728,
    "LINEAR": 9729,
    "NEAREST_MIPMAP_NEAREST": 9984,
    "LINEAR_MIPMAP_NEAREST": 9985,
    "NEAREST_MIPMAP_LINEAR": 9986,
    "LINEAR_MIPMAP_LINEAR": 9987,
    "TEXTURE_MAG_FILTER": 10240,
    "TEXTURE_MIN_FILTER": 10241,
    "TEXTURE_WRAP_S": 10242,
    "TEXTURE_WRAP_T": 10243,
    "TEXTURE_2D": 3553,
    "TEXTURE": 5890,
    "TEXTURE_CUBE_MAP": 34067,
    "TEXTURE_BINDING_CUBE_MAP": 34068,
    "TEXTURE_CUBE_MAP_POSITIVE_X": 34069,
    "TEXTURE_CUBE_MAP_NEGATIVE_X": 34070,
    "TEXTURE_CUBE_MAP_POSITIVE_Y": 34071,
    "TEXTURE_CUBE_MAP_NEGATIVE_Y": 34072,
    "TEXTURE_CUBE_MAP_POSITIVE_Z": 34073,
    "TEXTURE_CUBE_MAP_NEGATIVE_Z": 34074,
    "MAX_CUBE_MAP_TEXTURE_SIZE": 34076,
    "TEXTURE0": 33984,
    "TEXTURE1": 33985,
    "TEXTURE2": 33986,
    "TEXTURE3": 33987,
    "TEXTURE4": 33988,
    "TEXTURE5": 33989,
    "TEXTURE6": 33990,
    "TEXTURE7": 33991,
    "TEXTURE8": 33992,
    "TEXTURE9": 33993,
    "TEXTURE10": 33994,
    "TEXTURE11": 33995,
    "TEXTURE12": 33996,
    "TEXTURE13": 33997,
    "TEXTURE14": 33998,
    "TEXTURE15": 33999,
    "TEXTURE16": 34000,
    "TEXTURE17": 34001,
    "TEXTURE18": 34002,
    "TEXTURE19": 34003,
    "TEXTURE20": 34004,
    "TEXTURE21": 34005,
    "TEXTURE22": 34006,
    "TEXTURE23": 34007,
    "TEXTURE24": 34008,
    "TEXTURE25": 34009,
    "TEXTURE26": 34010,
    "TEXTURE27": 34011,
    "TEXTURE28": 34012,
    "TEXTURE29": 34013,
    "TEXTURE30": 34014,
    "TEXTURE31": 34015,
    "ACTIVE_TEXTURE": 34016,
    "REPEAT": 10497,
    "CLAMP_TO_EDGE": 33071,
    "MIRRORED_REPEAT": 33648,
    "FLOAT_VEC2": 35664,
    "FLOAT_VEC3": 35665,
    "FLOAT_VEC4": 35666,
    "INT_VEC2": 35667,
    "INT_VEC3": 35668,
    "INT_VEC4": 35669,
    "BOOL": 35670,
    "BOOL_VEC2": 35671,
    "BOOL_VEC3": 35672,
    "BOOL_VEC4": 35673,
    "FLOAT_MAT2": 35674,
    "FLOAT_MAT3": 35675,
    "FLOAT_MAT4": 35676,
    "SAMPLER_2D": 35678,
    "SAMPLER_CUBE": 35680,
    "VERTEX_ATTRIB_ARRAY_ENABLED": 34338,
    "VERTEX_ATTRIB_ARRAY_SIZE": 34339,
    "VERTEX_ATTRIB_ARRAY_STRIDE": 34340,
    "VERTEX_ATTRIB_ARRAY_TYPE": 34341,
    "VERTEX_ATTRIB_ARRAY_NORMALIZED": 34922,
    "VERTEX_ATTRIB_ARRAY_POINTER": 34373,
    "VERTEX_ATTRIB_ARRAY_BUFFER_BINDING": 34975,
    "IMPLEMENTATION_COLOR_READ_TYPE": 35738,
    "IMPLEMENTATION_COLOR_READ_FORMAT": 35739,
    "COMPILE_STATUS": 35713,
    "LOW_FLOAT": 36336,
    "MEDIUM_FLOAT": 36337,
    "HIGH_FLOAT": 36338,
    "LOW_INT": 36339,
    "MEDIUM_INT": 36340,
    "HIGH_INT": 36341,
    "FRAMEBUFFER": 36160,
    "RENDERBUFFER": 36161,
    "RGBA4": 32854,
    "RGB5_A1": 32855,
    "RGBA8": 32856,
    "RGB565": 36194,
    "DEPTH_COMPONENT16": 33189,
    "STENCIL_INDEX8": 36168,
    "DEPTH_STENCIL": 34041,
    "RENDERBUFFER_WIDTH": 36162,
    "RENDERBUFFER_HEIGHT": 36163,
    "RENDERBUFFER_INTERNAL_FORMAT": 36164,
    "RENDERBUFFER_RED_SIZE": 36176,
    "RENDERBUFFER_GREEN_SIZE": 36177,
    "RENDERBUFFER_BLUE_SIZE": 36178,
    "RENDERBUFFER_ALPHA_SIZE": 36179,
    "RENDERBUFFER_DEPTH_SIZE": 36180,
    "RENDERBUFFER_STENCIL_SIZE": 36181,
    "FRAMEBUFFER_ATTACHMENT_OBJECT_TYPE": 36048,
    "FRAMEBUFFER_ATTACHMENT_OBJECT_NAME": 36049,
    "FRAMEBUFFER_ATTACHMENT_TEXTURE_LEVEL": 36050,
    "FRAMEBUFFER_ATTACHMENT_TEXTURE_CUBE_MAP_FACE": 36051,
    "COLOR_ATTACHMENT0": 36064,
    "DEPTH_ATTACHMENT": 36096,
    "STENCIL_ATTACHMENT": 36128,
    "DEPTH_STENCIL_ATTACHMENT": 33306,
    "NONE": 0,
    "FRAMEBUFFER_COMPLETE": 36053,
    "FRAMEBUFFER_INCOMPLETE_ATTACHMENT": 36054,
    "FRAMEBUFFER_INCOMPLETE_MISSING_ATTACHMENT": 36055,
    "FRAMEBUFFER_INCOMPLETE_DIMENSIONS": 36057,
    "FRAMEBUFFER_UNSUPPORTED": 36061,
    "FRAMEBUFFER_BINDING": 36006,
    "RENDERBUFFER_BINDING": 36007,
    "MAX_RENDERBUFFER_SIZE": 34024,
    "INVALID_FRAMEBUFFER_OPERATION": 1286,
    "UNPACK_FLIP_Y_WEBGL": 37440,
    "UNPACK_PREMULTIPLY_ALPHA_WEBGL": 37441,
    "CONTEXT_LOST_WEBGL": 37442,
    "UNPACK_COLORSPACE_CONVERSION_WEBGL": 37443,
    "BROWSER_DEFAULT_WEBGL": 37444,
  })){
    Object.defineProperty(ctor,name,{value,enumerable:true});Object.defineProperty(ctor.prototype,name,{value,enumerable:true});
  }
  for(const ctor of [Context2])for(const [name,value] of Object.entries({
    "READ_BUFFER": 3074,
    "UNPACK_ROW_LENGTH": 3314,
    "UNPACK_SKIP_ROWS": 3315,
    "UNPACK_SKIP_PIXELS": 3316,
    "PACK_ROW_LENGTH": 3330,
    "PACK_SKIP_ROWS": 3331,
    "PACK_SKIP_PIXELS": 3332,
    "COLOR": 6144,
    "DEPTH": 6145,
    "STENCIL": 6146,
    "RED": 6403,
    "RGB8": 32849,
    "RGB10_A2": 32857,
    "TEXTURE_BINDING_3D": 32874,
    "UNPACK_SKIP_IMAGES": 32877,
    "UNPACK_IMAGE_HEIGHT": 32878,
    "TEXTURE_3D": 32879,
    "TEXTURE_WRAP_R": 32882,
    "MAX_3D_TEXTURE_SIZE": 32883,
    "UNSIGNED_INT_2_10_10_10_REV": 33640,
    "MAX_ELEMENTS_VERTICES": 33000,
    "MAX_ELEMENTS_INDICES": 33001,
    "TEXTURE_MIN_LOD": 33082,
    "TEXTURE_MAX_LOD": 33083,
    "TEXTURE_BASE_LEVEL": 33084,
    "TEXTURE_MAX_LEVEL": 33085,
    "MIN": 32775,
    "MAX": 32776,
    "DEPTH_COMPONENT24": 33190,
    "MAX_TEXTURE_LOD_BIAS": 34045,
    "TEXTURE_COMPARE_MODE": 34892,
    "TEXTURE_COMPARE_FUNC": 34893,
    "CURRENT_QUERY": 34917,
    "QUERY_RESULT": 34918,
    "QUERY_RESULT_AVAILABLE": 34919,
    "STREAM_READ": 35041,
    "STREAM_COPY": 35042,
    "STATIC_READ": 35045,
    "STATIC_COPY": 35046,
    "DYNAMIC_READ": 35049,
    "DYNAMIC_COPY": 35050,
    "MAX_DRAW_BUFFERS": 34852,
    "DRAW_BUFFER0": 34853,
    "DRAW_BUFFER1": 34854,
    "DRAW_BUFFER2": 34855,
    "DRAW_BUFFER3": 34856,
    "DRAW_BUFFER4": 34857,
    "DRAW_BUFFER5": 34858,
    "DRAW_BUFFER6": 34859,
    "DRAW_BUFFER7": 34860,
    "DRAW_BUFFER8": 34861,
    "DRAW_BUFFER9": 34862,
    "DRAW_BUFFER10": 34863,
    "DRAW_BUFFER11": 34864,
    "DRAW_BUFFER12": 34865,
    "DRAW_BUFFER13": 34866,
    "DRAW_BUFFER14": 34867,
    "DRAW_BUFFER15": 34868,
    "MAX_FRAGMENT_UNIFORM_COMPONENTS": 35657,
    "MAX_VERTEX_UNIFORM_COMPONENTS": 35658,
    "SAMPLER_3D": 35679,
    "SAMPLER_2D_SHADOW": 35682,
    "FRAGMENT_SHADER_DERIVATIVE_HINT": 35723,
    "PIXEL_PACK_BUFFER": 35051,
    "PIXEL_UNPACK_BUFFER": 35052,
    "PIXEL_PACK_BUFFER_BINDING": 35053,
    "PIXEL_UNPACK_BUFFER_BINDING": 35055,
    "FLOAT_MAT2x3": 35685,
    "FLOAT_MAT2x4": 35686,
    "FLOAT_MAT3x2": 35687,
    "FLOAT_MAT3x4": 35688,
    "FLOAT_MAT4x2": 35689,
    "FLOAT_MAT4x3": 35690,
    "SRGB": 35904,
    "SRGB8": 35905,
    "SRGB8_ALPHA8": 35907,
    "COMPARE_REF_TO_TEXTURE": 34894,
    "RGBA32F": 34836,
    "RGB32F": 34837,
    "RGBA16F": 34842,
    "RGB16F": 34843,
    "VERTEX_ATTRIB_ARRAY_INTEGER": 35069,
    "MAX_ARRAY_TEXTURE_LAYERS": 35071,
    "MIN_PROGRAM_TEXEL_OFFSET": 35076,
    "MAX_PROGRAM_TEXEL_OFFSET": 35077,
    "MAX_VARYING_COMPONENTS": 35659,
    "TEXTURE_2D_ARRAY": 35866,
    "TEXTURE_BINDING_2D_ARRAY": 35869,
    "R11F_G11F_B10F": 35898,
    "UNSIGNED_INT_10F_11F_11F_REV": 35899,
    "RGB9_E5": 35901,
    "UNSIGNED_INT_5_9_9_9_REV": 35902,
    "TRANSFORM_FEEDBACK_BUFFER_MODE": 35967,
    "MAX_TRANSFORM_FEEDBACK_SEPARATE_COMPONENTS": 35968,
    "TRANSFORM_FEEDBACK_VARYINGS": 35971,
    "TRANSFORM_FEEDBACK_BUFFER_START": 35972,
    "TRANSFORM_FEEDBACK_BUFFER_SIZE": 35973,
    "TRANSFORM_FEEDBACK_PRIMITIVES_WRITTEN": 35976,
    "RASTERIZER_DISCARD": 35977,
    "MAX_TRANSFORM_FEEDBACK_INTERLEAVED_COMPONENTS": 35978,
    "MAX_TRANSFORM_FEEDBACK_SEPARATE_ATTRIBS": 35979,
    "INTERLEAVED_ATTRIBS": 35980,
    "SEPARATE_ATTRIBS": 35981,
    "TRANSFORM_FEEDBACK_BUFFER": 35982,
    "TRANSFORM_FEEDBACK_BUFFER_BINDING": 35983,
    "RGBA32UI": 36208,
    "RGB32UI": 36209,
    "RGBA16UI": 36214,
    "RGB16UI": 36215,
    "RGBA8UI": 36220,
    "RGB8UI": 36221,
    "RGBA32I": 36226,
    "RGB32I": 36227,
    "RGBA16I": 36232,
    "RGB16I": 36233,
    "RGBA8I": 36238,
    "RGB8I": 36239,
    "RED_INTEGER": 36244,
    "RGB_INTEGER": 36248,
    "RGBA_INTEGER": 36249,
    "SAMPLER_2D_ARRAY": 36289,
    "SAMPLER_2D_ARRAY_SHADOW": 36292,
    "SAMPLER_CUBE_SHADOW": 36293,
    "UNSIGNED_INT_VEC2": 36294,
    "UNSIGNED_INT_VEC3": 36295,
    "UNSIGNED_INT_VEC4": 36296,
    "INT_SAMPLER_2D": 36298,
    "INT_SAMPLER_3D": 36299,
    "INT_SAMPLER_CUBE": 36300,
    "INT_SAMPLER_2D_ARRAY": 36303,
    "UNSIGNED_INT_SAMPLER_2D": 36306,
    "UNSIGNED_INT_SAMPLER_3D": 36307,
    "UNSIGNED_INT_SAMPLER_CUBE": 36308,
    "UNSIGNED_INT_SAMPLER_2D_ARRAY": 36311,
    "DEPTH_COMPONENT32F": 36012,
    "DEPTH32F_STENCIL8": 36013,
    "FLOAT_32_UNSIGNED_INT_24_8_REV": 36269,
    "FRAMEBUFFER_ATTACHMENT_COLOR_ENCODING": 33296,
    "FRAMEBUFFER_ATTACHMENT_COMPONENT_TYPE": 33297,
    "FRAMEBUFFER_ATTACHMENT_RED_SIZE": 33298,
    "FRAMEBUFFER_ATTACHMENT_GREEN_SIZE": 33299,
    "FRAMEBUFFER_ATTACHMENT_BLUE_SIZE": 33300,
    "FRAMEBUFFER_ATTACHMENT_ALPHA_SIZE": 33301,
    "FRAMEBUFFER_ATTACHMENT_DEPTH_SIZE": 33302,
    "FRAMEBUFFER_ATTACHMENT_STENCIL_SIZE": 33303,
    "FRAMEBUFFER_DEFAULT": 33304,
    "UNSIGNED_INT_24_8": 34042,
    "DEPTH24_STENCIL8": 35056,
    "UNSIGNED_NORMALIZED": 35863,
    "DRAW_FRAMEBUFFER_BINDING": 36006,
    "READ_FRAMEBUFFER": 36008,
    "DRAW_FRAMEBUFFER": 36009,
    "READ_FRAMEBUFFER_BINDING": 36010,
    "RENDERBUFFER_SAMPLES": 36011,
    "FRAMEBUFFER_ATTACHMENT_TEXTURE_LAYER": 36052,
    "MAX_COLOR_ATTACHMENTS": 36063,
    "COLOR_ATTACHMENT1": 36065,
    "COLOR_ATTACHMENT2": 36066,
    "COLOR_ATTACHMENT3": 36067,
    "COLOR_ATTACHMENT4": 36068,
    "COLOR_ATTACHMENT5": 36069,
    "COLOR_ATTACHMENT6": 36070,
    "COLOR_ATTACHMENT7": 36071,
    "COLOR_ATTACHMENT8": 36072,
    "COLOR_ATTACHMENT9": 36073,
    "COLOR_ATTACHMENT10": 36074,
    "COLOR_ATTACHMENT11": 36075,
    "COLOR_ATTACHMENT12": 36076,
    "COLOR_ATTACHMENT13": 36077,
    "COLOR_ATTACHMENT14": 36078,
    "COLOR_ATTACHMENT15": 36079,
    "FRAMEBUFFER_INCOMPLETE_MULTISAMPLE": 36182,
    "MAX_SAMPLES": 36183,
    "HALF_FLOAT": 5131,
    "RG": 33319,
    "RG_INTEGER": 33320,
    "R8": 33321,
    "RG8": 33323,
    "R16F": 33325,
    "R32F": 33326,
    "RG16F": 33327,
    "RG32F": 33328,
    "R8I": 33329,
    "R8UI": 33330,
    "R16I": 33331,
    "R16UI": 33332,
    "R32I": 33333,
    "R32UI": 33334,
    "RG8I": 33335,
    "RG8UI": 33336,
    "RG16I": 33337,
    "RG16UI": 33338,
    "RG32I": 33339,
    "RG32UI": 33340,
    "VERTEX_ARRAY_BINDING": 34229,
    "R8_SNORM": 36756,
    "RG8_SNORM": 36757,
    "RGB8_SNORM": 36758,
    "RGBA8_SNORM": 36759,
    "SIGNED_NORMALIZED": 36764,
    "COPY_READ_BUFFER": 36662,
    "COPY_WRITE_BUFFER": 36663,
    "COPY_READ_BUFFER_BINDING": 36662,
    "COPY_WRITE_BUFFER_BINDING": 36663,
    "UNIFORM_BUFFER": 35345,
    "UNIFORM_BUFFER_BINDING": 35368,
    "UNIFORM_BUFFER_START": 35369,
    "UNIFORM_BUFFER_SIZE": 35370,
    "MAX_VERTEX_UNIFORM_BLOCKS": 35371,
    "MAX_FRAGMENT_UNIFORM_BLOCKS": 35373,
    "MAX_COMBINED_UNIFORM_BLOCKS": 35374,
    "MAX_UNIFORM_BUFFER_BINDINGS": 35375,
    "MAX_UNIFORM_BLOCK_SIZE": 35376,
    "MAX_COMBINED_VERTEX_UNIFORM_COMPONENTS": 35377,
    "MAX_COMBINED_FRAGMENT_UNIFORM_COMPONENTS": 35379,
    "UNIFORM_BUFFER_OFFSET_ALIGNMENT": 35380,
    "ACTIVE_UNIFORM_BLOCKS": 35382,
    "UNIFORM_TYPE": 35383,
    "UNIFORM_SIZE": 35384,
    "UNIFORM_BLOCK_INDEX": 35386,
    "UNIFORM_OFFSET": 35387,
    "UNIFORM_ARRAY_STRIDE": 35388,
    "UNIFORM_MATRIX_STRIDE": 35389,
    "UNIFORM_IS_ROW_MAJOR": 35390,
    "UNIFORM_BLOCK_BINDING": 35391,
    "UNIFORM_BLOCK_DATA_SIZE": 35392,
    "UNIFORM_BLOCK_ACTIVE_UNIFORMS": 35394,
    "UNIFORM_BLOCK_ACTIVE_UNIFORM_INDICES": 35395,
    "UNIFORM_BLOCK_REFERENCED_BY_VERTEX_SHADER": 35396,
    "UNIFORM_BLOCK_REFERENCED_BY_FRAGMENT_SHADER": 35398,
    "INVALID_INDEX": 4294967295,
    "MAX_VERTEX_OUTPUT_COMPONENTS": 37154,
    "MAX_FRAGMENT_INPUT_COMPONENTS": 37157,
    "MAX_SERVER_WAIT_TIMEOUT": 37137,
    "OBJECT_TYPE": 37138,
    "SYNC_CONDITION": 37139,
    "SYNC_STATUS": 37140,
    "SYNC_FLAGS": 37141,
    "SYNC_FENCE": 37142,
    "SYNC_GPU_COMMANDS_COMPLETE": 37143,
    "UNSIGNALED": 37144,
    "SIGNALED": 37145,
    "ALREADY_SIGNALED": 37146,
    "TIMEOUT_EXPIRED": 37147,
    "CONDITION_SATISFIED": 37148,
    "WAIT_FAILED": 37149,
    "SYNC_FLUSH_COMMANDS_BIT": 1,
    "VERTEX_ATTRIB_ARRAY_DIVISOR": 35070,
    "ANY_SAMPLES_PASSED": 35887,
    "ANY_SAMPLES_PASSED_CONSERVATIVE": 36202,
    "SAMPLER_BINDING": 35097,
    "RGB10_A2UI": 36975,
    "INT_2_10_10_10_REV": 36255,
    "TRANSFORM_FEEDBACK": 36386,
    "TRANSFORM_FEEDBACK_PAUSED": 36387,
    "TRANSFORM_FEEDBACK_ACTIVE": 36388,
    "TRANSFORM_FEEDBACK_BINDING": 36389,
    "TEXTURE_IMMUTABLE_FORMAT": 37167,
    "MAX_ELEMENT_INDEX": 36203,
    "TEXTURE_IMMUTABLE_LEVELS": 33503,
    "TIMEOUT_IGNORED": -1,
    "MAX_CLIENT_WAIT_TIMEOUT_WEBGL": 37447,
  })){
    Object.defineProperty(ctor,name,{value,enumerable:true});Object.defineProperty(ctor.prototype,name,{value,enumerable:true});
  }
})();
