// Confere o mock contra o contrato: as 4 rotas, os cenários e os erros. Uso: npm run mock:smoke
// (sobe o servidor sozinho numa porta livre, roda e derruba). Relógio padrão: quinta 08/10 13:30;
// MOCK_NOW=... testa outro horário.
process.env.MOCK_NOW ||= '2026-10-08T13:30';
import { spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import assert from 'node:assert/strict';
import { mkdtempSync, rmSync, statSync, readFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { createClient, tabsOf, GestorApiError, commandDecision } from '../src/api/client.js';
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

const servers = [];
const stateDir = mkdtempSync(join(tmpdir(), 'gestor-mock-smoke-')); // nunca o ~/.apice-gestor-mock de verdade
async function start(mockNow) {
  const port = 18787 + Math.floor(Math.random() * 20000);
  const env = { ...process.env, MOCK_PORT: String(port), MOCK_NOW: mockNow, MOCK_HOST: '127.0.0.1', MOCK_STATE_DIR: stateDir };
  const srv = spawn(process.execPath, [fileURLToPath(new URL('./server.mjs', import.meta.url))], { env, stdio: ['ignore', 'pipe', 'inherit'] });
  servers.push(srv);
  await new Promise((ok) => srv.stdout.once('data', ok));
  return `http://127.0.0.1:${port}/api/gestor`;
}
const base = await start(process.env.MOCK_NOW);
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
  const syncedToday = d.sync.last_sync_at.slice(0, 10) === d.today;
  if (syncedToday) { assert.equal(d.summary.overdue + d.summary.due_today, 15); assert.equal(d.summary.open_total, 146); }
  assert.equal(d.sync.stale, false);
  assert.equal(d.today, spDate(now())); // vencida/hoje relativos ao momento da consulta (§3.1)
  assert.match(d.sync.last_sync_at, /T(07|10|13|16):04:00-03:00$/);
  assert.equal(d.sync.stale_after_business_hours, 6);
  for (const t of d.tasks) {
    for (const k of ['id', 'title', 'client', 'department', 'status', 'bucket', 'gestta_url']) assert.ok(t[k], `${k} em ${t.id}`);
    if (t.bucket === 'overdue') assert.ok(t.due_date < d.today && t.days_overdue > 0);
    if (t.bucket === 'today') assert.equal(t.due_date, d.today);
    if (t.status === 'done') { assert.equal(t.done_date, d.today); assert.equal(t.done_at, null); }
    else assert.equal(t.done_date, null);
    assert.equal(t.due_time, null);
    assert.match(t.gestta_url, /^https:\/\/app\.gestta\.com\.br\/#\/sidebar\/task\/\d+$/);
  }
  const tabs = tabsOf(d);
  assert.equal(tabs.hoje.overdue.length + tabs.hoje.today.length, d.summary.today_total);
  ok(`me/tasks: regras de bucket${syncedToday ? ', Rafael 15 em vencidas+hoje e 146 abertas' : ' (sync de outro dia: contagens livres)'}`);
  const again = await c.getTasks(); assert.deepEqual(again.tasks, d.tasks); ok('me/tasks: ETag/304 reaproveita o corpo');

  // ETag é por pessoa e a resposta é privada (§1.3): o ETag do Caio não serve para o Rafael.
  for (const path of ['/me/tasks', '/me/config']) {
    const h = (t, extra = {}) => ({ headers: { Authorization: `Bearer ${t}`, ...extra } });
    const rc = await fetch(base + path, h('gdt_mock_caio'));
    const rr = await fetch(base + path, h('gdt_mock_rafael', { 'If-None-Match': rc.headers.get('ETag') }));
    assert.equal(rr.status, 200, `${path}: ETag de outra pessoa não pode dar 304`);
    assert.notEqual(rr.headers.get('ETag'), rc.headers.get('ETag'));
    assert.match(rr.headers.get('Cache-Control'), /private/);
    assert.match(rr.headers.get('Vary'), /Authorization/);
    const r304 = await fetch(base + path, h('gdt_mock_rafael', { 'If-None-Match': rr.headers.get('ETag') }));
    assert.equal(r304.status, 304); assert.match(r304.headers.get('Vary') || '', /Authorization/);
  }
  ok('ETag por pessoa; Cache-Control private + Vary: Authorization');

  const big = await api('gdt_mock_bianca').getTasks(); assert.ok(big.summary.overdue > 100); ok('cenário pesado (>100 vencidas)');
  const old = await api('gdt_mock_denise').getTasks(); assert.equal(old.sync.stale, true); ok('cenário dado velho');
  const zero = await api('gdt_mock_caio').getTasks(); assert.equal(zero.summary.overdue + zero.summary.due_today, 0); ok('cenário dia limpo');

  // config
  const cfg = await c.getConfig(); assert.equal(cfg.default_tab, 'hoje'); assert.deepEqual(cfg.sync_times, ['07:00', '10:00', '13:00', '16:00']); assert.equal(cfg.paused, false);
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
  ok('erros de token (§1.4)');

  // Token emitido sobrevive a reinício do mock; o arquivo guarda só o hash, com permissão 600.
  const fresh = await start(process.env.MOCK_NOW);
  const after = await createClient({ baseUrl: fresh, token: e.token }).getTasks();
  assert.equal(after.person.id, 'pes_rafael');
  const file = join(stateDir, 'issued.json'), raw = readFileSync(file, 'utf8');
  assert.ok(!raw.includes(e.token) && !raw.includes('gdt_'), 'o arquivo não pode conter o token');
  assert.equal(statSync(file).mode & 0o777, 0o600);
  ok('token do enroll sobrevive a reinício (só SHA-256 em disco, 600)');

  // Reenrolar a mesma máquina revoga o token anterior (§1.2).
  const dev = { hostname: 'APICE-PES-014', os: 'Windows 11', machine_id: 'mg-reenrola' };
  const t1 = (await api(null).enroll('K7QM-4XPA', dev)).token;
  const t2 = (await api(null).enroll('K7QM-4XPA', dev)).token;
  await expectErr(api(t1).getTasks(), 'token_revoked');
  assert.equal((await api(t2).getTasks()).person.id, 'pes_rafael');
  ok('reenrolar a mesma máquina revoga o token anterior');

  // ---------- Ordens ao dispositivo (§8) ----------
  const root = base.replace('/api/gestor', '');
  const admin = async (path, body) => (await fetch(root + path, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(body) })).json();
  const raf = api('gdt_mock_rafael'), bia = api('gdt_mock_bianca');
  assert.equal((await raf.getConfig()).commands_poll_interval_seconds, 60);
  assert.deepEqual((await raf.getCommands()).commands, []); ok('me/commands vazio é o normal; config com commands_poll_interval_seconds');

  const c1 = await admin('/__mock/commands', { device_id: 'dev_mock_rafael' });
  const again1 = await admin('/__mock/commands', { device_id: 'dev_mock_rafael' });
  assert.equal(again1.id, c1.id); ok('mandar duas vezes = a mesma ordem (aceite 4)');
  const rc = await fetch(base + '/me/commands', { headers: { Authorization: 'Bearer gdt_mock_rafael' } });
  assert.match(rc.headers.get('Cache-Control'), /no-store/); assert.equal(rc.headers.get('ETag'), null);
  const listed = (await rc.json()).commands;
  assert.equal(listed.length, 1); assert.equal(listed[0].type, 'shutdown');
  assert.ok(listed[0].age_seconds >= 0 && listed[0].expires_in_seconds <= 300);
  assert.deepEqual((await bia.getCommands()).commands, []); // ordem é por dispositivo
  await expectErr(bia.claimCommand(c1.id), 'command_gone');
  ok('ordem só aparece e só é reivindicável pelo próprio dispositivo; no-store, sem ETag');

  const cl = await raf.claimCommand(c1.id); assert.equal(cl.id, c1.id);
  await expectErr(raf.claimCommand(c1.id), 'command_gone');
  assert.deepEqual((await raf.getCommands()).commands, []);
  ok('claim atômico: 200 uma vez, depois 409 command_gone');

  // Heartbeat fecha como executed só se o Windows ligou depois do claim (§8.5).
  const hbBase = { agent_version: '0.2.0', os: 'Windows 11', last_command_id: c1.id };
  await raf.heartbeat({ ...hbBase, uptime_seconds: 9999 });
  const state = async (id) => (await (await fetch(root + '/__mock')).json()).commands.find((c) => c.id === id).state;
  assert.equal(await state(c1.id), 'claimed', 'uptime maior que o tempo desde o claim: não fecha');
  await admin('/__mock/clock', { advance_seconds: 120 });
  await raf.heartbeat({ ...hbBase, uptime_seconds: 40 });
  assert.equal(await state(c1.id), 'executed'); ok('heartbeat pós-boot fecha como executed; sem boot, não');

  // Falha: result failed fecha; repetir dá 409.
  const c2 = await admin('/__mock/commands', { device_id: 'dev_mock_rafael' });
  await raf.claimCommand(c2.id);
  assert.equal((await raf.reportCommandFailed(c2.id, 'shutdown.exe 1190')).state, 'failed');
  await expectErr(raf.reportCommandFailed(c2.id, 'de novo'), 'command_gone');
  await expectErr(raf.reportCommandFailed('cmd_naoexiste', 'x'), 'command_gone');
  ok('result failed fecha a ordem uma vez');

  // Vencimento: 5 min sem claim, nunca mais entregue (aceite 3).
  const c3 = await admin('/__mock/commands', { device_id: 'dev_mock_rafael' });
  await admin('/__mock/clock', { advance_seconds: 301 });
  assert.deepEqual((await raf.getCommands()).commands, []);
  await expectErr(raf.claimCommand(c3.id), 'command_gone');
  assert.equal(await state(c3.id), 'expired');
  const c4 = await admin('/__mock/commands', { device_id: 'dev_mock_rafael', age_seconds: 400 });
  assert.equal(c4.state, 'expired'); ok('ordem vence em 5 min e o claim também respeita');

  // Rede caída no claim (aceite 5): o claim chega, a resposta se perde → cliente rejeita, ordem não volta.
  const c5 = await admin('/__mock/commands', { device_id: 'dev_mock_rafael', fail_claim: 'network' });
  await expectErr(raf.claimCommand(c5.id), 'network');
  assert.deepEqual((await raf.getCommands()).commands, []); assert.equal(await state(c5.id), 'claimed');
  const c6 = await admin('/__mock/commands', { device_id: 'dev_mock_rafael', fail_claim: 'server_error' });
  await expectErr(raf.claimCommand(c6.id), 'server_error');
  ok('claim sem 200 (rede/500) rejeita e a ordem não é reentregue');

  // Travas antes do claim (§8.6): função pura do cliente.
  const executed = new Set(['cmd_ja']);
  const decide = (cmd, up) => commandDecision({ type: 'shutdown', age_seconds: 10, ...cmd }, { processUptimeSeconds: up, executedIds: executed });
  assert.equal(decide({ id: 'cmd_ok' }, 60), 'claim');
  assert.equal(decide({ id: 'cmd_velha', age_seconds: 200 }, 60), 'ignore:before_start');   // trava b: ligou o PC depois da ordem
  assert.equal(decide({ id: 'cmd_ja' }, 60), 'ignore:already_executed');                    // trava c
  assert.equal(decide({ id: 'cmd_x', type: 'reboot' }, 60), 'ignore:unknown_type');
  assert.equal(decide({ id: 'cmd_nan', age_seconds: undefined }, 60), 'ignore:before_start');
  ok('travas b e c e tipo desconhecido (commandDecision)');

  // Virada do dia sem sync (§3 ETag, §3.1): mesma carteira, buckets novos, ETag diferente.
  const before = api('gdt_mock_rafael'), dayEnd = await before.getTasks();
  const b2 = await start('2026-10-08T23:59'), b3 = await start('2026-10-09T00:01');
  const c23 = createClient({ baseUrl: b2, token: 'gdt_mock_rafael' }), d23 = await c23.getTasks();
  const r23 = await fetch(b2 + '/me/tasks', { headers: { Authorization: 'Bearer gdt_mock_rafael' } });
  const etag = r23.headers.get('ETag');
  const r01 = await fetch(b3 + '/me/tasks', { headers: { Authorization: 'Bearer gdt_mock_rafael', 'If-None-Match': etag } });
  assert.equal(r01.status, 200, 'à 00:01 tem que vir 200, não 304');
  const d01 = await r01.json();
  assert.equal(d01.sync.last_sync_at, d23.sync.last_sync_at, 'sem sync novo');
  assert.equal(d01.summary.overdue, d23.summary.overdue + d23.summary.due_today, 'o hoje de ontem venceu');
  assert.equal(d01.summary.done_today, 0, 'concluídas de ontem saem');
  ok('00:01 sem sync: 200, hoje de ontem virou vencida, ETag mudou');
  void dayEnd;

  console.log(`\nMock OK — ${n} verificações.`);
} finally { servers.forEach((s) => s.kill()); rmSync(stateDir, { recursive: true, force: true }); }
