// Mock local da API do Ápice Gestor — serve docs/contrato-api.md (v1) sem dependências.
// Uso: npm run mock   (MOCK_PORT=8787, MOCK_HOST, MOCK_NOW=2026-10-08T13:02, MOCK_STATE_DIR opcionais)
import http from 'node:http';
import { createHash, randomBytes } from 'node:crypto';
import { mkdirSync, readFileSync, writeFileSync, renameSync } from 'node:fs';
import { homedir } from 'node:os';
import { join } from 'node:path';
import { PEOPLE, TOKENS, CODES, now, spDate, spInstant, buildTasks, classify, businessHoursBetween, robotRun, ROBOT_RUNS } from './data.mjs';

const PORT = Number(process.env.MOCK_PORT || 8787);
const HOST = process.env.MOCK_HOST || '127.0.0.1';
const API = 1;
const STALE_AFTER_BUSINESS_HOURS = 6; // horas úteis, seg–sex 08–18 (§3.5)
const CONFIG_VERSION = 'c-mock-1';
const DEPARA_VERSION = 'dp-mock-1'; // versão do de-para dispositivo → pessoa → nome no Gestta

// Tokens emitidos pelo enroll sobrevivem a reinício: ficam FORA do git, em MOCK_STATE_DIR
// (padrão ~/.apice-gestor-mock/issued.json). Como no Hub (§1.2), guarda-se só o SHA-256 do token.
const STATE_DIR = process.env.MOCK_STATE_DIR || join(homedir(), '.apice-gestor-mock');
const STATE_FILE = join(STATE_DIR, 'issued.json');
const sha256 = (t) => createHash('sha256').update(t).digest('hex');
const issued = new Map(); // sha256(token) → { person, device_id, hostname, enrolled_at }
try {
  for (const [h, v] of Object.entries(JSON.parse(readFileSync(STATE_FILE, 'utf8')))) issued.set(h, v);
} catch (e) { if (e.code !== 'ENOENT') console.error(`Estado ilegível em ${STATE_FILE}: ${e.message}`); }

function saveIssued() {
  mkdirSync(STATE_DIR, { recursive: true, mode: 0o700 });
  const tmp = STATE_FILE + '.tmp';
  writeFileSync(tmp, JSON.stringify(Object.fromEntries(issued), null, 2), { mode: 0o600 });
  renameSync(tmp, STATE_FILE);
}

const heartbeats = new Map(); // pessoa → último corpo (só memória; é telemetria)

// ---------- Ordens ao dispositivo (§8) — só memória ----------
const COMMAND_TTL_S = 300; // vence em 5 min sem claim (§8.3)
const commands = new Map(); // id → { id, device_id, type, created_ms, state, claimed_ms, closed_ms, error, fail_claim }
let clockOffsetMs = 0; // só para teste: /__mock/clock avança o relógio das ordens
const nowMs = () => now().getTime() + clockOffsetMs;

function expireStale() {
  const t = nowMs();
  for (const c of commands.values())
    if (c.state === 'pending' && t - c.created_ms >= COMMAND_TTL_S * 1000) { c.state = 'expired'; c.closed_ms = c.created_ms + COMMAND_TTL_S * 1000; }
}
function commandOut(c) {
  const age = Math.floor((nowMs() - c.created_ms) / 1000);
  return { id: c.id, type: c.type, age_seconds: age, expires_in_seconds: Math.max(0, COMMAND_TTL_S - age) };
}
function commandView(c) { // para /__mock
  const iso = (ms) => (ms ? spInstant(new Date(ms)) : null);
  return { id: c.id, device_id: c.device_id, type: c.type, state: c.state, created_at: iso(c.created_ms),
    claimed_at: iso(c.claimed_ms), closed_at: iso(c.closed_ms), error: c.error ?? null, fail_claim: c.fail_claim ?? null };
}
// Ordem nunca sai de cache (§8.2).
const NO_STORE = { 'Cache-Control': 'private, no-store', Vary: 'Authorization, Origin' };
const isLoopback = (req) => ['127.0.0.1', '::1', '::ffff:127.0.0.1'].includes(req.socket.remoteAddress);

const MESSAGES = {
  invalid_request: 'Requisição inválida.',
  invalid_code: 'Código de enrolamento não encontrado. Confira com o administrador.',
  code_expired: 'Este código expirou. Peça um novo ao administrador.',
  code_used: 'Este código já foi usado. Peça um novo ao administrador.',
  unauthorized: 'Este computador não está conectado ao Ápice Hub.',
  token_revoked: 'Este computador foi desconectado do Ápice Hub.',
  excluded_account: 'Esta conta não recebe o painel de tarefas.',
  person_not_mapped: 'Computador conectado. Aguardando o administrador vincular seu nome no Gestta.',
  upgrade_required: 'Há uma versão nova do Ápice Gestor. Atualizando…',
  rate_limited: 'Muitas tentativas. Aguarde um instante.',
  unavailable: 'Ápice Hub indisponível no momento.',
  not_found: 'Rota não encontrada.',
  command_gone: 'Esta ordem não está mais disponível.',
  forbidden: 'Só do próprio computador do mock.',
};
const STATUS = {
  invalid_request: 400, invalid_code: 400, code_expired: 410, code_used: 409, unauthorized: 401,
  token_revoked: 401, excluded_account: 403, person_not_mapped: 409, upgrade_required: 426,
  rate_limited: 429, unavailable: 503, not_found: 404, command_gone: 409, forbidden: 403,
};
const RETRY = { person_not_mapped: 3600, rate_limited: 30, unavailable: 60 };

function send(res, status, body, headers = {}) {
  const h = { 'X-Gestor-Api': String(API), ...headers };
  if (body === undefined) { res.writeHead(status, h); return res.end(); }
  const json = JSON.stringify(body, null, 2);
  res.writeHead(status, { 'Content-Type': 'application/json; charset=utf-8', ...h });
  res.end(json);
}

function fail(res, code) {
  send(res, STATUS[code], { api: API, error: { code, message: MESSAGES[code], retry_after_seconds: RETRY[code] ?? null } },
    { ...PRIVATE, ...(RETRY[code] ? { 'Retry-After': String(RETRY[code]) } : {}) });
}

// Toda resposta autenticada é da pessoa do token: nenhum cache pode servi-la a outro token (§1.3).
const PRIVATE = { 'Cache-Control': 'private, no-cache', Vary: 'Authorization, Origin' };

// Responde 304 se o ETag bater. `parts` é o que define o ETag e começa SEMPRE pela pessoa (§1.3).
function sendCached(req, res, body, parts) {
  const etag = '"' + createHash('sha256').update(JSON.stringify(parts)).digest('hex').slice(0, 20) + '"';
  if (req.headers['if-none-match'] === etag) return send(res, 304, undefined, { ETag: etag, ...PRIVATE });
  send(res, 200, body, { ETag: etag, ...PRIVATE });
}

async function readJson(req) {
  let raw = '';
  for await (const chunk of req) { raw += chunk; if (raw.length > 64_000) return null; }
  try { return raw ? JSON.parse(raw) : null; } catch { return null; }
}

function auth(req, res) {
  const m = /^Bearer (\S+)$/.exec(req.headers.authorization || '');
  if (!m) { fail(res, 'unauthorized'); return null; }
  const known = issued.get(sha256(m[1]));
  if (known) return { token: m[1], key: known.person, device: known.device_id };
  const t = TOKENS[m[1]];
  if (!t) { fail(res, 'unauthorized'); return null; }
  if (t.kind !== 'ok') { fail(res, t.kind); return null; }
  return { token: m[1], key: t.person, device: 'dev_mock_' + t.person };
}

function personOut(p) {
  return { id: p.id, display_name: p.display_name, first_name: p.first_name, department: p.department };
}

function lastSync(p, n) {
  // No Hub, last_sync_at é o max(atualizado_em) GLOBAL — um sync vale para todos (§3.1). No mock,
  // o cenário "velho" vive num token só para dar para testar sem derrubar os outros.
  // Rodadas fixas às 07, 10, 13 e 16h. Normal: a última que já passou.
  // Cenário "velho": o robô parou, como em 05/10 — a última rodada que deu certo fica a mais de
  // 2× o limite em horas úteis (contar rodadas não basta: segunda cedo cairia no fim de semana).
  if (!p.stale) return robotRun(n, 0);
  for (let k = 1; ; k++) {
    const t = robotRun(n, k);
    if (businessHoursBetween(t, n) > 2 * STALE_AFTER_BUSINESS_HOURS) return t;
  }
}

function tasksBody(key) {
  const p = PEOPLE[key];
  const n = now();
  const today = spDate(n);
  const synced = lastSync(p, n);
  // A carteira é a do último sync; os buckets são de agora. À 00:01, sem sync novo, o "hoje" de
  // ontem vira vencida e as concluídas de ontem saem do cartão.
  const tasks = classify(buildTasks(p, spDate(synced)), today);
  const count = (f) => tasks.filter(f).length;
  const summary = {
    overdue: count((t) => t.bucket === 'overdue'),
    due_today: count((t) => t.bucket === 'today' && t.status !== 'done'),
    done_today: count((t) => t.status === 'done'),
    today_total: 0,
    week: count((t) => t.bucket === 'week'),
    open_total: count((t) => t.status !== 'done'),
  };
  summary.today_total = summary.overdue + summary.due_today + summary.done_today;
  const stable = {
    api: API, today, person: personOut(p),
    sync: {
      source: 'gestta',
      last_sync_at: spInstant(synced),
      stale: businessHoursBetween(synced, n) > STALE_AFTER_BUSINESS_HOURS,
      stale_after_business_hours: STALE_AFTER_BUSINESS_HOURS,
    },
    summary, tasks, truncated: false,
  };
  // ETag = hash(pessoa, today, max(atualizado_em) global, versão do de-para, config_version) — §3.
  const etagParts = [p.id, today, stable.sync.last_sync_at, DEPARA_VERSION, CONFIG_VERSION];
  return { etagParts, body: { api: API, generated_at: spInstant(n), ...stable } };
}

function configBody(key) {
  const p = PEOPLE[key];
  return {
    api: API,
    config_version: CONFIG_VERSION,
    paused: !!p.paused,
    tabs: ['hoje', 'semana', 'todas'],
    default_tab: 'hoje',
    morning: { enabled: true, duration_seconds: 10, once_per_day: true, not_before: '07:00' },
    stale_after_business_hours: STALE_AFTER_BUSINESS_HOURS,
    sync_times: ROBOT_RUNS.map((h) => `${String(h).padStart(2, '0')}:00`),
    poll_interval_seconds: 600,
    heartbeat_interval_seconds: 900,
    commands_poll_interval_seconds: 60,
    notice: p.stale ? { level: 'warn', text: 'A sincronização com o Gestta está atrasada. Os dados podem não refletir baixas recentes.' } : null,
    update: { min_agent_version: '0.1.0', force: false },
  };
}

const routes = {
  'POST /api/gestor/enroll': async (req, res) => {
    const body = await readJson(req);
    const raw = body?.code;
    if (typeof raw !== 'string' || typeof body.device !== 'object' || !body.device) return fail(res, 'invalid_request');
    const code = raw.replace(/[\s-]/g, '').toUpperCase();
    if (!/^[A-HJ-NP-Z2-9]{8}$/.test(code)) return fail(res, 'invalid_code');
    const c = CODES[code];
    if (!c) return fail(res, 'invalid_code');
    if (c.kind !== 'ok') return fail(res, c.kind);
    const token = 'gdt_' + randomBytes(32).toString('base64url');
    const device_id = 'dev_' + sha256(token).slice(0, 12);
    issued.set(sha256(token), {
      person: c.person, device_id, hostname: String(body.device.hostname ?? ''), enrolled_at: spInstant(now()),
    });
    saveIssued();
    send(res, 201, {
      api: API, token,
      device_id,
      person: personOut(PEOPLE[c.person]),
    }, { 'Cache-Control': 'no-store' });
  },

  'GET /api/gestor/me/tasks': (req, res) => {
    const a = auth(req, res); if (!a) return;
    const { etagParts, body } = tasksBody(a.key);
    sendCached(req, res, body, etagParts);
  },

  'GET /api/gestor/me/config': (req, res) => {
    const a = auth(req, res); if (!a) return;
    const body = configBody(a.key);
    sendCached(req, res, body, [PEOPLE[a.key].id, body]);
  },

  'POST /api/gestor/heartbeat': async (req, res) => {
    const a = auth(req, res); if (!a) return;
    const body = await readJson(req);
    if (!body || typeof body.agent_version !== 'string') return fail(res, 'invalid_request');
    heartbeats.set(a.key, { ...body, _received: spInstant(now()) });
    // §8.5: fecha como executed se a ordem foi reivindicada por este dispositivo e o Windows ligou depois.
    const c = typeof body.last_command_id === 'string' && commands.get(body.last_command_id);
    if (c && c.device_id === a.device && c.state === 'claimed' && Number.isFinite(body.uptime_seconds)
        && body.uptime_seconds * 1000 < nowMs() - c.claimed_ms) { c.state = 'executed'; c.closed_ms = nowMs(); }
    send(res, 200, { api: API, received_at: spInstant(now()), config_version: CONFIG_VERSION }, PRIVATE);
  },

  'GET /api/gestor/me/commands': (req, res) => {
    const a = auth(req, res); if (!a) return;
    expireStale();
    const list = [...commands.values()].filter((c) => c.device_id === a.device && c.state === 'pending')
      .sort((x, y) => x.created_ms - y.created_ms).map(commandOut);
    send(res, 200, { api: API, commands: list }, NO_STORE);
  },

  // Só do mock: inspeção rápida. Não existe no contrato.
  'GET /__mock': (req, res) => { expireStale(); send(res, 200, {
    tokens: Object.keys(TOKENS), codes: Object.keys(CODES),
    enrolled: [...issued.values()], // sem token: só pessoa, dispositivo e hostname
    heartbeats: Object.fromEntries(heartbeats), now: spInstant(now()),
    commands: [...commands.values()].map(commandView),
  }); },

  // Admin do mock (faz o papel de /admin/maquinas). SÓ do próprio Mac: o mock da bancada escuta na
  // rede, e ninguém da rede pode mandar desligar o PC do Vinny por aqui.
  //   POST /__mock/commands {device_id, type?, age_seconds?, fail_claim?: 'network'|'server_error'}
  'POST /__mock/commands': async (req, res) => {
    if (!isLoopback(req)) return fail(res, 'forbidden');
    const b = await readJson(req);
    if (!b || typeof b.device_id !== 'string') return fail(res, 'invalid_request');
    const type = b.type ?? 'shutdown';
    expireStale();
    // No máximo uma shutdown pendente por dispositivo: pedir de novo devolve a mesma (§8.3).
    const dup = [...commands.values()].find((c) => c.device_id === b.device_id && c.type === type && c.state === 'pending');
    if (dup) return send(res, 200, commandView(dup));
    const c = { id: 'cmd_' + randomBytes(9).toString('base64url'), device_id: b.device_id, type, state: 'pending',
      created_ms: nowMs() - 1000 * (Number(b.age_seconds) || 0), fail_claim: b.fail_claim ?? null };
    commands.set(c.id, c);
    expireStale(); // age_seconds ≥ 300 já nasce vencida
    send(res, 201, commandView(c));
  },
  //   POST /__mock/clock {advance_seconds} — avança o relógio das ordens (vencimento, fechamento)
  'POST /__mock/clock': async (req, res) => {
    if (!isLoopback(req)) return fail(res, 'forbidden');
    const b = await readJson(req);
    clockOffsetMs += 1000 * (Number(b?.advance_seconds) || 0);
    send(res, 200, { now: spInstant(new Date(nowMs())), offset_seconds: clockOffsetMs / 1000 });
  },
};

// Rotas com id no caminho (§8.2).
const paramRoutes = [
  [/^POST \/api\/gestor\/me\/commands\/([^/]+)\/claim$/, async (req, res, id) => {
    const a = auth(req, res); if (!a) return;
    expireStale();
    const c = commands.get(id);
    if (!c || c.device_id !== a.device || c.state !== 'pending') return send(res, 409,
      { api: API, error: { code: 'command_gone', message: MESSAGES.command_gone, retry_after_seconds: null } }, NO_STORE);
    c.state = 'claimed'; c.claimed_ms = nowMs(); // atômico: Node é um laço só, nada entra entre o teste e a escrita
    if (c.fail_claim === 'network') return req.socket.destroy();  // claim chegou, resposta se perdeu
    if (c.fail_claim === 'server_error') return send(res, 500,
      { api: API, error: { code: 'server_error', message: 'Erro no Ápice Hub.', retry_after_seconds: 60 } }, NO_STORE);
    send(res, 200, { api: API, id: c.id, type: c.type, claimed_at: spInstant(new Date(c.claimed_ms)) }, NO_STORE);
  }],
  [/^POST \/api\/gestor\/me\/commands\/([^/]+)\/result$/, async (req, res, id) => {
    const a = auth(req, res); if (!a) return;
    const b = await readJson(req);
    if (!b || b.status !== 'failed' || typeof b.error !== 'string' || b.error.length > 500) return fail(res, 'invalid_request');
    const c = commands.get(id);
    if (!c || c.device_id !== a.device || c.state !== 'claimed') return send(res, 409,
      { api: API, error: { code: 'command_gone', message: MESSAGES.command_gone, retry_after_seconds: null } }, NO_STORE);
    c.state = 'failed'; c.error = b.error; c.closed_ms = nowMs();
    send(res, 200, { api: API, id: c.id, state: 'failed' }, NO_STORE);
  }],
];

const server = http.createServer(async (req, res) => {
  // CORS aberto: a interface roda em http://localhost:* (navegador) e tauri://localhost (casca).
  res.setHeader('Access-Control-Allow-Origin', req.headers.origin || '*');
  res.setHeader('Access-Control-Allow-Headers', 'Authorization, Content-Type, If-None-Match, X-Gestor-Api');
  res.setHeader('Access-Control-Allow-Methods', 'GET, POST, OPTIONS');
  res.setHeader('Access-Control-Expose-Headers', 'ETag, X-Gestor-Api, Retry-After');
  if (req.method === 'OPTIONS') { res.writeHead(204); return res.end(); }

  const path = req.url.split('?')[0].replace(/\/+$/, '');
  let handler = routes[`${req.method} ${path}`];
  if (!handler) for (const [re, fn] of paramRoutes) {
    const m = re.exec(`${req.method} ${path}`);
    if (m) { handler = (q, r) => fn(q, r, decodeURIComponent(m[1])); break; }
  }
  const t0 = Date.now();
  try {
    if (!handler) fail(res, 'not_found');
    else await handler(req, res);
  } catch (e) {
    console.error(e);
    if (!res.headersSent) send(res, 500, { api: API, error: { code: 'server_error', message: 'Erro no Ápice Hub.', retry_after_seconds: 60 } });
  }
  // Log sem token: só método, rota, status e tempo.
  console.log(`${req.method} ${path} → ${res.statusCode} (${Date.now() - t0} ms)`);
});

server.listen(PORT, HOST, () => {
  console.log(`Mock Ápice Gestor em http://${HOST}:${PORT}/api/gestor  (hoje = ${spDate(now())})`);
  console.log('Tokens: ' + Object.keys(TOKENS).join(', '));
  console.log(`Dispositivos enrolados: ${issued.size} (estado em ${STATE_FILE})`);
});
