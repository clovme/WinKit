// 打开 Vite preview，切到「服务管理」tab，截图。
const http = require('http');
const fs = require('fs');

function get(path) {
  return new Promise((resolve, reject) => {
    http.get({ host: '127.0.0.1', port: 9222, path }, (res) => {
      let data = '';
      res.on('data', (c) => (data += c));
      res.on('end', () => resolve(JSON.parse(data)));
    }).on('error', reject);
  });
}

async function main() {
  const targets = await get('/json');
  const page = targets.find((t) => t.type === 'page');
  if (!page) throw new Error('page not found: ' + JSON.stringify(targets));
  const ws = new WebSocket(page.webSocketDebuggerUrl);
  await new Promise((r) => ws.addEventListener('open', r, { once: true }));
  let id = 0;
  const pending = new Map();
  ws.onmessage = (ev) => {
    const msg = JSON.parse(ev.data.toString());
    if (msg.id && pending.has(msg.id)) {
      pending.get(msg.id)(msg);
      pending.delete(msg.id);
    }
  };
  function send(method, params = {}) {
    return new Promise((resolve) => {
      const myId = ++id;
      pending.set(myId, resolve);
      ws.send(JSON.stringify({ id: myId, method, params }));
    });
  }

  await send('Page.enable');
  await send('Runtime.enable');
  await send('Page.navigate', { url: 'http://127.0.0.1:1421/' });
  await new Promise((r) => setTimeout(r, 1800));

  const expr = `(() => {
    const btns = Array.from(document.querySelectorAll('.view-tab'));
    const hit = btns.find(b => b.textContent.includes('服务管理'));
    if (hit) { hit.click(); return 'clicked'; }
    return 'not-found:' + btns.map(b => b.textContent).join('|');
  })()`;
  const r1 = await send('Runtime.evaluate', { expression: expr, returnByValue: true });
  console.log('tab click:', r1.result.result.value);

  await new Promise((r) => setTimeout(r, 1200));

  const shot = await send('Page.captureScreenshot', { format: 'png' });
  fs.writeFileSync(process.argv[2] || 'services.png', Buffer.from(shot.result.data, 'base64'));
  console.log('saved');
  ws.close();
}

main().catch((e) => { console.error(e); process.exit(1); });
