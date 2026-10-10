// Included inside the private WebGL closure. No pixel storage is exposed as
// an expando, symbol or global map. WebGL uploads consume the original alpha
// representation; closing a bitmap drops its only owned pixel buffer.
const bitmaps = new WeakMap();
const Bitmap = illegal('ImageBitmap');
function bitmapState(receiver) {
  const value = bitmaps.get(receiver);
  if (!value) throw new TypeError('Illegal invocation');
  return value;
}
for (const name of ['width','height']) {
  const get = function() { return bitmapState(this)[name]; };
  _markNative(get);
  Object.defineProperty(Bitmap.prototype,name,{get,enumerable:true,configurable:true});
}
Object.defineProperty(Bitmap.prototype,'close',{enumerable:true,configurable:true,writable:true,value:function close() {
  const value=bitmapState(this);value.bytes=null;value.width=0;value.height=0;
}});
_markNative(Bitmap.prototype.close);
function newBitmap(value) { const bitmap=new Bitmap(secret);bitmaps.set(bitmap,value);return bitmap; }
function bitmapSize(width,height) {
  if(!Number.isInteger(width)||!Number.isInteger(height)||width<=0||height<=0||width>32767||height>32767||width*height>16777216)
    throw new DOMException('Bitmap dimensions exceed the supported allocation','InvalidStateError');
}
function bitmapOptions(value) {
  if(value!=null&&typeof value!=='object'&&typeof value!=='function')throw new TypeError('Expected ImageBitmapOptions');
  const result={};
  for(const [key,fallback,values] of [
    ['imageOrientation','from-image',['from-image','flipY']],
    ['premultiplyAlpha','default',['default','none','premultiply']],
    ['colorSpaceConversion','default',['default','none']],
    ['resizeQuality','low',['pixelated','low','medium','high']]]) {
    const input=value?.[key];result[key]=input===undefined?fallback:text(input);
    if(!values.includes(result[key]))throw new TypeError('Invalid '+key);
  }
  for(const key of ['resizeWidth','resizeHeight']) {
    const input=value?.[key];if(input===undefined)continue;
    const number=+input,integer=Math.trunc(number);
    if(!Number.isFinite(number)||integer<0||integer>4294967295)throw new TypeError('Invalid '+key);
    if(integer===0)throw new DOMException('Resize dimensions must be positive','InvalidStateError');
    result[key]=integer;
  }
  return result;
}
function formattedBitmap(source,crop,options) {
  bitmapSize(source.width,source.height);
  if(!source.bytes||source.bytes.byteLength!==source.width*source.height*4)throw new DOMException('Detached image source','InvalidStateError');
  let [x,y,width,height]=crop||[0,0,source.width,source.height];
  if(!width||!height)throw new RangeError('Crop dimensions must not be zero');
  if(width<0){x+=width;width=-width;}if(height<0){y+=height;height=-height;}
  const outWidth=options.resizeWidth??(options.resizeHeight===undefined?width:Math.ceil(width*options.resizeHeight/height));
  const outHeight=options.resizeHeight??(options.resizeWidth===undefined?height:Math.ceil(height*options.resizeWidth/width));
  bitmapSize(outWidth,outHeight);
  const output=new Uint8Array(outWidth*outHeight*4);
  // The engine's 2D store uses straight alpha. Keep that representation for
  // default, and convert only when explicitly requested by the caller.
  const premultiplied=options.premultiplyAlpha==='premultiply';
  const inputPremultiplied=source.premultiplied===true;
  // Pixelated scaling is nearest-neighbor to the nearest integer multiple,
  // followed by bilinear to the requested dimensions, evaluated without an
  // intermediate allocation (including a crop mostly outside the source).
  const multipleX=options.resizeQuality==='pixelated'?Math.max(1,Math.round(outWidth/width)):1;
  const multipleY=options.resizeQuality==='pixelated'?Math.max(1,Math.round(outHeight/height)):1;
  const virtualWidth=width*multipleX,virtualHeight=height*multipleY;
  function sample(px,py,channel) {
    const ix=x+Math.floor(Math.max(0,Math.min(virtualWidth-1,px))/multipleX);
    const iy=y+Math.floor(Math.max(0,Math.min(virtualHeight-1,py))/multipleY);
    if(ix<0||iy<0||ix>=source.width||iy>=source.height)return 0;
    const offset=(iy*source.width+ix)*4,alpha=source.bytes[offset+3]/255;
    const value=source.bytes[offset+channel];
    // Interpolation uses premultiplied colors to avoid transparent fringes.
    return channel<3&&!inputPremultiplied?value*alpha:value;
  }
  const exact=outWidth===width&&outHeight===height;
  for(let row=0;row<outHeight;row++)for(let col=0;col<outWidth;col++) {
    const sourceRow=options.imageOrientation==='flipY'?outHeight-1-row:row;
    const target=(row*outWidth+col)*4;
    if(exact) {
      const ix=x+col,iy=y+sourceRow;
      if(ix<0||iy<0||ix>=source.width||iy>=source.height)continue;
      const offset=(iy*source.width+ix)*4,alpha=source.bytes[offset+3]/255;
      for(let c=0;c<4;c++) {
        let value=source.bytes[offset+c];
        if(c<3&&premultiplied!==inputPremultiplied)value=premultiplied?value*alpha:(alpha?value/alpha:0);
        output[target+c]=Math.min(255,Math.round(value));
      }
      continue;
    }
    const fx=(col+.5)*virtualWidth/outWidth-.5,fy=(sourceRow+.5)*virtualHeight/outHeight-.5;
    const left=Math.floor(fx),top=Math.floor(fy),tx=fx-left,ty=fy-top;
    const values=[];
    for(let c=0;c<4;c++)values[c]=(sample(left,top,c)*(1-tx)+sample(left+1,top,c)*tx)*(1-ty)+(sample(left,top+1,c)*(1-tx)+sample(left+1,top+1,c)*tx)*ty;
    const alpha=values[3]/255;
    for(let c=0;c<4;c++)output[target+c]=Math.min(255,Math.round(c<3&&!premultiplied?(alpha?values[c]/alpha:0):values[c]));
  }
  return {width:outWidth,height:outHeight,bytes:output,premultiplied,originClean:source.originClean!==false,colorSpace:source.colorSpace||'srgb'};
}
const NativeBlob=globalThis.Blob;
const readBlob=NativeBlob?.prototype.arrayBuffer;
_canvasBitmapDraw=(context,image,args)=>{
  if(!bitmaps.has(image)&&!offscreens.has(image)&&!canvases.has(image)&&!_placeholderHas(image))return false;
  let source=bitmaps.has(image)?bitmapState(image):imageSource(null,image,true);
  if(!source.bytes)throw new DOMException('The ImageBitmap is detached','InvalidStateError');
  if(![2,4,8].includes(args.length))throw new TypeError('Invalid drawImage overload');
  const values=args.map(value=>+value);if(values.some(v=>!Number.isFinite(v)))return true;
  let sx=0,sy=0,sw=source.width,sh=source.height,dx,dy,dw=sw,dh=sh;
  if(values.length===8)[sx,sy,sw,sh,dx,dy,dw,dh]=values;
  else { [dx,dy]=values;if(values.length===4)[dw,dh]=values.slice(2); }
  if(!sw||!sh||!dw||!dh)return true;
  if(sw<0){sx+=sw;sw=-sw;}if(sh<0){sy+=sh;sh=-sh;}
  if(dw<0){dx+=dw;dw=-dw;}if(dh<0){dy+=dh;dh=-dh;}
  const destination=_canvas2DPixels(context);if(!destination)throw new TypeError('Illegal invocation');
  // Validate the draw before allocating conversion pixels. Rejected/no-op
  // arguments must not trigger color work or alter either image's ownership.
  source=pixelsInColorSpace(source,'srgb');
  if(source.bytes.buffer===destination.bytes.buffer)source={...source,bytes:source.bytes.slice()};
  if(!source.originClean)_canvas2DTaint(context);
  // Work only inside the destination, avoiding a temporary bitmap proportional
  // to a mostly clipped destination rectangle. The ordinary 2D path keeps its
  // existing scaling behavior; bitmap interpolation remains a conformance gate.
  const left=Math.max(0,Math.ceil(dx)),right=Math.min(destination.width,Math.ceil(dx+dw));
  const top=Math.max(0,Math.ceil(dy)),bottom=Math.min(destination.height,Math.ceil(dy+dh));
  const globalAlpha=Number(context.globalAlpha);
  if(!Number.isFinite(globalAlpha)||globalAlpha<0||globalAlpha>1)return true;
  for(let y=top;y<bottom;y++)for(let x=left;x<right;x++) {
    const ix=Math.floor(sx+(x+.5-dx)*sw/dw),iy=Math.floor(sy+(y+.5-dy)*sh/dh);
    if(ix<0||iy<0||ix>=source.width||iy>=source.height)continue;
    const from=(iy*source.width+ix)*4,to=(y*destination.width+x)*4;
    const rawAlpha=source.bytes[from+3]/255,alpha=rawAlpha*globalAlpha,oldAlpha=destination.bytes[to+3]/255;
    const copy=context.globalCompositeOperation==='copy';
    const outAlpha=destination.alpha===false?1:(copy?alpha:alpha+oldAlpha*(1-alpha));
    for(let c=0;c<3;c++) {
      const value=source.premultiplied?(rawAlpha?source.bytes[from+c]/rawAlpha:0):source.bytes[from+c];
      const old=destination.bytes[to+c];
      const blended=context.globalCompositeOperation==='multiply'?value*old/255:value;
      const output=copy?value*alpha:alpha*((1-oldAlpha)*value+oldAlpha*blended)+(1-alpha)*oldAlpha*old;
      destination.bytes[to+c]=outAlpha?Math.min(255,Math.round(output/outAlpha)):0;
    }
    destination.bytes[to+3]=Math.round(outAlpha*255);
  }
  context._markPaintDamage();return true;
};
function decodeBitmapBlob(bytes) {
  if(bytes.byteLength>67108864)throw new DOMException('Encoded image is too large','InvalidStateError');
  const info=__obscuraCore.ops.op_webgl_decode_info(bytes);
  if(info.status!=='ready')throw new DOMException('The Blob is not a supported image','InvalidStateError');
  bitmapSize(info.width,info.height);
  const pixels=new Uint8Array(info.width*info.height*4);
  if(!__obscuraCore.ops.op_webgl_decode_pixels(bytes,pixels))throw new DOMException('The Blob could not be decoded','InvalidStateError');
  return {width:info.width,height:info.height,bytes:pixels,originClean:true};
}
global('createImageBitmap',function createImageBitmap(image,...args) {
  // Snapshot ordinary source pixels before returning to the caller. Blob I/O
  // is asynchronous. Resolve on the bitmap task boundary rather than in a
  // microtask immediately following this function.
  let options,crop,result;
  const frame=_realmFrameId;
  const documentGeneration=_hostState.documentGeneration;
  try {
    if(arguments.length===0)throw new TypeError('Image source is required');
    if(args.length>1) {
      if(args.length<4)throw new TypeError('Four crop coordinates are required');
      crop=args.slice(0,4).map(value=>int(+value));
      if(!crop[2]||!crop[3])throw new RangeError('Crop dimensions must not be zero');
      options=bitmapOptions(args[4]);
    } else options=bitmapOptions(args[0]);
    if(NativeBlob&&image instanceof NativeBlob) {
      result=Promise.resolve(readBlob.call(image)).then(buffer=>formattedBitmap(decodeBitmapBlob(new Uint8Array(buffer)),crop,options));
    } else result=formattedBitmap(imageSource(null,image,true),crop,options);
  } catch(e) { return Promise.reject(e); }
  return new Promise((resolve,reject)=>{
    const current=()=>_hostState.documentGeneration===documentGeneration;
    Promise.resolve(result).then(value=>{if(current())queueContextTask(frame,()=>resolve(newBitmap(value)));},error=>{if(current())queueContextTask(frame,()=>reject(error));});
  });
});
