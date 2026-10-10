// Optional graphics bindings. Pixel ownership lives on a V8 private slot so a
// same-agent foreign realm can consume ImageData without reading page getters.
const imageDataCache=new WeakMap();
const ImageBytes=Uint8ClampedArray,ByteView=Uint8Array,isPixelView=ArrayBuffer.isView;
const arrayPrototype=Object.getPrototypeOf(ImageBytes.prototype);
const arrayTag=Object.getOwnPropertyDescriptor(arrayPrototype,Symbol.toStringTag).get;
const arrayLength=Object.getOwnPropertyDescriptor(arrayPrototype,'length').get;
const arrayBuffer=Object.getOwnPropertyDescriptor(arrayPrototype,'buffer').get;
const arrayOffset=Object.getOwnPropertyDescriptor(arrayPrototype,'byteOffset').get;
const bufferLength=Object.getOwnPropertyDescriptor(ArrayBuffer.prototype,'byteLength').get;
const bufferResizable=Object.getOwnPropertyDescriptor(ArrayBuffer.prototype,'resizable')?.get;
function imageDataState(value,required=true){
  let state;
  if(value!==null&&(typeof value==='object'||typeof value==='function')){
    state=imageDataCache.get(value)||__obscuraCore.ops.op_webgl_image_data(value,null);
    if(state)imageDataCache.set(value,state);
  }
  if(!state&&required)throw new TypeError('Expected ImageData');
  return state||null;
}
function imageDataOptions(value){
  if(value!=null&&typeof value!=='object'&&typeof value!=='function')throw new TypeError('Expected ImageDataSettings');
  const color=value?.colorSpace;
  const result={colorSpace:color===undefined?'srgb':colorSpace(color),pixelFormat:'rgba-unorm8'};
  const format=value?.pixelFormat;
  if(format!==undefined)result.pixelFormat=text(format);
  if(!['rgba-unorm8','rgba-float16'].includes(result.pixelFormat))throw new TypeError('Invalid ImageDataPixelFormat');
  // The current image pipeline owns RGBA8 surfaces. Do not silently label
  // eight-bit storage as HDR or reinterpret a float buffer as bytes.
  if(result.pixelFormat!=='rgba-unorm8')throw new DOMException('Float ImageData storage is unavailable','NotSupportedError');
  return result;
}
function imageDataSize(width,height){
  if(!width||!height)throw new DOMException('ImageData dimensions must be positive','IndexSizeError');
  if(width>32767||height>32767||width*height>16777216)throw new RangeError('ImageData allocation exceeds the pixel budget');
  return width*height*4;
}
function imageDataBytes(data){
  if(arrayTag.call(data)!=='Uint8ClampedArray')throw new TypeError('Expected Uint8ClampedArray');
  const buffer=arrayBuffer.call(data);
  // ArrayBuffer's intrinsic getter rejects SharedArrayBuffer, including one
  // with a forged tag. Resizable buffers are not accepted by this WebIDL type.
  bufferLength.call(buffer);
  if(bufferResizable?.call(buffer))throw new TypeError('Resizable ImageData buffers are unsupported');
  try{return new ByteView(buffer,arrayOffset.call(data),arrayLength.call(data));}
  catch(_){throw new DOMException('ImageData buffer is detached','InvalidStateError');}
}
function imageDataSource(value){
  const state=imageDataState(value,false);if(!state)return null;
  const bytes=imageDataBytes(state.data);
  if(bytes.length!==state.width*state.height*4)throw new DOMException('ImageData buffer is detached','InvalidStateError');
  return{width:state.width,height:state.height,bytes,colorSpace:state.colorSpace,originClean:true,premultiplied:false};
}
const BrowserImageData=class ImageData {
  constructor(first,second,...rest){
    if(arguments.length<2)throw new TypeError('ImageData requires two arguments');
    let width,height,data,settings;
    if(isPixelView(first)){
      width=uint(second);height=rest[0]===undefined?undefined:uint(rest[0]);settings=imageDataOptions(rest[1]);
      if(!width||height===0)throw new DOMException('ImageData dimensions must be positive','IndexSizeError');
      const bytes=imageDataBytes(first);
      if(!bytes.length||bytes.length%4)throw new DOMException('ImageData needs complete RGBA pixels','InvalidStateError');
      if(bytes.length%(width*4))throw new DOMException('ImageData rows do not match width','IndexSizeError');
      const rows=bytes.length/(width*4);
      if(height!==undefined&&height!==rows)throw new DOMException('ImageData height does not match data','IndexSizeError');
      height=rows;imageDataSize(width,height);data=first;
    }else{
      width=uint(first);height=uint(second);settings=imageDataOptions(rest[0]);
      data=new ImageBytes(imageDataSize(width,height));
    }
    const state=Object.freeze({width,height,data,...settings});
    if(!__obscuraCore.ops.op_webgl_image_data(this,state))throw new RangeError('ImageData state allocation failed');
    imageDataCache.set(this,state);
  }
};
for(const name of ['width','height','data','colorSpace','pixelFormat']){
  const get=function(){return imageDataState(this)[name];};_markNative(get);
  Object.defineProperty(BrowserImageData.prototype,name,{get,enumerable:true,configurable:true});
}
Object.defineProperty(BrowserImageData.prototype,Symbol.toStringTag,{value:'ImageData',configurable:true});
global('ImageData',BrowserImageData);

// Upgrade only the optional graphics candidate. Default builds keep their
// existing Canvas2D shim. These methods use private pixels and ignore expando
// getters on both the context and ImageData object.
if(typeof _Canvas2D!=='undefined'){
  const markDamage=_Canvas2D.prototype._markPaintDamage;
  function surface(receiver){const value=_canvas2DPixels(receiver);if(!value)throw new TypeError('Illegal invocation');return value;}
  const methods={
    createImageData:function createImageData(width,height,...rest){
      surface(this);
      if(arguments.length===1){const state=imageDataState(width);return new BrowserImageData(state.width,state.height,{colorSpace:state.colorSpace,pixelFormat:state.pixelFormat});}
      if(arguments.length<2)throw new TypeError('ImageData dimensions required');
      return new BrowserImageData(Math.abs(int(width)),Math.abs(int(height)),rest[0]);
    },
    getImageData:function getImageData(x,y,width,height,...rest){
      surface(this);if(arguments.length<4)throw new TypeError('ImageData rectangle required');
      x=int(x);y=int(y);width=int(width);height=int(height);const settings=imageDataOptions(rest[0]);
      if(!width||!height)throw new DOMException('ImageData dimensions must be nonzero','IndexSizeError');
      // Argument conversion can execute page code that resizes or taints the
      // canvas. Read its current backing only after those conversions finish.
      const current=surface(this);
      if(!current.originClean)throw new DOMException('The canvas is not origin-clean','SecurityError');
      if(width<0){x+=width;width=-width;}if(height<0){y+=height;height=-height;}
      const image=new BrowserImageData(width,height,settings),data=imageDataState(image).data;
      for(let row=Math.max(0,-y);row<Math.min(height,current.height-y);row++){
        const left=Math.max(0,-x),right=Math.min(width,current.width-x);if(left>=right)continue;
        const start=((y+row)*current.width+x+left)*4;
        data.set(current.bytes.subarray(start,start+(right-left)*4),(row*width+left)*4);
      }
      if(settings.colorSpace!=='srgb'&&!__obscuraCore.ops.op_webgl_convert_color('srgb',settings.colorSpace,false,data))
        throw new DOMException('Image color conversion failed','InvalidStateError');
      return image;
    },
    putImageData:function putImageData(image,dx,dy,...dirty){
      surface(this);if(arguments.length<3)throw new TypeError('ImageData position required');
      const metadata=imageDataState(image);
      dx=int(dx);dy=int(dy);
      let sx=0,sy=0,width=metadata.width,height=metadata.height;
      if(dirty.length){if(dirty.length<4)throw new TypeError('Four dirty rectangle arguments required');[sx,sy,width,height]=dirty.slice(0,4).map(int);}
      let input=imageDataSource(image);const current=surface(this);
      if(width<0){sx+=width;width=-width;}if(height<0){sy+=height;height=-height;}
      const left=Math.max(0,sx,-dx),right=Math.min(input.width,sx+width,current.width-dx);
      const top=Math.max(0,sy,-dy),bottom=Math.min(input.height,sy+height,current.height-dy);
      if(left>=right||top>=bottom)return;
      input=pixelsInColorSpace(input,'srgb');
      for(let y=top;y<bottom;y++){
        const start=(y*input.width+left)*4,target=((dy+y)*current.width+dx+left)*4;
        current.bytes.set(input.bytes.subarray(start,start+(right-left)*4),target);
        if(current.alpha===false)for(let x=target+3;x<target+(right-left)*4;x+=4)current.bytes[x]=255;
      }
      markDamage.call(this);
    }
  };
  for(const [name,value] of Object.entries(methods)){
    _markNative(value);Object.defineProperty(_Canvas2D.prototype,name,{value,writable:true,enumerable:true,configurable:true});
  }
}
