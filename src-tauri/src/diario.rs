//! Log em disco do serviço, para a Esteira diagnosticar queda na bancada (E6/E7d).
//! `%ProgramData%\ApiceGestor\servico.log` — **sem token, sem segredo, e nunca a lista do que a
//! pessoa abre**: só eventos de domínio (enrolou, ordem reivindicada/executada, processo
//! encerrado por um item da lista). Rotaciona em ~1 MB (um histórico, `.log.1`).

use std::io::Write;

/// ~1 MB antes de rotacionar.
const TETO: u64 = 1024 * 1024;

pub fn registrar(msg: &str) {
    let Some(pasta) = crate::cofre::pasta_dados() else { return };
    let _ = std::fs::create_dir_all(&pasta);
    let arquivo = pasta.join("servico.log");
    if std::fs::metadata(&arquivo).map(|m| m.len() > TETO).unwrap_or(false) {
        let _ = std::fs::rename(&arquivo, pasta.join("servico.log.1"));
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&arquivo) {
        let _ = writeln!(f, "{} v{} {}", crate::api::agora(), env!("CARGO_PKG_VERSION"), msg);
    }
}
