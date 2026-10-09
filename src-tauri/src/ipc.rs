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
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::windows::named_pipe::{
        ClientOptions, NamedPipeServer, NamedPipeClient, ServerOptions,
    };

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
                .create_with_security_attributes_raw(PIPE, seg.ptr() as *mut _)
        }
    }

    pub async fn ler_pedido(pipe: &mut NamedPipeServer) -> Option<Pedido> {
        let mut linha = String::new();
        let mut leitor = BufReader::new(&mut *pipe);
        if leitor.read_line(&mut linha).await.ok()? == 0 {
            return None;
        }
        serde_json::from_str(linha.trim()).ok()
    }

    pub async fn responder(pipe: &mut NamedPipeServer, r: &Resposta) -> io::Result<()> {
        pipe.write_all(&quadro(r)).await
    }

    /// Cliente (cápsula): abre o pipe, manda um pedido, lê a resposta.
    pub async fn pedir(p: &Pedido) -> Resposta {
        match conversa(p).await {
            Ok(r) => r,
            // Serviço não está no ar: mesmo contrato de "sem conexão".
            Err(_) => Resposta::erro("unavailable", "O serviço do Ápice Gestor não respondeu."),
        }
    }

    async fn conversa(p: &Pedido) -> io::Result<Resposta> {
        let mut cliente: NamedPipeClient = ClientOptions::new().open(PIPE)?;
        cliente.write_all(&quadro(p)).await?;
        let mut linha = String::new();
        let mut leitor = BufReader::new(&mut cliente);
        leitor.read_line(&mut linha).await?;
        serde_json::from_str(linha.trim())
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }
}

#[cfg(windows)]
pub use win::{instancia_servidor, ler_pedido, pedir, responder, Seguranca};

// ----- Mac (dev): sem named pipe -----

#[cfg(not(windows))]
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
