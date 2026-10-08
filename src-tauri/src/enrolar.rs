//! `apice-gestor.exe --enrolar <código>`: roda uma vez por PC, no script de instalação.
//! Não abre janela. Sucesso: imprime a pessoa e sai com 0. Erro: imprime `code: message` e sai ≠ 0.

use std::time::Duration;

use serde_json::{json, Value};

use crate::api;

/// Saídas para o `instalar.ps1` decidir sem ler texto.
pub const SAIDA_OK: i32 = 0;
const SAIDA_CODIGO: i32 = 2; // invalid_code, code_expired, code_used: pedir código novo
const SAIDA_LIMITE: i32 = 3; // rate_limited depois de esperar
const SAIDA_CONEXAO: i32 = 4; // unavailable, server_error
const SAIDA_OUTRO: i32 = 5; // invalid_request, excluded_account, keychain…
pub const SAIDA_USO: i32 = 64;

/// Tentativas quando o Hub responde `429` (os 95 PCs saem pelo mesmo IP, contrato §2).
const TENTATIVAS: u32 = 3;
/// Teto da espera entre tentativas, para o instalador não ficar pendurado.
const ESPERA_MAXIMA: u64 = 300;

pub fn executar(codigo: &str) -> i32 {
    let codigo = codigo.trim();
    if codigo.is_empty() {
        eprintln!("uso: apice-gestor --enrolar <código>");
        return SAIDA_USO;
    }
    let device = dispositivo();

    for tentativa in 1..=TENTATIVAS {
        match tauri::async_runtime::block_on(api::enrolar(codigo, device.clone())) {
            Ok(pessoa) => {
                println!("{} · {}", pessoa.nome, pessoa.departamento);
                return SAIDA_OK;
            }
            Err(erro) => {
                println!("{}: {}", erro.code, erro.message);
                match erro.code.as_str() {
                    // Nunca repetir sozinho: só com um código novo digitado.
                    "invalid_code" | "code_expired" | "code_used" => return SAIDA_CODIGO,
                    "rate_limited" if tentativa < TENTATIVAS => {
                        let espera = erro.retry_after_seconds.unwrap_or(60).min(ESPERA_MAXIMA);
                        println!("aguardando {espera} s para tentar de novo…");
                        std::thread::sleep(Duration::from_secs(espera));
                    }
                    "rate_limited" => return SAIDA_LIMITE,
                    "unavailable" | "server_error" => return SAIDA_CONEXAO,
                    _ => return SAIDA_OUTRO,
                }
            }
        }
    }
    SAIDA_LIMITE
}

/// `device` do contrato §2. Nada aqui é segredo.
fn dispositivo() -> Value {
    json!({
        "hostname": hostname(),
        "os": sistema(),
        "agent_version": env!("CARGO_PKG_VERSION"),
        "machine_id": machine_id(),
    })
}

#[cfg(windows)]
fn hostname() -> String {
    std::env::var("COMPUTERNAME").unwrap_or_default()
}

#[cfg(windows)]
pub(crate) fn sistema() -> String {
    const CHAVE: &str = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion";
    let nome = registro(CHAVE, "ProductName").unwrap_or_else(|| "Windows".into());
    let versao = registro(CHAVE, "DisplayVersion").unwrap_or_default();
    let build = registro(CHAVE, "CurrentBuild").unwrap_or_default();
    // ProductName diz "Windows 10" também no 11; o build (≥ 22000 = 11) desfaz a dúvida no Hub.
    format!("{nome} {versao} (build {build})").trim().to_owned()
}

#[cfg(windows)]
fn machine_id() -> String {
    registro(r"SOFTWARE\Microsoft\Cryptography", "MachineGuid").unwrap_or_default()
}

/// Lê um valor texto de HKLM, na visão de 64 bits.
#[cfg(windows)]
fn registro(chave: &str, valor: &str) -> Option<String> {
    use windows_sys::Win32::System::Registry::{
        RegGetValueW, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ, RRF_SUBKEY_WOW6464KEY,
    };
    let largo = |s: &str| s.encode_utf16().chain(Some(0)).collect::<Vec<u16>>();
    let (chave, valor) = (largo(chave), largo(valor));
    let mut buffer = [0u16; 256];
    let mut bytes = (buffer.len() * 2) as u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            chave.as_ptr(),
            valor.as_ptr(),
            RRF_RT_REG_SZ | RRF_SUBKEY_WOW6464KEY,
            std::ptr::null_mut(),
            buffer.as_mut_ptr().cast(),
            &mut bytes,
        )
    };
    if status != 0 {
        return None;
    }
    let len = (bytes as usize / 2).saturating_sub(1);
    Some(String::from_utf16_lossy(&buffer[..len]))
}

// Mac só serve de desenvolvimento: valores de identificação, sem rigor.
#[cfg(not(windows))]
fn hostname() -> String {
    std::process::Command::new("hostname")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        .unwrap_or_default()
}

#[cfg(not(windows))]
pub(crate) fn sistema() -> String {
    format!("{} (desenvolvimento)", std::env::consts::OS)
}

#[cfg(not(windows))]
fn machine_id() -> String {
    String::new()
}
