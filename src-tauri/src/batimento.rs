//! Heartbeat (contrato §5) e o `me/config` que o agente obedece sozinho (`paused`).
//! Só telemetria do agente: nada de tela, teclado, aplicativo ou janela de outro programa.
//! Falha de heartbeat nunca aparece para o usuário; tenta de novo no próximo ciclo.
//!
//! Agnóstico ao Tauri: recebe a `Api`, a `Telemetria`, o diretório de dados e um callback de
//! pausa. Quem decide o efeito da pausa é o chamador — na cápsula, esconder/mostrar a janela; no
//! serviço, avisar a cápsula pelo IPC.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::api::{self, Api};

/// Mínimos do contrato §4.
const HEARTBEAT_MINIMO: u64 = 300;
const HEARTBEAT_PADRAO: u64 = 900;
/// O agente relê `me/config` no mínimo a cada hora (§4).
const CONFIG_MAXIMO: Duration = Duration::from_secs(3600);
/// Sem token ainda: espera o enrolamento sem martelar nada.
const ESPERA_SEM_TOKEN: Duration = Duration::from_secs(300);

/// Callback de pausa: recebe o `paused` do config. Implementado pelo chamador.
pub type Pausa = Arc<dyn Fn(bool) + Send + Sync>;

/// Contadores de UI do SERVIÇO (chegam da cápsula por IPC e vão no heartbeat). A pausa é estado da
/// cápsula (PausaCapsula, em lib.rs), não daqui — D1.
#[derive(Default)]
pub struct Telemetria {
    aberturas: AtomicU32,
    bom_dia_em: Mutex<Option<String>>,
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

/// Laço: config ao iniciar, heartbeat a cada `heartbeat_interval_seconds`, config de novo quando o
/// heartbeat disser que mudou ou a cada hora. O chamador spawna no seu runtime.
pub async fn rodar(api: Arc<Api>, telemetria: Arc<Telemetria>, data_dir: PathBuf, pausa: Pausa) {
    let mut config: Option<Value> = None;
    let mut config_lida_em: Option<Instant> = None;
    let mut enrolado_antes: Option<bool> = None; // para logar só a transição
    loop {
        let vencida = config_lida_em.is_none_or(|t| t.elapsed() >= CONFIG_MAXIMO);
        if vencida {
            match api.get("me/config").await {
                Ok(c) => {
                    if enrolado_antes != Some(true) {
                        crate::diario::registrar("enrolado: token aceito pelo Hub");
                        enrolado_antes = Some(true);
                    }
                    pausa(c["paused"].as_bool().unwrap_or(false));
                    config = Some(c);
                    config_lida_em = Some(Instant::now());
                }
                Err(e) if e.code == "not_enrolled" => {
                    if enrolado_antes != Some(false) {
                        crate::diario::registrar("sem token: aguardando enrolamento");
                        enrolado_antes = Some(false);
                    }
                    tokio::time::sleep(ESPERA_SEM_TOKEN).await;
                    continue;
                }
                Err(_) => {} // segue com a config anterior; tenta de novo no próximo ciclo
            }
        }

        let versao_enviada =
            config.as_ref().and_then(|c| c["config_version"].as_str()).map(str::to_owned);
        if let Ok(resposta) = enviar(&api, &telemetria, &data_dir, versao_enviada.as_deref()).await {
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
}

async fn enviar(
    api: &Api,
    telemetria: &Telemetria,
    data_dir: &Path,
    config_version: Option<&str>,
) -> Result<Value, api::ErroApi> {
    let aberturas = telemetria.aberturas.swap(0, Ordering::SeqCst);
    // §8.5: id da última ordem executada (gravada antes de desligar, sobrevive ao boot).
    let last_command_id = crate::desligar::Executados::em(data_dir).ultimo();
    let corpo = json!({
        "sent_at": api::agora(),
        "agent_version": env!("CARGO_PKG_VERSION"),
        "os": crate::enrolar::sistema(),
        "config_version": config_version,
        "last_tasks_ok_at": *api.tarefas_ok_em.lock().unwrap(),
        "panel_opens_since_last": aberturas,
        "morning_shown_at": *telemetria.bom_dia_em.lock().unwrap(),
        "last_error_code": *api.ultimo_erro.lock().unwrap(),
        "last_command_id": last_command_id,
        "uptime_seconds": crate::desligar::uptime_windows_segundos(),
    });
    let resultado = api.post("heartbeat", &corpo).await;
    if resultado.is_err() {
        // Não perde as aberturas: vão no próximo heartbeat.
        telemetria.aberturas.fetch_add(aberturas, Ordering::SeqCst);
    }
    resultado
}
