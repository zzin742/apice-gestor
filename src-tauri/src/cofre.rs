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
///
/// Como o `fechar_acl` não recursa mais (sem /T), o `gravar()` não conserta conteúdo pré-plantado.
/// Então o ensaio GARANTE aqui, ainda ANTES do POST (código não consumido), que o `gravar()` não
/// vai falhar por ACL ou arquivo pré-existente de outro dono — senão o código seria QUEIMADO.
pub fn ensaio() -> Result<(), ErroCofre> {
    let pasta = pasta()?;
    std::fs::create_dir_all(&pasta).map_err(|e| ErroCofre::Falha(e.to_string()))?;
    // (B1) Sem /T não dá pra confiar que o alvo real é o esperado: recusa junction/symlink, que
    // desviaria o cofre pra fora da pasta protegida.
    #[cfg(windows)]
    recusar_reparse(&pasta)?;
    #[cfg(windows)]
    fechar_acl(&pasta, false)?;

    // device.token já plantado (por outro dono): toma posse/ACL dele AGORA. Falhou → Falha (código
    // não consumido); deu certo → o rename do gravar() cai sobre um arquivo que já é nosso.
    #[cfg(windows)]
    {
        let arquivo = pasta.join(ARQUIVO);
        if arquivo.exists() {
            fechar_acl(&arquivo, true)?;
        }
    }
    // .tmp/.probe velhos de outro dono travariam o `create_new` do gravar(): remove AGORA e FALHA
    // se não conseguir (nada de mascarar com `let _ =`).
    remover_cofre(&pasta.join("device.token.tmp"))?;
    let probe = pasta.join("device.token.probe");
    remover_cofre(&probe)?;
    {
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&probe)
            .map_err(|e| ErroCofre::Falha(format!("sem escrita no cofre: {e}")))?;
        f.write_all(b"ok").map_err(|e| ErroCofre::Falha(e.to_string()))?;
    }
    // Limpeza do nosso próprio probe: best-effort (sobra é inofensiva, o próximo ensaio remove).
    let _ = std::fs::remove_file(&probe);

    // (B1c) Prova que o DPAPI de máquina funciona NESTE PC: cifra e decifra um valor de teste,
    // ainda ANTES do POST (código não consumido). Sem isto, o `gravar()` só descobriria uma falha
    // de DPAPI DEPOIS de consumir o código uso-único — e ele seria QUEIMADO. Importa no rollout:
    // os PCs são clones da mesma imagem, e uma imagem mal sysprep-ada pode ter o DPAPI de máquina
    // quebrado. Não grava nada no cofre; o valor de teste nunca toca o disco.
    #[cfg(windows)]
    {
        let amostra: &[u8] = b"apice-gestor-ensaio";
        let cifrado = proteger(amostra).map_err(|e| match e {
            ErroCofre::Falha(m) => ErroCofre::Falha(format!("DPAPI (cifrar): {m}")),
            outro => outro,
        })?;
        let claro = desproteger(&cifrado).map_err(|e| match e {
            ErroCofre::Falha(m) => ErroCofre::Falha(format!("DPAPI (decifrar): {m}")),
            outro => outro,
        })?;
        if claro.as_slice() != amostra {
            return Err(ErroCofre::Falha("DPAPI: round-trip não confere".into()));
        }
    }
    Ok(())
}

/// Remove um arquivo do cofre: Ok se já não existe, Falha se existe e não saiu (dono alheio).
fn remover_cofre(p: &std::path::Path) -> Result<(), ErroCofre> {
    match std::fs::remove_file(p) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(ErroCofre::Falha(format!("não removeu {}: {e}", p.display()))),
    }
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

/// Recusa a pasta do cofre se ela for um ponto de reparse (junction/symlink). `symlink_metadata`
/// não segue o último componente, então o atributo aparece no próprio alvo. Três linhas de B1:
/// sem isso um atacante apontaria `%ProgramData%\ApiceGestor` pra fora e o cofre escreveria lá.
#[cfg(windows)]
fn recusar_reparse(pasta: &std::path::Path) -> Result<(), ErroCofre> {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    let meta = std::fs::symlink_metadata(pasta)
        .map_err(|e| ErroCofre::Falha(format!("symlink_metadata: {e}")))?;
    if meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(ErroCofre::Falha("pasta é junction/symlink".into()));
    }
    Ok(())
}

#[cfg(windows)]
/// Herança desligada, dono = Administradores (S-1-5-32-544), e só SYSTEM (S-1-5-18) e
/// Administradores no DACL. SID em vez do nome (localizado). `arquivo`=false aplica a herança
/// de contêiner na pasta (OI)(CI); true é o device.token (sem (OI)(CI)).
///
/// Duas causas distintas, as duas tratadas aqui:
///
/// 1. A 1ª falha na bancada (Vinny) era SINTAXE: `/setowner` junto de `/inheritance`+`/grant` numa
///    chamada só dá erro 87 ("Invalid parameter") em QUALQUER Windows. Por isso as DUAS chamadas
///    separadas abaixo. A 2ª falha foi erro 5 ("Acesso negado") do `/setowner /T` recursando sobre
///    um arquivo aberto na pasta compartilhada — resolvido tirando o /T (ver o bloco "SEM /T" mais
///    abaixo). O teste de um clique da CI não pegava a 1ª porque `base_url()` roda ANTES de
///    `ensaio()` em `api::enrolar` e aquele build não tem endereço: saía 4 antes de encostar no
///    cofre. Quem pega agora é o gate de cofre dedicado (porta fechada, exige saída 4 e nenhum
///    `cofre:` no stderr).
///
/// 2. Dono = Administradores (S-1-5-32-544), NÃO SYSTEM, MESMO com o comando bem formado: o
///    --enrolar roda como o admin ELEVADO, que é membro de Administradores; o icacls NÃO liga
///    SeRestorePrivilege sozinho, então `/setowner` para OUTRA conta (SYSTEM) daria acesso negado,
///    enquanto para um grupo do qual o chamador é membro dispensa esse privilégio. O B1 do Diretor
///    já permitia S-1-5-18 OU S-1-5-32-544; o furo fica fechado igual — o DACL abaixo só concede
///    SYSTEM + Administradores, então usuário comum continua sem ser dono e fora do DACL.
fn fechar_acl(alvo: &std::path::Path, arquivo: bool) -> Result<(), ErroCofre> {
    use std::os::windows::process::CommandExt;
    use std::process::Command;
    const SEM_JANELA: u32 = 0x0800_0000;
    let raiz = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
    let icacls = format!(r"{raiz}\System32\icacls.exe");
    let heranca = if arquivo { "F" } else { "(OI)(CI)F" };

    // SEM /T de propósito: a pasta é COMPARTILHADA — o serviço mantém o servico.log ABERTO, o
    // instalar.log vem do ps1 não elevado, e há msi-*.log. Recursar com /setowner bateria num
    // arquivo aberto/alheio e sairia com 5 ("Acesso negado") — foi o que a bancada pegou no
    // f90b27d. O device.token é travado À PARTE por fechar_acl(arquivo, true); a herança (OI)(CI)
    // na pasta faz os filhos NOVOS (token, probe) nascerem já só com SYSTEM + Administradores.
    //
    // Roda UMA invocação do icacls e, no erro, diz QUAL etapa falhou (setowner|acl) e carrega a
    // saída dele (caminho + mensagem do Windows, sem segredo) pra o log da Esteira não mostrar só
    // a frase genérica e a gente não voltar cega se o Vinny falhar de novo.
    let rodar = |etapa: &str, args: &[&str]| -> Result<(), ErroCofre> {
        let mut cmd = Command::new(&icacls);
        cmd.arg(alvo).args(args);
        let saida = cmd
            .creation_flags(SEM_JANELA)
            .output()
            .map_err(|e| ErroCofre::Falha(format!("{etapa}: icacls não rodou: {e}")))?;
        if saida.status.success() {
            return Ok(());
        }
        let mut texto = String::from_utf8_lossy(&saida.stderr).trim().to_string();
        let out = String::from_utf8_lossy(&saida.stdout);
        let out = out.trim();
        if !out.is_empty() {
            if !texto.is_empty() {
                texto.push_str(" | ");
            }
            texto.push_str(out);
        }
        Err(ErroCofre::Falha(format!(
            "{etapa} (icacls {}) saiu {:?}: {texto}",
            args.join(" "),
            saida.status.code()
        )))
    };

    // DUAS chamadas, NESTA ORDEM, com check de saída em cada: `/setowner` é uma FORMA SEPARADA da
    // linha do icacls — `icacls <alvo> /setowner <sid>`. Misturar com /inheritance|/grant dá erro
    // 87 ("Invalid parameter") em QUALQUER Windows (foi o que o gate do cofre pegou; o ac878b1
    // ainda juntava tudo numa chamada só).
    // A ORDEM importa: o /setowner dá a POSSE, e a posse dá WRITE_DAC implícito, então a 2ª chamada
    // reescreve o DACL MESMO num alvo pré-criado por outro usuário.
    // (1) dono = Administradores (ver acima: dispensa SeRestorePrivilege do admin elevado).
    rodar("setowner", &["/setowner", "*S-1-5-32-544"])?;
    // (2) herança desligada e DACL só SYSTEM + Administradores.
    let g_sys = format!("*S-1-5-18:{heranca}");
    let g_adm = format!("*S-1-5-32-544:{heranca}");
    rodar(
        "acl",
        &[
            "/inheritance:r",
            "/grant:r",
            g_sys.as_str(),
            "/grant:r",
            g_adm.as_str(),
        ],
    )
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
