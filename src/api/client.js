// Cliente HTTP do agente para a API do Ápice Hub — contrato em docs/contrato-api.md (v1).
// ES module sem dependências: funciona na casca Tauri e no navegador apontando para o mock.
//
//   import { createClient, GestorApiError } from './api/client.js';
//   const api = createClient({ baseUrl: 'http://127.0.0.1:8787/api/gestor', token, agentVersion: '0.1.0' });
//   const data = await api.getTasks();   // corpo de §3; com ETag, devolve o último corpo quando vier 304
//
// O token NÃO é guardado aqui: quem chama lê do keychain e passa. Nunca logue o token.

export const API_VERSION = 1;
export const MOCK_BASE_URL = 'http://127.0.0.1:8787/api/gestor';

export class GestorApiError extends Error {
  // code: um dos códigos de §1.4, ou 'network' quando nem chegou resposta.
  constructor(code, message, { status = 0, retryAfterSeconds = null } = {}) {
    super(message);
    this.name = 'GestorApiError';
    this.code = code;
    this.status = status;
    this.retryAfterSeconds = retryAfterSeconds;
  }
}

export function createClient({ baseUrl = MOCK_BASE_URL, token = null, agentVersion = '0.0.0', fetchImpl = globalThis.fetch } = {}) {
  const base = baseUrl.replace(/\/+$/, '');
  const cache = new Map(); // path → { etag, body }

  async function call(method, path, { body, auth = true, cached = false } = {}) {
    const headers = { 'X-Gestor-Api': String(API_VERSION), Accept: 'application/json' };
    if (body !== undefined) headers['Content-Type'] = 'application/json';
    if (auth) {
      if (!token) throw new GestorApiError('unauthorized', 'Este computador não está conectado ao Ápice Hub.', { status: 401 });
      headers.Authorization = `Bearer ${token}`;
    }
    const hit = cached && cache.get(path);
    if (hit) headers['If-None-Match'] = hit.etag;

    let res;
    try {
      res = await fetchImpl(base + path, { method, headers, body: body === undefined ? undefined : JSON.stringify(body) });
    } catch (e) {
      throw new GestorApiError('network', 'Sem conexão com o Ápice Hub.');
    }
    if (res.status === 304 && hit) return hit.body;

    let data = null;
    try { data = await res.json(); } catch { /* corpo vazio ou não-JSON */ }
    if (!res.ok) {
      const err = data?.error;
      throw new GestorApiError(err?.code || 'server_error', err?.message || 'Erro no Ápice Hub.', {
        status: res.status,
        retryAfterSeconds: err?.retry_after_seconds ?? (Number(res.headers.get('Retry-After')) || null),
      });
    }
    const etag = res.headers.get('ETag');
    if (cached && etag) cache.set(path, { etag, body: data });
    return data;
  }

  return {
    /** Troca o código de enrolamento por token (§2). Devolve { token, device_id, person }. */
    async enroll(code, device) {
      return call('POST', '/enroll', { auth: false, body: { code, device: { agent_version: agentVersion, ...device } } });
    },
    /** Tarefas da pessoa do dispositivo (§3). */
    getTasks() { return call('GET', '/me/tasks', { cached: true }); },
    /** Configuração já resolvida para esta máquina (§4). */
    getConfig() { return call('GET', '/me/config', { cached: true }); },
    /** Telemetria (§5). Devolve { received_at, config_version }. */
    heartbeat(info) {
      return call('POST', '/heartbeat', { body: { sent_at: new Date().toISOString(), agent_version: agentVersion, ...info } });
    },
    /** Ordens pendentes deste dispositivo (§8.2). Sem cache. */
    getCommands() { return call('GET', '/me/commands'); },
    /** Reivindica a ordem (§8.2). Resolve só com 200; QUALQUER outra coisa rejeita → não executar. */
    claimCommand(id) { return call('POST', `/me/commands/${encodeURIComponent(id)}/claim`, { body: {} }); },
    /** Avisa que a execução falhou e o PC continuou ligado (§8.2). */
    reportCommandFailed(id, error) {
      return call('POST', `/me/commands/${encodeURIComponent(id)}/result`, { body: { status: 'failed', error: String(error).slice(0, 500) } });
    },
    /** Troca o token (depois do enroll) e limpa o cache. */
    setToken(t) { token = t; cache.clear(); },
  };
}

// Abas do cartão a partir do corpo de getTasks() — regra de §3.2, para a interface não reimplementar.
export function tabsOf(data) {
  const t = data.tasks;
  const today = t.filter((x) => x.bucket === 'today');
  return {
    hoje: {
      count: data.summary.overdue + data.summary.due_today,
      overdue: t.filter((x) => x.bucket === 'overdue'),
      today: [...today.filter((x) => x.status !== 'done'), ...today.filter((x) => x.status === 'done')],
    },
    semana: { count: data.summary.week, tasks: t.filter((x) => x.bucket === 'week') },
    todas: { count: data.summary.open_total, tasks: t.filter((x) => x.status !== 'done') },
  };
}

// Travas do agente que valem ANTES do claim (contrato §8.6). Função pura, para a casca seguir à risca:
//   'claim'                   → pode reivindicar (e só executa se o claim der 200 — trava a)
//   'ignore:unknown_type'     → tipo que esta versão não conhece
//   'ignore:already_executed' → id já gravado localmente (trava c)
//   'ignore:before_start'     → ordem mais velha que esta execução do app (trava b)
// processUptimeSeconds: relógio monotônico DESTE processo, nunca o relógio do PC.
export function commandDecision(cmd, { processUptimeSeconds, executedIds }) {
  if (cmd.type !== 'shutdown') return 'ignore:unknown_type';
  if (executedIds.has(cmd.id)) return 'ignore:already_executed';
  if (!(cmd.age_seconds <= processUptimeSeconds)) return 'ignore:before_start'; // NaN/indefinido também ignora
  return 'claim';
}
