// Confere o mock contra o contrato: as 4 rotas, os cenários e os erros. Uso: npm run mock:smoke
// (sobe o servidor sozinho numa porta livre, roda e derruba).
import { spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import assert from 'node:assert/strict';
import { createClient, tabsOf, GestorApiError } from '../src/api/client.js';
import { businessHoursBetween, robotRun, now, spDate } from './data.mjs';

// Funções puras de §3.5, antes de subir o servidor. Datas em São Paulo (−03:00).
const sp = (s) => new Date(s + '-03:00');
assert.equal(businessHoursBetween(sp('2026-10-09T16:00'), sp('2026-10-12T09:00')), 3); // sex 16h → seg 9h
assert.ok(businessHoursBetween(sp('2026-10-06T10:00'), sp('2026-10-06T16:01')) > 6);   // ter 10h → 16h01
assert.equal(businessHoursBetween(sp('2026-10-06T18:00'), sp('2026-10-07T08:00')), 0); // a noite não conta
assert.equal(businessHoursBetween(sp('2026-10-10T08:00'), sp('2026-10-11T18:00')), 0); // fim de semana não conta
assert.equal(robotRun(sp('2026-10-08T12:59')).getTime(), sp('2026-10-08T10:04').getTime());
assert.equal(robotRun(sp('2026-10-08T06:59')).getTime(), sp('2026-10-07T16:04').getTime());
console.log('  ✓ horas úteis e rodadas 07/10/13/16h (§3.5)');

const port = 18787 + Math.floor(Math.random() * 1000);
const env = { ...process.env, MOCK_PORT: String(port) };
const srv = spawn(process.execPath, [fileURLToPath(new URL('./server.mjs', import.meta.url))], { env, stdio: ['ignore', 'pipe', 'inherit'] });
await new Promise((ok) => srv.stdout.once('data', ok));
const base = `http://127.0.0.1:${port}/api/gestor`;
const api = (token) => createClient({ baseUrl: base, token, agentVersion: '0.1.0' });
const expectErr = async (p, code) => {
  await assert.rejects(p, (e) => e instanceof GestorApiError && e.code === code, `esperava ${code}`);
};
let n = 0; const ok = (m) => console.log(`  ✓ ${m}`, ++n);

try {
  // enroll
  const e = await api().enroll('k7qm-4xpa', { hostname: 'APICE-PES-014', os: 'Windows 11', machine_id: 'x' });
  assert.match(e.token, /^gdt_[A-Za-z0-9_-]{43}$/); assert.equal(e.person.first_name, 'Rafael'); ok('enroll devolve token e pessoa');
  await expectErr(api().enroll('ZZZZ-ZZZZ', {}), 'invalid_code');
  await expectErr(api().enroll('VENC-7DQX', {}), 'code_expired');
  await expectErr(api().enroll('USAD-2222', {}), 'code_used');
  await expectErr(api().enroll('CAXA-4444', {}), 'excluded_account'); ok('enroll: erros de código');

  // tasks com o token emitido + regras de §3
  const c = api(e.token);
  const d = await c.getTasks();
  assert.equal(d.api, 1); assert.equal(d.person.display_name, 'Rafael Moura');
  assert.equal(d.summary.overdue + d.summary.due_today, 15); assert.equal(d.summary.open_total, 146);
  assert.equal(d.sync.stale, false);
  assert.equal(d.today, spDate(now())); // vencida/hoje relativos ao momento da consulta (§3.1)
  assert.match(d.sync.last_sync_at, /T(07|10|13|16):04:00-03:00$/);
  assert.equal(d.sync.stale_after_business_hours, 6);
  for (const t of d.tasks) {
    for (const k of ['id', 'title', 'client', 'department', 'status', 'bucket', 'gestta_url']) assert.ok(t[k], `${k} em ${t.id}`);
    if (t.bucket === 'overdue') assert.ok(t.due_date < d.today && t.days_overdue > 0);
    if (t.bucket === 'today') assert.equal(t.due_date, d.today);
    if (t.status === 'done') assert.ok(t.done_at);
  }
  const tabs = tabsOf(d);
  assert.equal(tabs.hoje.overdue.length + tabs.hoje.today.length, d.summary.today_total);
  ok('me/tasks: Rafael 15 em vencidas+hoje, 146 abertas, regras de bucket');
  const again = await c.getTasks(); assert.deepEqual(again.tasks, d.tasks); ok('me/tasks: ETag/304 reaproveita o corpo');

  const big = await api('gdt_mock_bianca').getTasks(); assert.ok(big.summary.overdue > 100); ok('cenário pesado (>100 vencidas)');
  const old = await api('gdt_mock_denise').getTasks(); assert.equal(old.sync.stale, true); ok('cenário dado velho');
  const zero = await api('gdt_mock_caio').getTasks(); assert.equal(zero.summary.overdue + zero.summary.due_today, 0); ok('cenário dia limpo');

  // config
  const cfg = await c.getConfig(); assert.equal(cfg.default_tab, 'hoje'); assert.equal(cfg.paused, false);
  assert.equal((await api('gdt_mock_elton').getConfig()).paused, true); ok('me/config (normal e pausado)');

  // heartbeat
  const hb = await c.heartbeat({ os: 'Windows 11', config_version: cfg.config_version, panel_opens_since_last: 1, morning_shown_at: null, last_tasks_ok_at: d.generated_at, last_error_code: null });
  assert.equal(hb.config_version, cfg.config_version); ok('heartbeat');

  // erros de token
  await expectErr(api(null).getTasks(), 'unauthorized');
  await expectErr(api('gdt_nao_existe').getTasks(), 'unauthorized');
  await expectErr(api('gdt_mock_revogado').getTasks(), 'token_revoked');
  await expectErr(api('gdt_mock_sem_depara').getTasks(), 'person_not_mapped');
  await expectErr(api('gdt_mock_caixa').getTasks(), 'excluded_account');
  await expectErr(api('gdt_mock_fora').getTasks(), 'unavailable');
  await expectErr(api('gdt_mock_velho_agente').getConfig(), 'upgrade_required');
  ok('erros de token (§1.3)');
  console.log(`\nMock OK — ${n} verificações.`);
} finally { srv.kill(); }
