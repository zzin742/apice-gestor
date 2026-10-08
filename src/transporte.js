// Transporte da interface: dentro da casca Tauri, fala com a API por IPC (o token fica no keychain,
// do lado do Rust — a interface nunca o vê); no navegador, usa src/api/client.js contra o mock.
// Os dois devolvem o mesmo JSON do contrato (docs/contrato-api.md) e erros como GestorApiError.

import { createClient, GestorApiError, MOCK_BASE_URL } from './api/client.js';

const tauri = globalThis.__TAURI__;

function viaIpc() {
  const invoke = tauri.core.invoke;
  // A casca devolve o erro como { code, message } (combinado com o Chassi).
  const call = (cmd, args) => invoke(cmd, args).catch((e) => {
    throw new GestorApiError(e?.code || 'server_error', e?.message || String(e));
  });
  return {
    getTasks: () => call('gestor_tarefas'),
    getConfig: () => call('gestor_config'),
    heartbeat: (payload) => call('gestor_batimento', { payload }),
    enroll: (codigo) => call('gestor_enrolar', { codigo }),
  };
}

function viaNavegador() {
  // ?token=gdt_mock_* troca o cenário do mock (mock/README.md); ?api= troca o endereço.
  const qs = new URLSearchParams(location.search);
  return createClient({
    baseUrl: qs.get('api') || MOCK_BASE_URL,
    token: qs.get('token') || 'gdt_mock_rafael',
    agentVersion: '0.1.0',
  });
}

export const api = tauri?.core?.invoke ? viaIpc() : viaNavegador();

// Abre a tarefa no navegador padrão. Na casca, pelo plugin opener (escopo: app.gestta.com.br).
export function abrirNoNavegador(url) {
  if (tauri?.opener?.openUrl) return tauri.opener.openUrl(url);
  window.open(url, '_blank', 'noopener');
  return Promise.resolve();
}

// Tamanho da janela da casca em px lógicos (o Chassi cuida do DPI e mantém o canto inferior direito).
// Fora da casca não há janela: guarda o pedido para revisão no navegador.
// ancora = canto da janela que fica parado ('inferior-direito' | 'inferior-esquerdo' | 'superior-direito' | 'superior-esquerdo').
export function ajustarJanela({ largura, altura, ancora = 'inferior-direito' }) {
  if (tauri?.core?.invoke) return tauri.core.invoke('painel_tamanho', { largura, altura, ancora });
  globalThis.__painelTamanho = { largura, altura, ancora };
  globalThis.dispatchEvent(new CustomEvent('painel-tamanho', { detail: { largura, altura, ancora } }));
  return Promise.resolve();
}

// Arraste nativo da janela (a casca chama start_dragging). Fora da casca só conta, para revisão.
export function arrastarJanela() {
  if (tauri?.core?.invoke) return tauri.core.invoke('painel_arrastar');
  globalThis.__painelArrastes = (globalThis.__painelArrastes || 0) + 1;
  return Promise.resolve();
}

// Posição da janela: { x, y, largura, altura, area: { largura, altura } }, px lógicos, relativos à área útil.
// Fora da casca não há posição (null): a interface usa a direção padrão ou a simulada por ?abre=.
export function posicaoJanela() {
  if (tauri?.core?.invoke) return tauri.core.invoke('painel_posicao');
  return Promise.resolve(globalThis.__painelPosicao || null);
}

// A casca emite 'painel://posicao' ~300 ms depois de a janela parar (arraste, 'Voltar ao canto', troca de monitor).
export function aoMudarPosicao(fn) {
  if (tauri?.event?.listen) return tauri.event.listen('painel://posicao', (e) => fn(e.payload));
  globalThis.addEventListener('painel-posicao', (e) => fn(e.detail));
  return Promise.resolve(() => {});
}
