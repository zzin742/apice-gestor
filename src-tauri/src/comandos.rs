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

pub async fn rodar(api: Arc<Api>, data_dir: PathBuf) {
    loop {
        let intervalo = ciclo(&api, &data_dir).await;
        tokio::time::sleep(Duration::from_secs(intervalo)).await;
    }
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
    // (c) grava o id ANTES de desligar; se a gravação falhar, não desliga e avisa.
    if let Err(e) = executados.registrar(id) {
        api.result_falhou(id, &format!("falha ao gravar o id antes de desligar: {e}")).await;
        return;
    }
    // (d) uma tentativa da sequência; se falhar de cara, avisa já.
    if let Err(motivo) = desligar::desligar() {
        api.result_falhou(id, &motivo).await;
        return;
    }
    // Deu certo: o PC apaga durante a espera e este processo morre. Se ainda estamos vivos, falhou.
    tokio::time::sleep(ESPERA_DESLIGAR).await;
    api.result_falhou(id, "o processo seguiu vivo 60 s após o comando de desligar").await;
}
