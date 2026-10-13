//! Laço das ordens do Hub (contrato §8): consulta `me/commands`, aplica as quatro travas e,
//! com a ordem reivindicada, desliga o PC. No máximo uma ordem por consulta.
//!
//! Agnóstico ao Tauri: recebe a `Api` e o diretório de dados; o chamador (serviço, ou a cápsula em
//! dev) spawna `rodar` no seu runtime.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use crate::api::Api;
use crate::desligar::{self, Executados};

/// Intervalo padrão quando não há config, e mínimo quando ligado (§8.4).
const INTERVALO_PADRAO: u64 = 60;
const INTERVALO_MINIMO: u64 = 30;
/// Com as ordens desligadas (`commands_poll_interval_seconds = 0`), reavalia o config de vez em
/// quando: o admin pode religar.
const INTERVALO_DESLIGADO: u64 = 300;
/// §8.6 (d): vivo este tempo depois de mandar desligar = a sequência falhou.
const ESPERA_DESLIGAR: Duration = Duration::from_secs(60);
/// J1: salto do tempo suspenso entre voltas que conta como "o PC dormiu/hibernou".
const SALTO_SUSPENSAO: Duration = Duration::from_secs(120);

pub async fn rodar(api: Arc<Api>, data_dir: PathBuf) {
    // J1: rede de segurança contra a corrida na retomada — o POWEREVENT pode chegar DEPOIS desta
    // volta. O tempo suspenso acumulado (QueryInterruptTime − QueryUnbiasedInterruptTime) cresce
    // quando o PC dorme/hiberna; se saltou mais que o limiar desde a volta anterior, houve
    // suspensão → rezera o relógio da trava (b) ANTES de avaliar a ordem.
    let mut suspenso_antes = suspensao_acumulada();
    loop {
        if let (Some(antes), Some(agora)) = (suspenso_antes, suspensao_acumulada()) {
            if agora.saturating_sub(antes) > SALTO_SUSPENSAO {
                desligar::marcar_inicio();
                crate::diario::registrar("suspensão detectada no laço de ordens; relógio da trava (b) zerado");
            }
        }
        suspenso_antes = suspensao_acumulada();
        let intervalo = ciclo(&api, &data_dir).await;
        tokio::time::sleep(Duration::from_secs(intervalo)).await;
    }
}

/// Tempo total que o PC passou suspenso desde o boot (QueryInterruptTime inclui suspensão;
/// QueryUnbiasedInterruptTime exclui; a diferença é o suspenso). Detector do J1, não âncora.
#[cfg(windows)]
fn suspensao_acumulada() -> Option<Duration> {
    use windows_sys::Win32::System::WindowsProgramming::{
        QueryInterruptTime, QueryUnbiasedInterruptTime,
    };
    let (mut com, mut sem): (u64, u64) = (0, 0);
    unsafe {
        QueryInterruptTime(&mut com);
        if QueryUnbiasedInterruptTime(&mut sem) == 0 {
            return None;
        }
    }
    // Unidades de 100 ns.
    Some(Duration::from_nanos(com.saturating_sub(sem).saturating_mul(100)))
}

#[cfg(not(windows))]
fn suspensao_acumulada() -> Option<Duration> {
    None
}

/// Uma volta do laço. Devolve quantos segundos dormir até a próxima.
async fn ciclo(api: &Api, data_dir: &Path) -> u64 {
    // O intervalo (e o liga/desliga) vêm do me/config; a leitura é barata (cache ETag).
    let poll = match api.get("me/config").await {
        Ok(cfg) => cfg
            .get("commands_poll_interval_seconds")
            .and_then(|v| v.as_u64())
            .unwrap_or(INTERVALO_PADRAO),
        // Sem config (sem rede, não enrolado): tenta de novo no intervalo padrão.
        Err(_) => return INTERVALO_PADRAO,
    };
    if poll == 0 {
        return INTERVALO_DESLIGADO; // ordens desligadas pelo admin
    }

    verificar(api, data_dir).await;
    poll.max(INTERVALO_MINIMO)
}

/// Processa no máximo uma ordem, na ordem de operações do §8.6.
async fn verificar(api: &Api, data_dir: &Path) {
    let Ok(resposta) = api.get_sem_cache("me/commands").await else { return };
    let Some(ordem) = resposta.get("commands").and_then(|c| c.as_array()).and_then(|a| a.first())
    else {
        return; // lista vazia é o caso normal
    };

    // type == shutdown? (tipo desconhecido: ignora, não reivindica)
    if ordem.get("type").and_then(|v| v.as_str()) != Some("shutdown") {
        return;
    }
    let Some(id) = ordem.get("id").and_then(|v| v.as_str()) else { return };
    let age = ordem.get("age_seconds").and_then(|v| v.as_u64()).unwrap_or(u64::MAX);

    let executados = Executados::em(data_dir);

    // (c) já executado nunca roda de novo.
    if executados.contem(id) {
        return;
    }
    // (b) ordem anterior a esta execução do app: ignora.
    if desligar::anterior_a_esta_execucao(age) {
        return;
    }
    // (a) só executa com claim 200 NESTA execução.
    if !api.claim(id).await {
        return;
    }
    crate::diario::registrar(&format!("ordem {id} reivindicada"));
    // (c) grava o id ANTES de desligar; se a gravação falhar, não desliga e avisa.
    if let Err(e) = executados.registrar(id) {
        crate::diario::registrar(&format!("ordem {id} abortada: falha ao gravar o id"));
        api.result_falhou(id, &format!("falha ao gravar o id antes de desligar: {e}")).await;
        return;
    }
    crate::diario::registrar(&format!("ordem {id} executada (desligando)"));
    // (d) uma tentativa da sequência; se falhar de cara, avisa já.
    if let Err(motivo) = desligar::desligar() {
        crate::diario::registrar(&format!("ordem {id} falhou no desligar"));
        api.result_falhou(id, &motivo).await;
        return;
    }
    // Deu certo: o PC apaga durante a espera e este processo morre. Se ainda estamos vivos, falhou.
    tokio::time::sleep(ESPERA_DESLIGAR).await;
    crate::diario::registrar(&format!("ordem {id} seguiu viva 60 s (falhou)"));
    api.result_falhou(id, "o processo seguiu vivo 60 s após o comando de desligar").await;
}
