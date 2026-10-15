// Observe project and model workflows in an isolated, legacy-layout reference.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { Cdp } from './stage4/cdp_client.mjs';
import { delay, describe, option } from './p0/reference_ui.mjs';

const endpoint = process.env.CHATGPT_CDP_HTTP;
if (!endpoint) throw new Error('Set CHATGPT_CDP_HTTP to the dedicated reference instance');
const output = option('output', 'artifacts/project-composer-reference');
fs.mkdirSync(output, { recursive: true });
const cdp = await Cdp.connect(endpoint);
const report = [];
async function click(label, { projectRow = false } = {}) {
  const expression = `(() => {
    const scope = document.querySelector('[role=dialog],[role=menu]') || document;
    const e = [...scope.querySelectorAll('button,[role=button],[role=option],[role=menuitem],[role=menuitemradio]')]
      .find(e => (e.getAttribute('aria-label') || e.textContent).trim() === ${JSON.stringify(label)});
    if (!e) throw new Error('Missing control: ' + ${JSON.stringify(label)});
    let row = e;
    if (${projectRow}) {
      while (row.parentElement && !row.classList.contains('group/folder-row')) row = row.parentElement;
    }
    const r = row.getBoundingClientRect(); return [r.x + r.width/2, r.y + r.height/2];
  })()`;
  const point = await cdp.evaluate(expression);
  await cdp.hover(...(label === 'Add new project' ? [100, point[1]] : point));
  await delay(300);
  // Sidebar actions take their position only after the row's hover animation.
  const target = await cdp.evaluate(expression.replace(`if (${projectRow})`, 'if (false)'));
  await cdp.click(...target);
  await delay(350);
}
async function capture(name) {
  const geometry = await describe(cdp, `document.querySelectorAll('[role=dialog], [role=menu], [role=option], [role=menuitem], [role=menuitemradio], [role=slider]')`, { max: 50 });
  const state = await cdp.evaluate(`({theme:document.documentElement.className, viewport:[innerWidth,innerHeight,devicePixelRatio], text:document.body.innerText.slice(-1800)})`);
  fs.writeFileSync(path.join(output, `${name}.json`), JSON.stringify({ ...state, geometry }, null, 2));
  await cdp.screenshot(path.join(output, `${name}.png`));
  report.push({ name, viewport: state.viewport, geometry: geometry.map(e => ({ role: e.role, label: e.label, text: e.text, rect: e.rect })) });
}
try {
  assert(await cdp.evaluate(`!!document.querySelector('nav[aria-label="Chat history"],nav[aria-label="聊天记录"]') && !document.querySelector('nav[aria-label="App navigation"],nav[aria-label="应用导航"]')`), 'Pin the dedicated reference to the legacy layout before capturing');
  await cdp.key('Escape');
  await cdp.scroll(160, 450, -4000);
  await delay(350);
  const project = option('project', 'Echora');
  await click(`Start new chat in ${project}`, { projectRow: true });
  assert((await cdp.evaluate('document.body.innerText')).includes(`What should we build in ${project}?`));
  await capture('new-chat');
  await click(`Change project: ${project}`);
  await capture('project-picker');
  await cdp.send('Input.insertText', { text: 'no-matching-project-acceptance' });
  await delay(200);
  const filtered = await cdp.evaluate(`document.querySelector('[role=dialog]').innerText`);
  assert(filtered.includes('New project'));
  assert(filtered.includes("Don't work in a project"));
  await capture('project-search-empty');
  await click('New project');
  await capture('create-project');
  await cdp.send('Input.insertText', { text: 'Discarded acceptance draft' });
  assert.equal(await cdp.evaluate(`document.querySelector('[role=dialog] input').value`), 'Discarded acceptance draft');
  await click('Project device');
  await capture('project-device');
  assert.equal((await cdp.evaluate(`document.querySelector('[role=menu]').innerText`)).trim(), 'This computer');
  await cdp.key('Escape');
  await click('Close dialog');
  await click('Add new project', { projectRow: false });
  assert.equal(await cdp.evaluate(`document.querySelector('[role=dialog] input').value`), '');
  await click('Cancel');
  await click(`Change project: ${project}`);
  await click("Don't work in a project");
  await capture('projectless');
  await click('Choose project');
  await click(project);
  const modelTrigger = await cdp.evaluate(`(() => { const e = [...document.querySelectorAll('button')].find(e => e.textContent.includes('Persistent')); const r = e.getBoundingClientRect(); return [r.x+r.width/2,r.y+r.height/2]; })()`);
  await cdp.click(...modelTrigger);
  await delay(350);
  await capture('model-slider');
  await click('Select model');
  await capture('model-list');
  const otherModel = option('model', '6 Astra');
  await click(otherModel);
  await capture('model-selected');
  assert((await cdp.evaluate(`document.querySelector('[role=menu]').innerText`)).includes(otherModel));
  await cdp.key('ArrowLeft');
  await capture('model-effort');
  await click('Reset to default');
  await capture('model-default');
  await cdp.key('Escape');
  fs.writeFileSync(path.join(output, 'report.json'), JSON.stringify(report, null, 2));
  console.log(JSON.stringify({ passed: true, captures: report.length, output }));
} finally {
  cdp.close();
}
