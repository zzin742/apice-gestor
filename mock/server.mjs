// Mock local da API do Ápice Gestor — serve docs/contrato-api.md (v1) sem dependências.
// Uso: npm run mock   (MOCK_PORT=8787, MOCK_NOW=2026-10-08T13:02 opcionais)
import http from 'node:http';
import { createHash, randomBytes } from 'node:crypto';
import { PEOPLE, TOKENS, CODES, now, spDate, spInstant, buildTasks, businessHoursBetween, robotRun } from './data.mjs';

const PORT = Number(process.env.MOCK_PORT || 8787);
const HOST = process.env.MOCK_HOST || '127.0.0.1';
const API = 1;
const STALE_AFTER_BUSINESS_HOURS = 6; // horas úteis, seg–sex 08–18 (§3.5)
const CONFIG_VERSION = 'c-mock-1';

// Estado em memória (some ao reiniciar): tokens emitidos por enroll e últimos heartbeats.
const issued = new Map();   // token → person key
const heartbeats = new Map(); // person/token → último corpo

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
};
const STATUS = {
  invalid_request: 400, invalid_code: 400, code_expired: 410, code_used: 409, unauthorized: 401,
  token_revoked: 401, excluded_account: 403, person_not_mapped: 409, upgrade_required: 426,
  rate_limited: 429, unavailable: 503, not_found: 404,
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
    RETRY[code] ? { 'Retry-After': String(RETRY[code]) } : {});
}

// Responde 304 se o ETag bater; o ETag ignora `generated_at`, que muda a cada chamada.
function sendCached(req, res, body, stableBody) {
  const etag = '"' + createHash('sha1').update(JSON.stringify(stableBody)).digest('hex').slice(0, 20) + '"';
  if (req.headers['if-none-match'] === etag) return send(res, 304, undefined, { ETag: etag });
  send(res, 200, body, { ETag: etag, 'Cache-Control': 'no-cache' });
}

async function readJson(req) {
  let raw = '';
  for await (const chunk of req) { raw += chunk; if (raw.length > 64_000) return null; }
  try { return raw ? JSON.parse(raw) : null; } catch { return null; }
}

function auth(req, res) {
  const m = /^Bearer (\S+)$/.exec(req.headers.authorization || '');
  if (!m) { fail(res, 'unauthorized'); return null; }
  if (issued.has(m[1])) return { token: m[1], key: issued.get(m[1]) };
  const t = TOKENS[m[1]];
  if (!t) { fail(res, 'unauthorized'); return null; }
  if (t.kind !== 'ok') { fail(res, t.kind); return null; }
  return { token: m[1], key: t.person };
}

function personOut(p) {
  return { id: p.id, display_name: p.display_name, first_name: p.first_name, department: p.department };
}

function lastSync(p, n) {
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
  const tasks = buildTasks(p, today);
  const synced = lastSync(p, n);
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
  return { stable, body: { api: API, generated_at: spInstant(n), ...stable } };
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
    poll_interval_seconds: 600,
    heartbeat_interval_seconds: 900,
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
    issued.set(token, c.person);
    send(res, 201, {
      api: API, token,
      device_id: 'dev_' + createHash('sha1').update(token).digest('hex').slice(0, 12),
      person: personOut(PEOPLE[c.person]),
    });
  },

  'GET /api/gestor/me/tasks': (req, res) => {
    const a = auth(req, res); if (!a) return;
    const { stable, body } = tasksBody(a.key);
    sendCached(req, res, body, stable);
  },

  'GET /api/gestor/me/config': (req, res) => {
    const a = auth(req, res); if (!a) return;
    const body = configBody(a.key);
    sendCached(req, res, body, body);
  },

  'POST /api/gestor/heartbeat': async (req, res) => {
    const a = auth(req, res); if (!a) return;
    const body = await readJson(req);
    if (!body || typeof body.agent_version !== 'string') return fail(res, 'invalid_request');
    heartbeats.set(a.key, { ...body, _received: spInstant(now()) });
    send(res, 200, { api: API, received_at: spInstant(now()), config_version: CONFIG_VERSION });
  },

  // Só do mock: inspeção rápida. Não existe no contrato.
  'GET /__mock': (req, res) => send(res, 200, {
    tokens: Object.keys(TOKENS), codes: Object.keys(CODES),
    heartbeats: Object.fromEntries(heartbeats), now: spInstant(now()),
  }),
};

const server = http.createServer(async (req, res) => {
  // CORS aberto: a interface roda em http://localhost:* (navegador) e tauri://localhost (casca).
  res.setHeader('Access-Control-Allow-Origin', req.headers.origin || '*');
  res.setHeader('Access-Control-Allow-Headers', 'Authorization, Content-Type, If-None-Match, X-Gestor-Api');
  res.setHeader('Access-Control-Allow-Methods', 'GET, POST, OPTIONS');
  res.setHeader('Access-Control-Expose-Headers', 'ETag, X-Gestor-Api, Retry-After');
  if (req.method === 'OPTIONS') { res.writeHead(204); return res.end(); }

  const path = req.url.split('?')[0].replace(/\/+$/, '');
  const handler = routes[`${req.method} ${path}`];
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
});
