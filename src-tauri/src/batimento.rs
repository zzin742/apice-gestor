//! Heartbeat (contrato §5) e o `me/config` que a casca obedece sozinha (`paused`).
//! Só telemetria do agente: nada de tela, teclado, aplicativo ou janela de outro programa.
//! Falha de heartbeat nunca aparece para o usuário; tenta de novo no próximo ciclo.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tauri::{AppHandle, Manager, Runtime};

use crate::api::{self, Api};

/// Mínimos do contrato §4.
const HEARTBEAT_MINIMO: u64 = 300;
const HEARTBEAT_PADRAO: u64 = 900;
/// O agente relê `me/config` no mínimo a cada hora (§4).
const CONFIG_MAXIMO: Duration = Duration::from_secs(3600);
/// Sem token ainda: espera o enrolamento sem martelar nada.
const ESPERA_SEM_TOKEN: Duration = Duration::from_secs(300);

/// O que a interface relata pelo `painel_evento`.
#[derive(Default)]
pub struct Telemetria {
    aberturas: AtomicU32,
    bom_dia_em: Mutex<Option<String>>,
    /// A casca escondeu a janela por `paused`; só ela mostra de volta.
    escondida_por_pausa: AtomicBool,
}

impl Telemetria {
    /// `painel_evento({tipo})`: `aberto` (o usuário abriu o cartão) ou `bom_dia`.
    pub fn registrar(&self, tipo: &str) {
        match tipo {
            "aberto" => {
                self.aberturas.fetch_add(1, Ordering::SeqCst);
            }
            "bom_dia" => *self.bom_dia_em.lock().unwrap() = Some(api::agora()),
            _ => {}
        }
    }
}

/// Laço da casca: config ao iniciar, heartbeat a cada `heartbeat_interval_seconds`,
/// config de novo quando o heartbeat disser que mudou ou a cada hora.
pub fn iniciar<R: Runtime>(app: AppHandle<R>) {
    tauri::async_runtime::spawn(async move {
        let api = app.state::<Api>();
        let mut config: Option<Value> = None;
        let mut config_lida_em: Option<Instant> = None;
        loop {
            let vencida = config_lida_em.is_none_or(|t| t.elapsed() >= CONFIG_MAXIMO);
            if vencida {
                match api.get("me/config").await {
                    Ok(c) => {
                        aplicar_pausa(&app, c["paused"].as_bool().unwrap_or(false));
                        config = Some(c);
                        config_lida_em = Some(Instant::now());
                    }
                    Err(e) if e.code == "not_enrolled" => {
                        tokio::time::sleep(ESPERA_SEM_TOKEN).await;
                        continue;
                    }
                    Err(_) => {} // segue com a config anterior; tenta de novo no próximo ciclo
                }
            }

            let versao_enviada = config.as_ref().and_then(|c| c["config_version"].as_str()).map(str::to_owned);
            if let Ok(resposta) = enviar(&app, &api, versao_enviada.as_deref()).await {
                // Só compara quando temos uma config: sem ela, a releitura já está agendada.
                let mudou = versao_enviada.is_some()
                    && resposta["config_version"].as_str() != versao_enviada.as_deref();
                if mudou {
                    config_lida_em = None; // mudou no Hub: relê no começo do próximo laço
                    continue;
                }
            }

            let intervalo = config
                .as_ref()
                .and_then(|c| c["heartbeat_interval_seconds"].as_u64())
                .unwrap_or(HEARTBEAT_PADRAO)
                .max(HEARTBEAT_MINIMO);
            tokio::time::sleep(Duration::from_secs(intervalo)).await;
        }
    });
}

async fn enviar<R: Runtime>(
    app: &AppHandle<R>,
    api: &Api,
    config_version: Option<&str>,
) -> Result<Value, api::ErroApi> {
    let telemetria = app.state::<Telemetria>();
    let aberturas = telemetria.aberturas.swap(0, Ordering::SeqCst);
    let corpo = json!({
        "sent_at": api::agora(),
        "agent_version": env!("CARGO_PKG_VERSION"),
        "os": crate::enrolar::sistema(),
        "config_version": config_version,
        "last_tasks_ok_at": *api.tarefas_ok_em.lock().unwrap(),
        "panel_opens_since_last": aberturas,
        "morning_shown_at": *telemetria.bom_dia_em.lock().unwrap(),
        "last_error_code": *api.ultimo_erro.lock().unwrap(),
    });
    let resultado = api.post("heartbeat", &corpo).await;
    if resultado.is_err() {
        // Não perde as aberturas: vão no próximo heartbeat.
        telemetria.aberturas.fetch_add(aberturas, Ordering::SeqCst);
    }
    resultado
}

/// `paused: true` → a cápsula some (fica só a bandeja); `false` → volta, se fomos nós que escondemos.
fn aplicar_pausa<R: Runtime>(app: &AppHandle<R>, pausado: bool) {
    let Some(painel) = app.get_webview_window("main") else { return };
    let telemetria = app.state::<Telemetria>();
    if pausado {
        if painel.hide().is_ok() {
            telemetria.escondida_por_pausa.store(true, Ordering::SeqCst);
        }
    } else if telemetria.escondida_por_pausa.swap(false, Ordering::SeqCst) {
        let _ = painel.show();
    }
}

/// A casca está pausada pelo Hub? (a segunda abertura do app não deve mostrar a janela).
pub fn pausado<R: Runtime>(app: &AppHandle<R>) -> bool {
    app.state::<Telemetria>().escondida_por_pausa.load(Ordering::SeqCst)
}
