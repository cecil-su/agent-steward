const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const server = path.resolve(__dirname, '../..');

test('production embedded UI comes only from the readonly snapshot', () => {
  const source = fs.readFileSync(path.join(server, 'src/ui.rs'), 'utf8');
  for (const file of ['index.html', 'app.js', 'style.css']) {
    assert(source.includes(`include_str!("../web-readonly/${file}")`));
    assert(fs.statSync(path.join(server, 'web-readonly', file)).size > 0);
  }
  assert(!fs.existsSync(path.join(server, 'web')), 'retired writable UI must not return');
  const bundle = fs.readFileSync(path.join(server, 'web-readonly/app.js'), 'utf8');
  assert(!bundle.includes('/api/commands/'), 'embedded bundle must not send business commands');
  assert(!bundle.includes('/api/hook'), 'embedded bundle must not ingest hook events');
  assert(bundle.includes('/api/login'), 'authentication is still available');
  assert(bundle.includes('/api/tasks'), 'readonly business queries are still available');
});
