// Dados fictícios do mock — determinísticos, no tamanho dos números reais
// (Arquitetura §4: mediana 15 em "vencidas + hoje", mediana 146 abertas, 4 pessoas > 100).
// Nenhum nome aqui é de pessoa ou cliente real.

export const TZ_OFFSET = '-03:00'; // America/Sao_Paulo, sem horário de verão desde 2019

// "Agora" do mock. MOCK_NOW=2026-10-08T13:02 fixa o relógio (útil para print e teste).
export function now() {
  return process.env.MOCK_NOW ? new Date(process.env.MOCK_NOW + TZ_OFFSET) : new Date();
}

// Data de calendário em São Paulo, YYYY-MM-DD.
export function spDate(d) {
  return new Intl.DateTimeFormat('en-CA', { timeZone: 'America/Sao_Paulo' }).format(d);
}

// Instante ISO com offset de São Paulo, sem milissegundos.
export function spInstant(d) {
  const local = new Date(d.getTime() - 3 * 3600e3).toISOString().slice(0, 19);
  return local + TZ_OFFSET;
}

export function addDays(ymd, n) {
  const d = new Date(ymd + 'T12:00:00Z');
  d.setUTCDate(d.getUTCDate() + n);
  return d.toISOString().slice(0, 10);
}

function rng(seed) { // mulberry32
  return function () {
    seed |= 0; seed = (seed + 0x6d2b79f5) | 0;
    let t = Math.imul(seed ^ (seed >>> 15), 1 | seed);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

const CLIENTS = [
  'Auto Posto Sol', 'MM Mercado', 'Clínica Vida', 'Transportes RT', 'Construtora Álamo',
  'Padaria Trigo de Ouro', 'Mercado União', 'Oficina do Km', 'Panificadora Sul', 'Farmácia Central',
  'Distribuidora Norte', 'Lanchonete Praça', 'Serralheria Ideal', 'Escola Horizonte', 'Hotel Mirante',
  'Gráfica Rápida', 'Ótica Visão Clara', 'Pet Shop Amigo Fiel', 'Laticínios Serra Azul', 'Agropecuária Boa Safra',
  'Restaurante Sabor Caseiro', 'Academia Corpo em Forma', 'Imobiliária Porto Seguro', 'Madeireira Ipê',
  'Confecções Linha Fina', 'Odontologia Sorriso', 'Vidraçaria Cristal', 'Elétrica Faísca', 'Calçados Passo Certo',
  'Cerealista Grão Bom', 'Transportadora Via Leste', 'Floricultura Jardim', 'Mecânica Torque', 'Papelaria Traço',
];

const TEMPLATES = {
  Pessoal: [
    ['Folha mensal — fechamento', true], ['Envio de holerites', true], ['FGTS — guia', true],
    ['GPS — guia', true], ['eSocial — evento S-1200', true], ['eSocial — evento S-2200', false],
    ['Admissão — novo empregado', false], ['Rescisão — contrato', false], ['Aviso de férias', false],
    ['Cálculo de férias', false], ['Afastamento — INSS', false], ['Pró-labore — cálculo', true],
    ['Homologação de rescisão', false], ['DCTFWeb', true],
  ],
  Fiscal: [
    ['Apuração de ICMS', true], ['Apuração de ISS', true], ['EFD ICMS/IPI', true], ['EFD Contribuições', true],
    ['DAS — Simples Nacional', true], ['DIFAL — guia', true], ['Conferência de notas de entrada', true],
    ['DCTF', true], ['Parcelamento — acompanhamento', false], ['GIA', true],
  ],
  'Contábil': [
    ['Conciliação bancária', true], ['Balancete mensal', true], ['Lançamentos contábeis', true],
    ['DRE gerencial', true], ['Provisões trabalhistas', true], ['ECD — envio', false], ['ECF — envio', false],
  ],
  'Legalização': [
    ['Alteração contratual', false], ['Renovação de alvará', false], ['Certidão negativa', false],
    ['Abertura de empresa', false], ['Licença sanitária', false], ['Baixa de inscrição estadual', false],
  ],
};

// Monta a carteira de uma pessoa: n tarefas por faixa, relativo a `today`.
// counts = { overdue, today, done, week, later }, open_total = overdue + today + week + later.
export function buildTasks(person, today) {
  const r = rng(person.seed);
  const pick = (arr) => arr[Math.floor(r() * arr.length)];
  const tpls = TEMPLATES[person.department];
  const [y, m] = today.split('-').map(Number);
  const prevComp = m === 1 ? `${y - 1}-12` : `${y}-${String(m - 1).padStart(2, '0')}`;
  let nextId = 7_400_000 + person.seed * 1_000;
  const out = [];

  function make(bucket, offset, extra = {}) {
    const [name, monthly] = pick(tpls);
    const competence = monthly ? prevComp : null;
    let title = name;
    if (monthly) title += ` — comp. ${competence.slice(5)}/${competence.slice(0, 4)}`;
    else if (name.startsWith('Rescisão')) title += ` ${String(Math.floor(r() * 900) + 100).padStart(5, '0')}`;
    else if (/férias|Afastamento/.test(name)) title += ` — ${pick('ABCDEFGJLMPRSTV')}. ${pick(['Santos', 'Lima', 'Souza', 'Rocha', 'Alves', 'Costa'])}`;
    const id = String(nextId++);
    const due_date = offset === null ? null : addDays(today, offset);
    out.push({
      id, title, client: pick(CLIENTS),
      department: r() < 0.92 ? person.department : pick(Object.keys(TEMPLATES)),
      competence,
      status: 'open', bucket, due_date,
      due_time: r() < 0.3 ? pick(['12:00', '16:00', '17:00', '18:00']) : null,
      days_overdue: offset !== null && offset < 0 ? -offset : 0,
      done_at: null,
      gestta_url: `https://app.gestta.com.br/tarefa/${id}`,
      ...extra,
    });
  }

  // Vencidas: maioria recente, cauda longa até ~90 dias.
  for (let i = 0; i < person.counts.overdue; i++) make('overdue', -(1 + Math.floor(r() ** 2.2 * 90)));
  for (let i = 0; i < person.counts.today; i++) make('today', 0);
  for (let i = 0; i < person.counts.done; i++) {
    const h = 8 + Math.floor(r() * 5), mi = Math.floor(r() * 60);
    make('today', 0, { status: 'done', done_at: `${today}T${String(h).padStart(2, '0')}:${String(mi).padStart(2, '0')}:00${TZ_OFFSET}` });
  }
  for (let i = 0; i < person.counts.week; i++) make('week', 1 + Math.floor(r() * 7));
  for (let i = 0; i < person.counts.later; i++) make('later', r() < 0.04 ? null : 8 + Math.floor(r() * 110));

  // ~3% das abertas em impedimento (IMPEDIMENT conta como aberta).
  for (const t of out) if (t.status === 'open' && r() < 0.03) t.status = 'impediment';
  for (const t of out) if (t.status === 'done') t.due_time = null;

  out.sort((a, b) =>
    (a.due_date ?? '9999') .localeCompare(b.due_date ?? '9999') ||
    (a.due_time ?? '99:99').localeCompare(b.due_time ?? '99:99') ||
    (a.status === 'done') - (b.status === 'done') ||
    a.client.localeCompare(b.client, 'pt-BR'));
  return out;
}

// Pessoas fictícias, uma por cenário. Tokens e códigos só valem no mock.
export const PEOPLE = {
  rafael: {
    id: 'pes_rafael', display_name: 'Rafael Moura', first_name: 'Rafael', department: 'Pessoal', seed: 11,
    counts: { overdue: 6, today: 9, done: 4, week: 18, later: 113 }, // 15 em vencidas+hoje, 146 abertas
  },
  bianca: {
    id: 'pes_bianca', display_name: 'Bianca Teles', first_name: 'Bianca', department: 'Fiscal', seed: 23,
    counts: { overdue: 118, today: 23, done: 2, week: 64, later: 307 }, // as "4 pessoas > 100"
  },
  caio: {
    id: 'pes_caio', display_name: 'Caio Nunes', first_name: 'Caio', department: 'Contábil', seed: 37,
    counts: { overdue: 0, today: 0, done: 1, week: 3, later: 8 }, // dia limpo
  },
  denise: {
    id: 'pes_denise', display_name: 'Denise Prado', first_name: 'Denise', department: 'Pessoal', seed: 41,
    counts: { overdue: 8, today: 7, done: 0, week: 21, later: 98 }, stale: true, // robô parado: última rodada a > 12 h úteis
  },
  elton: {
    id: 'pes_elton', display_name: 'Elton Reis', first_name: 'Elton', department: 'Legalização', seed: 53,
    counts: { overdue: 3, today: 2, done: 1, week: 6, later: 40 }, paused: true, // pausado no Hub
  },
};

// token → cenário. `kind` diferente de 'ok' é erro.
export const TOKENS = {
  gdt_mock_rafael: { kind: 'ok', person: 'rafael' },
  gdt_mock_bianca: { kind: 'ok', person: 'bianca' },
  gdt_mock_caio: { kind: 'ok', person: 'caio' },
  gdt_mock_denise: { kind: 'ok', person: 'denise' },
  gdt_mock_elton: { kind: 'ok', person: 'elton' },
  gdt_mock_revogado: { kind: 'token_revoked' },
  gdt_mock_sem_depara: { kind: 'person_not_mapped' },
  gdt_mock_caixa: { kind: 'excluded_account' },
  gdt_mock_fora: { kind: 'unavailable' },
  gdt_mock_velho_agente: { kind: 'upgrade_required' },
};

// Código de enrolamento (normalizado, sem hífen) → cenário.
export const CODES = {
  K7QM4XPA: { kind: 'ok', person: 'rafael' },
  B3NT9ZQW: { kind: 'ok', person: 'bianca' },
  C4UN2SLM: { kind: 'ok', person: 'caio' },
  VENC7DQX: { kind: 'code_expired' },
  USAD2222: { kind: 'code_used' },
  CAXA4444: { kind: 'excluded_account' },
};

// ---------- dado velho em horas úteis (contrato §3.5) ----------

const SP_MS = 3 * 3600e3; // São Paulo = UTC−3; trabalhamos num "relógio local" deslocado
const BUSINESS = { from: 8, to: 18 }; // seg–sex, 08:00–18:00. Feriado conta como útil na v1.
export const ROBOT_RUNS = [7, 10, 13, 16]; // rodadas fixas do robô (decisão de 08/10/2026)

// Horas úteis entre dois instantes (Date), seg–sex 08–18 em São Paulo.
export function businessHoursBetween(a, b) {
  if (b <= a) return 0;
  let ms = 0;
  const end = b.getTime() - SP_MS;
  let day = new Date(a.getTime() - SP_MS); day.setUTCHours(0, 0, 0, 0);
  for (; day.getTime() <= end; day.setUTCDate(day.getUTCDate() + 1)) {
    const dow = day.getUTCDay();
    if (dow === 0 || dow === 6) continue;
    const open = day.getTime() + BUSINESS.from * 3600e3, close = day.getTime() + BUSINESS.to * 3600e3;
    const s = Math.max(open, a.getTime() - SP_MS), e = Math.min(close, end);
    if (e > s) ms += e - s;
  }
  return ms / 3600e3;
}

// Instante da k-ésima rodada do robô antes de `n` (k = 0 é a mais recente).
// Cada rodada grava alguns minutos depois do horário cheio.
export function robotRun(n, k = 0, minutesAfter = 4) {
  const local = new Date(n.getTime() - SP_MS);
  const day = new Date(local); day.setUTCHours(0, 0, 0, 0);
  const runs = [];
  for (let d = 0; runs.length <= k; d++) {
    const base = day.getTime() - d * 86400e3;
    for (const h of [...ROBOT_RUNS].reverse()) {
      const t = base + h * 3600e3 + minutesAfter * 60e3;
      if (t <= local.getTime()) runs.push(t);
    }
  }
  return new Date(runs[k] + SP_MS);
}
