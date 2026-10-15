// Observe a real turn through the dedicated reference's own composer.
import fs from 'node:fs';
import path from 'node:path';
import { Cdp, delay, option } from './stage4/cdp_client.mjs';

const out = path.resolve(option('output', 'artifacts/live-reference'));
fs.mkdirSync(out, { recursive: true });
const cdp = await Cdp.connect(process.env.CHATGPT_CDP_HTTP ?? 'http://127.0.0.1:9335');
await cdp.send('Emulation.setDeviceMetricsOverride', { width: 1440, height: 900, deviceScaleFactor: 1, mobile: false });
const snapshot = `(() => {
  const root = document.querySelector('[data-codex-composer-root]');
  const button = [...(root?.querySelectorAll('button') ?? [])].at(-1);
  const describe = e => { const s = getComputedStyle(e), r = e.getBoundingClientRect(); return {html:e.outerHTML,rect:[r.x,r.y,r.width,r.height],color:s.color,background:s.backgroundColor,opacity:s.opacity,borderRadius:s.borderRadius,cursor:s.cursor,disabled:e.disabled,busy:e.getAttribute('aria-busy')}; };
  return {time:performance.now(),button:button && describe(button), input:root?.querySelector('[contenteditable]')?.outerHTML, units:[...document.querySelectorAll('[data-content-search-unit-key]')].map(e=>({key:e.getAttribute('data-content-search-unit-key'),...describe(e)})), text:document.body.innerText.slice(-5000)};
})()`;
const frames = [];
async function record(name, capture = true) {
  const state = await cdp.evaluate(snapshot);
  frames.push({ name, ...state });
  if (capture) await cdp.screenshot(path.join(out, name + '.png'));
  fs.writeFileSync(path.join(out, 'timeline.json'), JSON.stringify(frames, null, 2));
  return state;
}
try {
  await cdp.key('Escape');
  const theme = option('theme', 'dark');
  await cdp.evaluate(`document.documentElement.classList.remove('electron-light','electron-dark'); document.documentElement.classList.add('electron-${theme}'); document.documentElement.dataset.theme = '${theme}'`);
  const fresh = await cdp.evaluate(`(() => {const e=[...document.querySelectorAll('button,a')].find(e=>e.getAttribute('aria-label')==='New chat' || e.textContent.trim()==='New chat'); const r=e?.getBoundingClientRect(); return r && [r.x+r.width/2,r.y+r.height/2]})()`);
  if (fresh) await cdp.click(...fresh);
  await delay(600);
  await record('00-empty');
  const input = await cdp.evaluate(`(() => {const r=document.querySelector('[data-codex-composer-root] [contenteditable=true]').getBoundingClientRect();return [r.x+30,r.y+12]})()`);
  await cdp.click(...input);
  await cdp.send('Input.insertText', { text: option('prompt', 'TOOLS_STREAM') });
  await delay(250);
  const ready = await record('01-ready');
  const r = ready.button.rect;
  await cdp.hover(r[0] + r[2]/2, r[1] + r[3]/2);
  await record('02-hover');
  await cdp.send('Input.dispatchMouseEvent', { type:'mousePressed',x:r[0]+r[2]/2,y:r[1]+r[3]/2,button:'left',clickCount:1 });
  await record('03-pressed');
  await cdp.send('Input.dispatchMouseEvent', { type:'mouseReleased',x:r[0]+r[2]/2,y:r[1]+r[3]/2,button:'left',clickCount:1 });
  const started = Date.now();
  let drafted = false;
  for (let i=0;i<90;i++) {
    await delay(200);
    await record('live-' + String(i).padStart(3,'0'), i%5===0);
    if (option('draft-at') && !drafted && Date.now()-started > Number(option('draft-at'))) {
      drafted = true;
      const input = await cdp.evaluate(`(() => {const r=document.querySelector('[data-codex-composer-root] [contenteditable=true]').getBoundingClientRect();return [r.x+20,r.y+10]})()`);
      await cdp.click(...input);
      await cdp.send('Input.insertText', {text:'Draft follow-up'});
      await delay(250);
      await record('running-with-draft');
      await cdp.key('a', 4);
      await cdp.key('Backspace');
      await delay(250);
      await record('running-after-clear');
    }
    if (option('interrupt') && Date.now()-started > Number(option('interrupt'))) {
      const state = await cdp.evaluate(snapshot);
      if (state.button?.html.includes('aria-label="Stop"')) {
        const [x,y,w,h]=state.button.rect;
        await cdp.click(x+w/2,y+h/2);
        await record('interrupted');
        await delay(500);
        break;
      }
    }
  }
  await record('99-final');
  console.log(JSON.stringify({output:out,frames:frames.length,buttonStates:[...new Set(frames.map(f=>JSON.stringify({label:f.button?.html.match(/aria-label="([^"]+)/)?.[1],busy:f.button?.busy,opacity:f.button?.opacity,background:f.button?.background,rect:f.button?.rect})))]},null,2));
} finally { cdp.socket.close(); }
