import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import ts from 'typescript';

const source = readFileSync(new URL('../src/providerTurnstile.ts', import.meta.url), 'utf8');
function moduleWith(env = {}) {
  const js = ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } }).outputText;
  const mod = { exports: {} };
  new Function('exports', 'module', 'window', 'document', 'setTimeout', 'clearTimeout', js)(mod.exports, mod, env.window || { location: { hostname: 'console.example.test' } }, env.document, env.setTimeout || setTimeout, env.clearTimeout || clearTimeout);
  return mod.exports;
}
function widget(width = 480) {
  const events = []; let options; let resets = 0; let removed = 0;
  const api = { render: (_, opts) => { options = opts; return 'widget-1'; }, reset: () => resets++, remove: () => removed++ };
  const controller = moduleWith().createProviderChallenge(api, { clientWidth: width }, 'public-fixture-key', s => events.push(s));
  return { controller, events, get options() { return options; }, get resets() { return resets; }, get removed() { return removed; } };
}
test('repeated domain failures stop without resetting or automatic retry', () => {
  const w = widget();
  for (let i = 0; i < 3; i++) w.options['error-callback']('110200');
  assert.equal(w.resets, 0); assert.equal(w.options.retry, 'never');
  assert.equal(w.options['refresh-expired'], 'manual'); assert.equal(w.options['refresh-timeout'], 'manual');
  assert.equal(w.events.filter(e => e.status === 'error').length, 1);
  assert.match(w.events.at(-1).message, /console.example.test/);
  w.options.callback('late-proof'); assert.equal(w.controller.consume(), '');
});
test('proof is consumed exactly once and late callbacks cannot restore it', () => {
  const w = widget(); w.options.callback('fixture-one-time-proof');
  assert.equal(w.controller.consume(), 'fixture-one-time-proof');
  w.options.callback('late-proof'); w.options['expired-callback']();
  assert.equal(w.controller.consume(), ''); assert.equal(w.resets, 0);
  assert.equal(w.events.at(-1).status, 'consumed');
  assert.equal(w.options['response-field'], false);
});
test('expiry and timeout clear proof without starting new challenges', () => {
  for (const event of ['expired-callback', 'timeout-callback', 'unsupported-callback']) {
    const w = widget(); w.options.callback('fixture-proof'); w.options[event]();
    assert.equal(w.controller.consume(), ''); assert.equal(w.resets, 0);
  }
});
test('five-minute expiry is enforced even before a delayed callback', () => {
  const now = Date.now; let clock = 1000; Date.now = () => clock;
  try { const w = widget(); w.options.callback('fixture-proof'); clock += 300000; assert.equal(w.controller.consume(), ''); assert.equal(w.events.at(-1).status, 'expired'); }
  finally { Date.now = now; }
});
test('disposed widgets ignore all late callbacks and remove only once', () => {
  const w = widget(); w.controller.dispose(); w.controller.dispose(); const count = w.events.length;
  w.options.callback('late'); w.options['error-callback']('110200');
  assert.equal(w.events.length, count); assert.equal(w.removed, 1); assert.equal(w.controller.consume(), '');
});
test('narrow forms use compact widgets without clipping the challenge', () => {
  assert.equal(widget(248).options.size, 'compact'); assert.equal(widget(480).options.size, 'flexible');
});
test('network and cache failures have actionable messages', () => {
  const { challengeError } = moduleWith();
  assert.match(challengeError('200100', 'console.test'), /缓存/);
  assert.match(challengeError('200500', 'console.test'), /challenges.cloudflare.com/);
  assert.match(challengeError('600010', 'console.test'), /600010/);
});
function scriptFixture() {
  let current; const scripts = []; const timers = new Set(); const win = { location: { hostname: 'console.test' } };
  const doc = { querySelector: () => current, createElement: () => {
    const listeners = new Map(); const node = { dataset: {}, attributes: {}, addEventListener: (n, fn) => listeners.set(n, fn), removeEventListener: n => listeners.delete(n), setAttribute: (k,v) => node.attributes[k] = v, remove: () => { if (current === node) current = null; }, fire: n => listeners.get(n)?.() };
    return node;
  }, head: { appendChild: node => { current = node; scripts.push(node); } } };
  return { win, scripts, timers, module: moduleWith({ window: win, document: doc, setTimeout: fn => { timers.add(fn); return fn; }, clearTimeout: fn => timers.delete(fn) }) };
}
test('concurrent loaders share one script; network failure can be retried', async () => {
  const f = scriptFixture(); const first = f.module.loadProviderTurnstile();
  assert.equal(f.module.loadProviderTurnstile(), first); assert.equal(f.scripts.length, 1);
  assert.equal(f.scripts[0].attributes['data-cfasync'], 'false');
  f.scripts[0].fire('error'); await assert.rejects(first, /加载失败/);
  const retry = f.module.loadProviderTurnstile(); assert.equal(f.scripts.length, 2);
  f.win.turnstile = {}; f.scripts[1].fire('load');
  assert.equal(await retry, f.win.turnstile); assert.equal(f.timers.size, 0);
});
test('stalled scripts time out instead of leaving the form pending forever', async () => {
  const f = scriptFixture(); const pending = f.module.loadProviderTurnstile();
  [...f.timers][0](); await assert.rejects(pending, /超时/);
  const retry = f.module.loadProviderTurnstile(); assert.equal(f.scripts.length, 2);
  f.win.turnstile = {}; f.scripts[1].fire('load'); await retry;
});
