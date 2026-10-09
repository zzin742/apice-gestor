//! Canal entre o serviço (SYSTEM, Sessão 0) e a cápsula (sessão do usuário): named pipe local.
//!
//! Só descem dados (tarefas, config) e sobem sinais de UI (aberturas, bom-dia). A cápsula **nunca**
//! recebe o token — ela pede, o serviço responde já resolvido. A DACL do pipe (SDDL) limita a
//! SYSTEM, Administradores e usuários interativos; nada que a cápsula mande desarma o watchdog nem
//! desliga o PC (esses pedidos não existem no protocolo).
#![allow(dead_code)] // ligado na troca atômica (serviço serve, cápsula pede)

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Caminho do pipe. Nome fixo por máquina (uma instância do serviço).
pub const PIPE: &str = r"\\.\pipe\ApiceGestor";

/// O que a cápsula pede ao serviço.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum Pedido {
    /// Tarefas já resolvidas (contrato §3).
    Tarefas,
    /// Config da máquina (§4).
    Config,
    /// Sinal de UI para o heartbeat (§5): "aberto" | "bom_dia".
    Evento { tipo: String },
}

/// O que o serviço responde.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "tipo", rename_all = "snake_case")]
pub enum Resposta {
    Ok { dados: Value },
    /// Mesmo formato de erro do contrato (§1.4), repassado ao JS pela cápsula.
    Erro { code: String, message: String },
}

impl Resposta {
    pub fn ok(dados: Value) -> Self {
        Resposta::Ok { dados }
    }
    pub fn erro(code: &str, message: &str) -> Self {
        Resposta::Erro { code: code.into(), message: message.into() }
    }
}

/// Um quadro é uma linha JSON terminada em `\n`.
fn quadro<T: Serialize>(v: &T) -> Vec<u8> {
    let mut b = serde_json::to_vec(v).unwrap_or_default();
    b.push(b'\n');
    b
}

// ----- Windows: transporte -----

#[cfg(windows)]
mod win {
    use super::*;
    use std::io;
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
    use tokio::net::windows::named_pipe::{
        ClientOptions, NamedPipeClient, NamedPipeServer, ServerOptions,
    };

    /// Teto de um quadro (pedido ou resposta), em bytes (A1).
    const LIMITE_QUADRO: u64 = 64 * 1024;

    /// Mantém vivo o security descriptor enquanto o `SECURITY_ATTRIBUTES` o aponta.
    pub struct Seguranca {
        psd: *mut core::ffi::c_void,
        sa: windows_sys::Win32::Security::SECURITY_ATTRIBUTES,
    }

    impl Seguranca {
        /// DACL via SDDL: SYSTEM e Administradores com tudo; usuários interativos leem e escrevem.
        /// Sem isso, o pipe criado por SYSTEM não seria acessível à cápsula do usuário.
        pub fn nova() -> io::Result<Self> {
            use windows_sys::Win32::Security::Authorization::{
                ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
            };
            const SDDL: &str = "D:(A;;GA;;;SY)(A;;GA;;;BA)(A;;GRGW;;;IU)";
            let sddl: Vec<u16> = SDDL.encode_utf16().chain(Some(0)).collect();
            let mut psd: *mut core::ffi::c_void = std::ptr::null_mut();
            let ok = unsafe {
                ConvertStringSecurityDescriptorToSecurityDescriptorW(
                    sddl.as_ptr(),
                    SDDL_REVISION_1,
                    &mut psd,
                    std::ptr::null_mut(),
                )
            };
            if ok == 0 {
                return Err(io::Error::last_os_error());
            }
            let sa = windows_sys::Win32::Security::SECURITY_ATTRIBUTES {
                nLength: std::mem::size_of::<windows_sys::Win32::Security::SECURITY_ATTRIBUTES>() as u32,
                lpSecurityDescriptor: psd,
                bInheritHandle: 0,
            };
            Ok(Self { psd, sa })
        }

        fn ptr(&self) -> *const core::ffi::c_void {
            &self.sa as *const _ as *const core::ffi::c_void
        }
    }

    impl Drop for Seguranca {
        fn drop(&mut self) {
            use windows_sys::Win32::Foundation::LocalFree;
            unsafe { LocalFree(self.psd as _) };
        }
    }

    /// Cria uma instância do servidor com a DACL. `primeira` marca a primeira instância (checa que
    /// ninguém mais criou o pipe antes — defesa contra sequestro de nome).
    pub fn instancia_servidor(seg: &Seguranca, primeira: bool) -> io::Result<NamedPipeServer> {
        unsafe {
            ServerOptions::new()
                .first_pipe_instance(primeira)
                .reject_remote_clients(true) // explícito: só cliente local (tokio já põe por padrão)
                .create_with_security_attributes_raw(PIPE, seg.ptr() as *mut _)
        }
    }

    /// O que uma leitura de pedido produziu.
    pub enum Leitura {
        Pedido(Pedido),
        /// Quadro grande demais (A1), JSON inválido ou erro de IO: responder `bad_request` e fechar.
        Invalido,
        /// Conexão encerrada sem dados.
        Vazia,
    }

    pub async fn ler_pedido(pipe: &mut NamedPipeServer) -> Leitura {
        let mut buf = Vec::new();
        let mut leitor = BufReader::new((&mut *pipe).take(LIMITE_QUADRO));
        if leitor.read_until(b'\n', &mut buf).await.is_err() {
            return Leitura::Invalido;
        }
        if buf.is_empty() {
            return Leitura::Vazia;
        }
        // Encheu o limite sem fechar a linha: quadro grande demais.
        if buf.len() as u64 >= LIMITE_QUADRO && buf.last() != Some(&b'\n') {
            return Leitura::Invalido;
        }
        match serde_json::from_slice::<Pedido>(&buf) {
            Ok(p) => Leitura::Pedido(p),
            Err(_) => Leitura::Invalido,
        }
    }

    pub async fn responder(pipe: &mut NamedPipeServer, r: &Resposta) -> io::Result<()> {
        pipe.write_all(&quadro(r)).await
    }

    /// Cliente (cápsula): abre o pipe, manda um pedido, lê a resposta. Timeout de 10 s (E7a): um
    /// serviço travado não pendura a cápsula.
    pub async fn pedir(p: &Pedido) -> Resposta {
        match tokio::time::timeout(std::time::Duration::from_secs(10), conversa(p)).await {
            Ok(Ok(r)) => r,
            // Serviço fora do ar ou sem resposta a tempo: mesmo contrato de "sem conexão".
            _ => Resposta::erro("unavailable", "O serviço do Ápice Gestor não respondeu."),
        }
    }

    async fn conversa(p: &Pedido) -> io::Result<Resposta> {
        let mut cliente: NamedPipeClient = ClientOptions::new().open(PIPE)?;
        cliente.write_all(&quadro(p)).await?;
        let mut buf = Vec::new();
        let mut leitor = BufReader::new((&mut cliente).take(LIMITE_QUADRO));
        leitor.read_until(b'\n', &mut buf).await?;
        serde_json::from_slice(&buf).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }
}

#[cfg(windows)]
pub use win::{instancia_servidor, ler_pedido, pedir, responder, Leitura, Seguranca};

// ----- Mac (dev): sem named pipe -----

// Dev no Mac: sem serviço nem pipe. Para a interface ter dados no localhost (regra do José Luiz
// de mostrar no localhost), o cliente fala direto com uma Api local (mock). Em release ou no
// Windows isto NÃO existe — lá a cápsula usa o named pipe acima.
#[cfg(all(not(windows), debug_assertions))]
pub async fn pedir(p: &Pedido) -> Resposta {
    use crate::api::Api;
    use std::sync::OnceLock;
    static API: OnceLock<Api> = OnceLock::new();
    let api = API.get_or_init(Api::new);
    let de = |r: Result<serde_json::Value, crate::api::ErroApi>| match r {
        Ok(dados) => Resposta::ok(dados),
        Err(e) => Resposta::erro(&e.code, &e.message),
    };
    match p {
        Pedido::Tarefas => de(api.get("me/tasks").await),
        Pedido::Config => de(api.get("me/config").await),
        Pedido::Evento { .. } => Resposta::ok(serde_json::Value::Null),
    }
}

#[cfg(all(not(windows), not(debug_assertions)))]
pub async fn pedir(_p: &Pedido) -> Resposta {
    Resposta::erro("unavailable", "IPC só existe no Windows.")
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn protocolo_roundtrip() {
        let p = Pedido::Evento { tipo: "aberto".into() };
        let txt = serde_json::to_string(&p).unwrap();
        assert_eq!(txt, r#"{"cmd":"evento","tipo":"aberto"}"#);
        assert_eq!(serde_json::from_str::<Pedido>(&txt).unwrap(), p);

        assert_eq!(serde_json::to_string(&Pedido::Tarefas).unwrap(), r#"{"cmd":"tarefas"}"#);

        let r = Resposta::erro("not_enrolled", "x");
        let txt = serde_json::to_string(&r).unwrap();
        assert_eq!(serde_json::from_str::<Resposta>(&txt).unwrap(), r);
    }
}
