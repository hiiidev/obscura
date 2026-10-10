#!/usr/bin/env python3
"""Local 800x600 graphics captures through an owned CDP endpoint or Chromium."""
import argparse,asyncio,base64,hashlib,json
from pathlib import Path
from playwright.async_api import async_playwright

async def main(args):
    args.out.mkdir(parents=True,exist_ok=False)
    fixture=Path(__file__).with_name('fixture.html').read_bytes()
    result={'fixture_sha256':hashlib.sha256(fixture).hexdigest(),'viewport':[800,600,1],'mode':args.expect,'scope':args.scope,'scenes':[]}
    async def handle(reader,writer):
        try:
            head=await asyncio.wait_for(reader.readuntil(b'\r\n\r\n'),5)
            allowed=head.split(b'\r\n',1)[0] == b'GET /fixture HTTP/1.1'
            body=fixture if allowed else b'';status=b'200 OK' if allowed else b'404 Not Found'
            writer.write(b'HTTP/1.1 '+status+b'\r\nContent-Type: text/html\r\nContent-Length: '+str(len(body)).encode()+b'\r\nConnection: close\r\n\r\n'+body);await writer.drain()
        finally:
            writer.close()
            try:await writer.wait_closed()
            except ConnectionError:pass
    server=await asyncio.start_server(handle,'127.0.0.1',0)
    url='http://127.0.0.1:'+str(server.sockets[0].getsockname()[1])+'/fixture'
    external=[]
    try:
      async with server,async_playwright() as pw:
        browser=await pw.chromium.connect_over_cdp(args.endpoint) if args.endpoint else await pw.chromium.launch(headless=True)
        context=None
        try:
            context=await browser.new_context(viewport={'width':800,'height':600},device_scale_factor=1,locale='en-US',timezone_id='UTC')
            async def restrict(route):
                if route.request.url==url:await route.continue_()
                else:external.append(route.request.url);await route.abort()
            await context.route('**/*',restrict)
            page=await context.new_page();await page.goto(url,wait_until='load',timeout=30000)
            result['browser_version']=browser.version
            if args.expect=='unavailable':
                probe=await page.evaluate("() => {const a=document.createElement('canvas'),b=document.createElement('canvas'),c=document.createElement('canvas');const ctx=c.getContext('2d');ctx.fillStyle='red';ctx.fillRect(0,0,1,1);return [a.getContext('webgl')===null,b.getContext('webgl2')===null,Array.from(ctx.getImageData(0,0,1,1).data)]}")
                result['baseline_probe']=probe
                assert probe[:2]==[True,True] and all(abs(a-b)<=2 for a,b in zip(probe[2],[255,0,0,255])),probe
            else:
                if args.scope in ('full','core'):
                  for version in [1,2]:
                      for scene in ['clear','triangle','texture','alpha','resize']:
                          label=f'webgl{version}-{scene}'
                          item=await asyncio.wait_for(page.evaluate('args=>runScene(args)',{'version':version,'scene':scene}),30)
                          raw=item.pop('readPixelsBase64');png=item.pop('png')
                          (args.out/(label+'.rgba')).write_bytes(base64.b64decode(raw))
                          (args.out/(label+'.canvas.png')).write_bytes(base64.b64decode(png.split(',',1)[1]))
                          await page.screenshot(path=str(args.out/(label+'.browser.png')))
                          result['scenes'].append(item)
                          assert item['error']==0 and item['pixel_checks_passed'],item
                          assert item['timer_ticks']>0 and item['mutation_count']>0,item
                          assert item['viewport']==[800,600,1],item
                  if args.scope in ('full','presentation'):
                  presentation=await asyncio.wait_for(page.evaluate('()=>runPresentation()'),30)
                  png=presentation.pop('png');(args.out/'placeholder.canvas.png').write_bytes(base64.b64decode(png.split(',',1)[1]))
                  await page.screenshot(path=str(args.out/'placeholder-svg.browser.png'))
                  result['presentation']=presentation
                  assert presentation['presented']==[30,10]
                  assert presentation['author_dimensions']==[90,10] and presentation['bitmap_dimensions']==[30,10]
                  assert all(abs(a-b)<=2 for a,b in zip(presentation['bitmap_pixel'],[255,0,0,255]))
                  assert presentation['svg_natural']==[300,150] and presentation['svg_rect']==[300,150]
                  # The renderer rounds paint geometry to device pixels. Preserve
                  # the measured fractional Chromium geometry rather than claiming parity.
                  assert presentation['rect'][0]==100 and abs(presentation['rect'][1]-100/9)<=1
              assert not external,external
        finally:
            try:
                if context is not None:await context.close()
            finally:
                if not args.endpoint:await browser.close()
    finally:
        result['unexpected_requests']=external
        (args.out/'result.json').write_text(json.dumps(result,indent=2)+'\n')

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--out',type=Path,required=True);p.add_argument('--endpoint');p.add_argument('--expect',choices=['webgl','unavailable'],default='webgl');p.add_argument('--scope',choices=['full','core','presentation'],default='full');asyncio.run(main(p.parse_args()))
