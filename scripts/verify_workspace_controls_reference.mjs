// Inspect only a dedicated legacy-layout reference; never switch its repository.
import assert from 'node:assert/strict';
import path from 'node:path';
import { Cdp, delay, option, writeJson } from './stage4/cdp_client.mjs';

const endpoint = process.env.CHATGPT_CDP_HTTP;
assert(endpoint, 'Set CHATGPT_CDP_HTTP to the dedicated reference');
const output = option('output', 'artifacts/issues-23-24/reference');
const cdp = await Cdp.connect(endpoint);
async function click(label) {
  const p = await cdp.evaluate(`(() => {
    const e = [...document.querySelectorAll('button,[role=menuitem]')].find(e =>
      (e.getAttribute('aria-label') || e.textContent).trim() === ${JSON.stringify(label)});
    if (!e) throw Error('Missing ' + ${JSON.stringify(label)});
    const r = e.getBoundingClientRect(); return [r.x+r.width/2,r.y+r.height/2];
  })()`);
  await cdp.click(...p);
  await delay(250);
}
async function capture(name) {
  const data = await cdp.evaluate(`({
    viewport: [innerWidth,innerHeight,devicePixelRatio], theme:document.documentElement.getAttribute('data-theme'),
    text: document.body.innerText.slice(-2000),
    controls:[...document.querySelectorAll('[role=menu],[role=menuitem],[role=dialog],input,button[aria-label="Switch branch"],button[aria-label="Select where to run the chat"]')].map(e=>{
      const s=getComputedStyle(e),r=e.getBoundingClientRect();
      return {text:e.textContent,role:e.getAttribute('role'),label:e.getAttribute('aria-label'),value:e.value,placeholder:e.placeholder,
        rect:[r.x,r.y,r.width,r.height],font:s.font,color:s.color,background:s.backgroundColor,radius:s.borderRadius,padding:s.padding};
    })})`);
  writeJson(path.join(output, `${name}.json`), data);
  await cdp.screenshot(path.join(output, `${name}.png`));
}
async function escape() { await cdp.key('Escape'); await delay(350); }
try {
  assert(await cdp.evaluate(`!!document.querySelector('nav[aria-label="Chat history"],nav[aria-label="聊天记录"]') && !document.querySelector('nav[aria-label="App navigation"],nav[aria-label="应用导航"]')`));
  await escape();
  // The installed reference gates the legacy worktree flow independently of
  // the sidebar. Enable that existing flow in memory, without profile writes.
  await cdp.evaluate(`(() => {
    const c=window.__STATSIG__.firstInstance,original=c.getFeatureGate.bind(c);
    c.getFeatureGate=(name,options)=>{
      const gate=original(name,options);
      return ['505458','2785238453'].includes(name)?{...gate,value:name==='505458'}:gate;
    };
    c.$emt({name:'values_updated',status:'Ready',values:null});
  })()`);
  await delay(1600);
  // Keep the reference language override in memory, like the layout gate.
  await cdp.evaluate(`(() => {
    const root=document.querySelector('#root'),key=Object.keys(root).find(k=>k.startsWith('__reactContainer'));
    let provider; const walk=f=>{if(!f||provider)return;if(f.memoizedProps?.locale&&f.stateNode?.state?.intl){provider=f;return}walk(f.child);walk(f.sibling)};
    walk(root[key].current||root[key]);
    if(!provider)throw Error('Missing reference language provider');
    const props={...provider.memoizedProps,locale:'en-US',messages:{}};
    for(const fiber of [provider,provider.alternate].filter(Boolean)){fiber.memoizedProps=props;fiber.pendingProps=props}
    const instance=provider.stateNode;instance.props=props;
    instance.setState(provider.type.getDerivedStateFromProps(props,instance.state));
  })()`);
  await delay(500);
  await cdp.send('Emulation.setDeviceMetricsOverride', { width:1440,height:900,deviceScaleFactor:2,mobile:false });
  await click('Start new chat in Echora');
  await delay(500);
  if (await cdp.evaluate(`document.querySelector('button[aria-label="Select where to run the chat"]').textContent.includes('New local worktree')`)) {
    await click('Select where to run the chat');
    await click('Local');
  }
  for (const theme of ['dark','light']) {
    await cdp.evaluate(`document.documentElement.setAttribute('data-theme',${JSON.stringify(theme)})`);
    await click('Select where to run the chat');
    await capture(`${theme}-location`);
    assert((await cdp.evaluate(`document.querySelector('[role=menu]').innerText`)).includes('New local worktree'));
    await click('New local worktree');
    await capture(`${theme}-worktree-selected`);
    await click('What branch should this chat start from?');
    await capture(`${theme}-worktree-base`);
    await escape();
    await click('Select where to run the chat');
    await click('Local');
    await click('Switch branch');
    await capture(`${theme}-branches`);
    await cdp.send('Input.insertText', {text:'no-such-acceptance-branch'});
    await delay(400);
    await capture(`${theme}-branches-empty`);
    await escape();
    await click('Switch branch');
    await click('Create and checkout new branch…');
    await capture(`${theme}-create-branch`);
    await escape();
  }
  console.log(JSON.stringify({passed:true,output,captures:12}));
} finally { cdp.close(); }
