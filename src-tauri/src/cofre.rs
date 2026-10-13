//! Cofre do token do dispositivo quando quem fala com o Hub é o **serviço SYSTEM**
//! (AGENTS.md regra 4, atualizada por 10a93e4; §1.2 do contrato).
//!
//! Windows: `%ProgramData%\ApiceGestor\device.token`, cifrado com DPAPI de **máquina**
//! (só este PC descriptografa) e protegido por **ACL** — a proteção real contra outro usuário da
//! máquina, porque o DPAPI de máquina qualquer processo local decifra. A pasta nasce com herança
//! desligada e só SYSTEM + Administradores. Nunca texto puro, nunca em log, nunca em commit.
//!
//! Mac (desenvolvimento): arquivo simples em `~/.config/apice-gestor-dev/`, sem cifra — só para
//! `cargo`/bancada de dev; o build de release é Windows.
use std::path::PathBuf;

/// Falta o arquivo (não enrolado) vs. qualquer outra falha (acesso, cifra).
pub enum ErroCofre {
    NaoEnrolado,
    Falha(String),
}

#[cfg(windows)]
const PASTA: &str = "ApiceGestor";
const ARQUIVO: &str = "device.token";

#[cfg(windows)]
fn pasta() -> Result<PathBuf, ErroCofre> {
    let base = std::env::var_os("ProgramData")
        .ok_or_else(|| ErroCofre::Falha("ProgramData não definido".into()))?;
    Ok(PathBuf::from(base).join(PASTA))
}

#[cfg(not(windows))]
fn pasta() -> Result<PathBuf, ErroCofre> {
    let home = std::env::var_os("HOME")
        .ok_or_else(|| ErroCofre::Falha("HOME não definido".into()))?;
    Ok(PathBuf::from(home).join(".config/apice-gestor-dev"))
}

fn caminho() -> Result<PathBuf, ErroCofre> {
    Ok(pasta()?.join(ARQUIVO))
}

/// Pasta de dados da MÁQUINA (a mesma do device.token, com a ACL fechada pelo --enrolar). O
/// serviço guarda aqui os ids de ordens executadas (§8) — nunca em app_local_data_dir, que é por
/// usuário e o colaborador apagaria (D2 do Diretor).
#[cfg_attr(not(windows), allow(dead_code))]
pub fn pasta_dados() -> Option<PathBuf> {
    pasta().ok()
}

/// Grava o token. No `--enrolar` (elevado). Escrita atômica: temp na mesma pasta + rename.
pub fn gravar(token: &str) -> Result<(), ErroCofre> {
    use std::io::Write;
    let pasta = pasta()?;
    std::fs::create_dir_all(&pasta).map_err(|e| ErroCofre::Falha(e.to_string()))?;
    // Toma a posse e fecha a ACL da PASTA antes de escrever (pode ter sido pré-criada por outro
    // usuário, que seria o dono — B1a).
    #[cfg(windows)]
    fechar_acl(&pasta, false)?;

    let bytes = proteger(token.as_bytes())?;
    let arquivo = pasta.join(ARQUIVO);
    let temp = pasta.join("device.token.tmp");

    // Nunca escreve por cima de um temp pré-existente (de outro dono): create_new falha se existir,
    // então o token não cai num arquivo que outro usuário leia (B1b). Na dúvida, Falha.
    let _ = std::fs::remove_file(&temp);
    let mut opcoes = std::fs::OpenOptions::new();
    opcoes.write(true).create_new(true);
    // Dev no Mac: sem DPAPI nem ACL; nasce 0600 já no open, não legível por outros (B3/N3).
    #[cfg(not(windows))]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opcoes.mode(0o600);
    }
    let mut f = opcoes.open(&temp).map_err(|e| ErroCofre::Falha(format!("temp: {e}")))?;
    f.write_all(&bytes).map_err(|e| ErroCofre::Falha(e.to_string()))?;
    f.sync_all().map_err(|e| ErroCofre::Falha(e.to_string()))?;
    drop(f);

    std::fs::rename(&temp, &arquivo).map_err(|e| ErroCofre::Falha(e.to_string()))?;
    // Fecha a ACL e toma a posse também do ARQUIVO final (B1a).
    #[cfg(windows)]
    fechar_acl(&arquivo, true)?;
    Ok(())
}

/// Prova, ANTES de consumir o código no Hub, que dá para proteger a pasta e escrever no cofre
/// (precisa de elevação). Sem isso, um --enrolar sem admin consumiria o código e só falharia ao
/// gravar. Cria a pasta com a ACL e escreve/apaga um arquivo de teste; não toca no device.token.
pub fn ensaio() -> Result<(), ErroCofre> {
    let pasta = pasta()?;
    std::fs::create_dir_all(&pasta).map_err(|e| ErroCofre::Falha(e.to_string()))?;
    #[cfg(windows)]
    fechar_acl(&pasta, false)?;
    let probe = pasta.join("device.token.probe");
    let _ = std::fs::remove_file(&probe);
    {
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&probe)
            .map_err(|e| ErroCofre::Falha(format!("sem escrita no cofre: {e}")))?;
        f.write_all(b"ok").map_err(|e| ErroCofre::Falha(e.to_string()))?;
    }
    let _ = std::fs::remove_file(&probe);
    Ok(())
}

/// Lê o token. `NaoEnrolado` se o arquivo não existe.
pub fn ler() -> Result<String, ErroCofre> {
    let caminho = caminho()?;
    let bytes = match std::fs::read(&caminho) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(ErroCofre::NaoEnrolado),
        Err(e) => return Err(ErroCofre::Falha(e.to_string())),
    };
    let claro = desproteger(&bytes)?;
    String::from_utf8(claro).map_err(|_| ErroCofre::Falha("token corrompido".into()))
}

/// Apaga o token (revogado pelo Hub).
pub fn apagar() {
    if let Ok(c) = caminho() {
        let _ = std::fs::remove_file(c);
    }
}

// ----- Windows: DPAPI de máquina + ACL -----

#[cfg(windows)]
/// Herança desligada, dono = SYSTEM, e só SYSTEM (S-1-5-18) e Administradores (S-1-5-32-544).
/// SID em vez do nome (localizado). `arquivo`=false aplica a herança de contêiner na pasta (OI)(CI);
/// true é o device.token (sem (OI)(CI)). O --enrolar roda elevado, então o /setowner tem posse.
fn fechar_acl(alvo: &std::path::Path, arquivo: bool) -> Result<(), ErroCofre> {
    use std::os::windows::process::CommandExt;
    use std::process::Command;
    const SEM_JANELA: u32 = 0x0800_0000;
    let raiz = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
    let icacls = format!(r"{raiz}\System32\icacls.exe");
    let heranca = if arquivo { "F" } else { "(OI)(CI)F" };
    let mut cmd = Command::new(icacls);
    cmd.arg(alvo).args(["/setowner", "*S-1-5-18"]);
    if !arquivo {
        // Num objeto único o /T não faz nada; na pasta recai no conteúdo. Sem /C: se o icacls
        // tropeçar em algo plantado dentro, melhor Falha do que seguir mascarando o erro.
        cmd.arg("/T");
    }
    let saida = cmd
        .args(["/inheritance:r"])
        .args(["/grant:r", &format!("*S-1-5-18:{heranca}")])
        .args(["/grant:r", &format!("*S-1-5-32-544:{heranca}")])
        .creation_flags(SEM_JANELA)
        .output()
        .map_err(|e| ErroCofre::Falha(format!("icacls: {e}")))?;
    if saida.status.success() {
        Ok(())
    } else {
        Err(ErroCofre::Falha("icacls não aplicou a ACL".into()))
    }
}

#[cfg(windows)]
fn proteger(claro: &[u8]) -> Result<Vec<u8>, ErroCofre> {
    dpapi(claro, true)
}

#[cfg(windows)]
fn desproteger(cifrado: &[u8]) -> Result<Vec<u8>, ErroCofre> {
    dpapi(cifrado, false)
}

#[cfg(windows)]
fn dpapi(entrada: &[u8], proteger: bool) -> Result<Vec<u8>, ErroCofre> {
    use windows_sys::Win32::Security::Cryptography::{
        CryptProtectData, CryptUnprotectData, CRYPTPROTECT_LOCAL_MACHINE, CRYPT_INTEGER_BLOB,
    };
    use windows_sys::Win32::Foundation::LocalFree;

    let mut dentro = CRYPT_INTEGER_BLOB {
        cbData: entrada.len() as u32,
        pbData: entrada.as_ptr() as *mut u8,
    };
    let mut fora = CRYPT_INTEGER_BLOB { cbData: 0, pbData: std::ptr::null_mut() };
    let ok = unsafe {
        if proteger {
            CryptProtectData(
                &mut dentro,
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                CRYPTPROTECT_LOCAL_MACHINE,
                &mut fora,
            )
        } else {
            CryptUnprotectData(
                &mut dentro,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                CRYPTPROTECT_LOCAL_MACHINE,
                &mut fora,
            )
        }
    };
    if ok == 0 {
        return Err(ErroCofre::Falha("DPAPI falhou".into()));
    }
    let saida =
        unsafe { std::slice::from_raw_parts(fora.pbData, fora.cbData as usize) }.to_vec();
    unsafe { LocalFree(fora.pbData as _) };
    Ok(saida)
}

// ----- Mac (dev): sem cifra -----

#[cfg(not(windows))]
fn proteger(claro: &[u8]) -> Result<Vec<u8>, ErroCofre> {
    Ok(claro.to_vec())
}

#[cfg(not(windows))]
fn desproteger(cifrado: &[u8]) -> Result<Vec<u8>, ErroCofre> {
    Ok(cifrado.to_vec())
}
