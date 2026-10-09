//! Watchdog da lista de bloqueio por máquina (contrato §9). Roda **no serviço** (SYSTEM).
//!
//! A invariante que não se negocia (§9.1): a **allowlist de protegidos é a primeira regra**, antes
//! de qualquer casamento com a lista. Um processo protegido **nunca** é encerrado, mesmo que um item
//! mal montado aponte para ele — no máximo vira `refused_protected`. A allowlist é **embutida**,
//! não vem do Hub e não é editável remotamente.
//!
//! O agente **só encerra** o processo (§9.1): não apaga, não desinstala, não bloqueia recurso.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};

use crate::api::Api;

/// Um processo em execução, no que interessa para casar com a lista.
#[derive(Clone, Debug)]
pub struct Processo {
    pub pid: u32,
    pub nome: String,           // nome da imagem, ex.: "jogo.exe"
    pub caminho: String,        // caminho completo, ou "" se não obtido
    pub publisher: Option<String>, // sujeito do certificado Authenticode, se assinado e obtido
}

/// Critério forte do item (§9.3). Nome sozinho nunca é aceito.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Criterio {
    NameAndPath,
    NameAndPublisher,
    PathExact,
}

/// Um item da lista (§9.2).
#[derive(Clone, Debug)]
pub struct Item {
    pub id: String,
    pub nome: String,
    pub criterio: Criterio,
    pub caminho: Option<String>,   // com curinga só em segmento de usuário (\Users\*\)
    pub publisher: Option<String>,
}

/// O que o watchdog decide para um processo.
#[derive(Clone, Debug, PartialEq)]
pub enum Acao {
    Encerrar,          // casou com um item e não é protegido
    RecusarProtegido,  // casou com um item, mas é protegido: não encerra, reporta
}

/// Avalia um processo contra a lista. A allowlist (§9.1) vem **antes** do casamento: se o processo
/// é protegido e algum item casa, o resultado é `RecusarProtegido`, nunca `Encerrar`. `None` = nada
/// a fazer (não casou, ou casou sendo protegido sem nada a reportar — ver abaixo). Devolve o id do
/// item que casou, para o relatório (§9.5).
pub fn avaliar(proc: &Processo, itens: &[Item]) -> Option<(String, Acao)> {
    let protegido = eh_protegido(proc);
    for item in itens {
        if casa(proc, item) {
            // Protegido nunca é encerrado — a allowlist vence, estruturalmente.
            let acao = if protegido { Acao::RecusarProtegido } else { Acao::Encerrar };
            return Some((item.id.clone(), acao));
        }
    }
    None
}

/// Allowlist embutida (§9.1): SO, o próprio Gestor e antivírus. Nunca encerra nada aqui.
fn eh_protegido(proc: &Processo) -> bool {
    nome_de_sistema(&proc.nome)
        || caminho_protegido(&proc.caminho)
        || eh_o_gestor(proc)
        || eh_antivirus(proc)
}

/// Processos essenciais ao logon e à sessão. Comparação sem distinção de caixa.
fn nome_de_sistema(nome: &str) -> bool {
    const NOMES: &[&str] = &[
        "system", "registry", "idle",
        "smss.exe", "csrss.exe", "wininit.exe", "winlogon.exe", "services.exe",
        "lsass.exe", "lsaiso.exe", "svchost.exe", "spoolsv.exe",
        "explorer.exe", "dwm.exe", "fontdrvhost.exe", "sihost.exe",
        "taskhostw.exe", "ctfmon.exe", "userinit.exe", "logonui.exe",
        "runtimebroker.exe", "shellexperiencehost.exe", "searchhost.exe",
        "startmenuexperiencehost.exe", "wininit.exe", "conhost.exe",
    ];
    let n = nome.to_ascii_lowercase();
    NOMES.contains(&n.as_str())
}

/// Qualquer coisa dentro da pasta do Windows (System32 etc.) é do SO — protege de sobra.
fn caminho_protegido(caminho: &str) -> bool {
    if caminho.is_empty() {
        return false;
    }
    let c = caminho.to_ascii_lowercase().replace('/', "\\");
    let mut raiz = std::env::var("SystemRoot")
        .unwrap_or_else(|_| r"C:\Windows".into())
        .to_ascii_lowercase();
    // Barra final: c:\windows\ não protege c:\windows.old (E7e).
    if !raiz.ends_with('\\') {
        raiz.push('\\');
    }
    c.starts_with(&raiz)
}

fn eh_o_gestor(proc: &Processo) -> bool {
    let n = proc.nome.to_ascii_lowercase();
    if n == "apice-gestor.exe" || n == "apice-gestor-servico.exe" {
        return true;
    }
    let c = proc.caminho.to_ascii_lowercase();
    c.contains(r"\ápice gestor\") || c.contains(r"\apice gestor\")
}

/// Defender e caminho do Defender. Produtos de terceiros registrados no Windows Security Center
/// entram aqui quando a leitura do WSC for implementada (marcado para a fase do serviço).
fn eh_antivirus(proc: &Processo) -> bool {
    let n = proc.nome.to_ascii_lowercase();
    const AV: &[&str] = &[
        "msmpeng.exe", "nissrv.exe", "mpdefendercoreservice.exe",
        "securityhealthservice.exe", "smartscreen.exe",
    ];
    if AV.contains(&n.as_str()) {
        return true;
    }
    let c = proc.caminho.to_ascii_lowercase();
    c.contains(r"\windows defender\") || c.contains(r"\microsoft\windows defender\")
}

/// Casa o processo com um item, pelo critério forte. Nome é sempre necessário e sem distinção de
/// caixa; o critério acrescenta caminho ou editor. Sem o dado exigido (ex.: publisher ausente), não
/// casa — nunca encerra na dúvida.
fn casa(proc: &Processo, item: &Item) -> bool {
    let mesmo_nome = proc.nome.eq_ignore_ascii_case(&item.nome);
    match item.criterio {
        Criterio::PathExact => match &item.caminho {
            Some(padrao) => caminho_casa(padrao, &proc.caminho),
            None => false,
        },
        Criterio::NameAndPath => {
            mesmo_nome && matches!(&item.caminho, Some(p) if caminho_casa(p, &proc.caminho))
        }
        Criterio::NameAndPublisher => {
            mesmo_nome
                && match (&item.publisher, &proc.publisher) {
                    (Some(esp), Some(real)) => esp.eq_ignore_ascii_case(real),
                    _ => false, // não assinado / não obtido: nunca casa
                }
        }
    }
}

/// Compara caminhos sem distinção de caixa, com no máximo um curinga `*` (segmento de usuário,
/// ex.: `C:\Users\*\Downloads\x.exe`). Sem `*`, é igualdade. Barras normalizadas para `\`.
fn caminho_casa(padrao: &str, real: &str) -> bool {
    let norm = |s: &str| s.to_ascii_lowercase().replace('/', "\\");
    let (padrao, real) = (norm(padrao), norm(real));
    if real.is_empty() {
        return false;
    }
    match padrao.split_once('*') {
        None => padrao == real,
        Some((antes, depois)) => {
            if !real.starts_with(&antes) || !real.ends_with(&depois) {
                return false;
            }
            if real.len() < antes.len() + depois.len() {
                return false;
            }
            // E4: o * é um único segmento de usuário — o que ele cobre não pode ter barra, senão
            // C:\Users\*\x.exe casaria C:\Users\joao\Temp\sub\x.exe.
            let coberto = &real[antes.len()..real.len() - depois.len()];
            !coberto.contains('\\')
        }
    }
}

// ----- Windows: enumerar e encerrar processos -----

/// Lista os processos em execução (pid, nome, caminho). Publisher fica `None` nesta fase; a leitura
/// do Authenticode (para `name_and_publisher`) entra junto com o serviço. Sem publisher, esse
/// critério não casa — lado seguro.
#[cfg(windows)]
pub fn processos() -> Vec<Processo> {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
    };

    let mut lista = Vec::new();
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap.is_null() {
            return lista;
        }
        let mut entrada: PROCESSENTRY32W = std::mem::zeroed();
        entrada.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut ok = Process32FirstW(snap, &mut entrada);
        while ok != 0 {
            let nome = u16_ate_nul(&entrada.szExeFile);
            let pid = entrada.th32ProcessID;
            let caminho = caminho_do_processo(pid);
            lista.push(Processo { pid, nome, caminho, publisher: None });
            ok = Process32NextW(snap, &mut entrada);
        }
        CloseHandle(snap);
    }
    lista
}

#[cfg(windows)]
fn caminho_do_processo(pid: u32) -> String {
    use windows_sys::Win32::Foundation::{CloseHandle, MAX_PATH};
    use windows_sys::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() {
            return String::new();
        }
        let mut buf = [0u16; MAX_PATH as usize];
        let mut tam = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(h, 0, buf.as_mut_ptr(), &mut tam);
        CloseHandle(h);
        if ok == 0 {
            String::new()
        } else {
            String::from_utf16_lossy(&buf[..tam as usize])
        }
    }
}

/// Encerra o processo (§9.1: só encerrar). `true` se o SO aceitou. Protegido do Windows (PPL, crítico)
/// falha aqui, de propósito — é a última linha de defesa do próprio SO.
#[cfg(windows)]
pub fn encerrar(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{OpenProcess, TerminateProcess, PROCESS_TERMINATE};
    unsafe {
        let h = OpenProcess(PROCESS_TERMINATE, 0, pid);
        if h.is_null() {
            return false;
        }
        let ok = TerminateProcess(h, 1);
        CloseHandle(h);
        ok != 0
    }
}

#[cfg(windows)]
fn u16_ate_nul(buf: &[u16]) -> String {
    let fim = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..fim])
}

#[cfg(not(windows))]
pub fn processos() -> Vec<Processo> {
    Vec::new()
}

#[cfg(not(windows))]
pub fn encerrar(_pid: u32) -> bool {
    false
}

// ----- Laço do watchdog (roda no serviço) -----

/// §9.4: intervalo de `GET me/blocklist`, padrão e mínimo quando ligado.
const POLL_PADRAO: u64 = 120;
const POLL_MINIMO: u64 = 30;
/// Enforcement desligado (`blocklist_poll_interval_seconds = 0`): reavalia o config de vez em quando.
const POLL_DESLIGADO: u64 = 300;

/// Laço do §9: a cada ciclo relê a lista e varre os processos. Reavalia os já abertos (varre todos),
/// não só os que abrem depois (§9.4). O chamador (serviço) spawna no seu runtime.
pub async fn rodar(api: Arc<Api>) {
    // refused_protected é reportado uma vez por item nesta execução, para não repetir a cada ciclo
    // (terminated/terminate_failed são ações reais e podem repetir: "reabriu, encerra de novo").
    let mut ja_recusados: HashSet<String> = HashSet::new();
    loop {
        let intervalo = ciclo(&api, &mut ja_recusados).await;
        tokio::time::sleep(Duration::from_secs(intervalo)).await;
    }
}

async fn ciclo(api: &Api, ja_recusados: &mut HashSet<String>) -> u64 {
    let poll = match api.get("me/config").await {
        Ok(cfg) => cfg
            .get("blocklist_poll_interval_seconds")
            .and_then(|v| v.as_u64())
            .unwrap_or(POLL_PADRAO),
        Err(_) => return POLL_PADRAO,
    };
    if poll == 0 {
        return POLL_DESLIGADO; // enforcement desligado pelo admin
    }
    varrer(api, ja_recusados).await;
    poll.max(POLL_MINIMO)
}

async fn varrer(api: &Api, ja_recusados: &mut HashSet<String>) {
    // me/blocklist suporta ETag/304 (muda pouco); api.get devolve a última lista vigente.
    let Ok(lista) = api.get("me/blocklist").await else { return };
    let itens = itens_de(&lista);
    if itens.is_empty() {
        return;
    }
    let mut eventos = Vec::new();
    for proc in processos() {
        let Some((entry_id, acao)) = avaliar(&proc, &itens) else { continue };
        let action = match acao {
            // A allowlist vence: protegido nunca é encerrado. Reporta uma vez por item.
            Acao::RecusarProtegido => {
                if !ja_recusados.insert(entry_id.clone()) {
                    continue;
                }
                "refused_protected"
            }
            Acao::Encerrar => {
                if encerrar(proc.pid) {
                    "terminated"
                } else {
                    "terminate_failed"
                }
            }
        };
        // Log de domínio (E7d): só o que casou com um item — entry_id + nome, nunca a lista do que
        // a pessoa abre.
        crate::diario::registrar(&format!("bloqueio {entry_id} {} {action}", proc.nome));
        // §9.5: reporta só o que casou com um item — nunca a lista do que a pessoa abre.
        eventos.push(json!({
            "entry_id": entry_id,
            "name": proc.nome,
            "action": action,
            "at": crate::api::agora(),
        }));
    }
    if !eventos.is_empty() {
        let _ = api.post("me/blocklist/events", &json!({ "events": eventos })).await;
    }
}

/// Converte os `entries` do contrato (§9.2) em `Item`. Item sem critério forte (nome sozinho) ou
/// com `match` desconhecido é descartado — nunca casa (§9.3).
fn itens_de(lista: &Value) -> Vec<Item> {
    lista
        .get("entries")
        .and_then(|e| e.as_array())
        .map(|arr| arr.iter().filter_map(item_de).collect())
        .unwrap_or_default()
}

fn item_de(v: &Value) -> Option<Item> {
    let criterio = match v.get("match")?.as_str()? {
        "name_and_path" => Criterio::NameAndPath,
        "name_and_publisher" => Criterio::NameAndPublisher,
        "path_exact" => Criterio::PathExact,
        _ => return None, // nome sozinho / desconhecido: ignora
    };
    Some(Item {
        id: v.get("id")?.as_str()?.to_owned(),
        nome: v.get("name")?.as_str()?.to_owned(),
        criterio,
        caminho: v.get("path").and_then(|p| p.as_str()).map(str::to_owned),
        publisher: v.get("publisher").and_then(|p| p.as_str()).map(str::to_owned),
    })
}

#[cfg(test)]
mod testes {
    use super::*;

    fn proc(nome: &str, caminho: &str) -> Processo {
        Processo { pid: 10, nome: nome.into(), caminho: caminho.into(), publisher: None }
    }
    fn item(id: &str, nome: &str, criterio: Criterio, caminho: Option<&str>) -> Item {
        Item { id: id.into(), nome: nome.into(), criterio, caminho: caminho.map(Into::into), publisher: None }
    }

    #[test]
    fn allowlist_vence_sempre() {
        // Mesmo com um item apontando certinho para o explorer, protegido nunca é encerrado.
        let p = proc("explorer.exe", r"C:\Windows\explorer.exe");
        let it = item("blk1", "explorer.exe", Criterio::NameAndPath, Some(r"C:\Windows\explorer.exe"));
        assert_eq!(avaliar(&p, &[it]), Some(("blk1".into(), Acao::RecusarProtegido)));
    }

    #[test]
    fn gestor_e_antivirus_protegidos() {
        for (nome, cam) in [
            ("apice-gestor-servico.exe", r"C:\Program Files\Ápice Gestor\apice-gestor-servico.exe"),
            ("MsMpEng.exe", r"C:\ProgramData\Microsoft\Windows Defender\Platform\x\MsMpEng.exe"),
        ] {
            let p = proc(nome, cam);
            let it = item("x", nome, Criterio::PathExact, Some(cam));
            assert_eq!(avaliar(&p, &[it]).map(|(_, a)| a), Some(Acao::RecusarProtegido));
        }
    }

    #[test]
    fn app_comum_casa_e_encerra() {
        let p = proc("instalador.exe", r"C:\Users\vinny\Downloads\instalador.exe");
        let it = item("blk2", "instalador.exe", Criterio::NameAndPath, Some(r"C:\Users\*\Downloads\instalador.exe"));
        assert_eq!(avaliar(&p, &[it]), Some(("blk2".into(), Acao::Encerrar)));
    }

    #[test]
    fn nome_sozinho_nao_casa_sem_criterio_forte() {
        // name_and_path sem caminho no item nunca casa.
        let p = proc("setup.exe", r"C:\Users\vinny\setup.exe");
        let it = item("blk3", "setup.exe", Criterio::NameAndPath, None);
        assert_eq!(avaliar(&p, &[it]), None);
    }

    #[test]
    fn publisher_so_casa_se_assinado_e_igual() {
        let mut p = proc("jogo.exe", r"C:\Games\jogo.exe");
        let mut it = item("blk4", "jogo.exe", Criterio::NameAndPublisher, None);
        it.publisher = Some("CN=Fulano Games Ltda".into());
        assert_eq!(avaliar(&p, &[it.clone()]), None); // sem publisher no processo
        p.publisher = Some("CN=Fulano Games Ltda".into());
        assert_eq!(avaliar(&p, &[it]), Some(("blk4".into(), Acao::Encerrar)));
    }

    #[test]
    fn curinga_nao_atravessa_barra() {
        // E4: o * cobre só um segmento; não pode casar subpasta.
        let p = proc("x.exe", r"C:\Users\joao\Temp\sub\x.exe");
        let it = item("b", "x.exe", Criterio::NameAndPath, Some(r"C:\Users\*\x.exe"));
        assert_eq!(avaliar(&p, &[it]), None);
    }

    #[test]
    fn windows_old_nao_e_protegido() {
        // E7e: c:\windows protege c:\windows\..., mas não c:\windows.old.
        std::env::set_var("SystemRoot", r"C:\Windows");
        assert!(super::caminho_protegido(r"C:\Windows\system32\x.exe"));
        assert!(!super::caminho_protegido(r"C:\Windows.old\x.exe"));
    }

    #[test]
    fn caminho_fora_do_curinga_nao_casa() {
        let p = proc("instalador.exe", r"C:\Temp\instalador.exe");
        let it = item("blk5", "instalador.exe", Criterio::NameAndPath, Some(r"C:\Users\*\Downloads\instalador.exe"));
        assert_eq!(avaliar(&p, &[it]), None);
    }
}
