//! A janela do painel: tamanho com âncora, arraste, posição lembrada por máquina, e a garantia
//! de que ela nunca fica fora da área útil de um monitor. Contrato combinado com a Prisma:
//! `painel_tamanho`, `painel_arrastar`, `painel_posicao` e o evento `painel://posicao`, tudo em
//! px lógicos.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, Monitor, Runtime, WebviewWindow};

/// Tamanho do painel antes da interface medir a cápsula, em px lógicos.
const TAMANHO_INICIAL: (f64, f64) = (240.0, 82.0);
/// Quanto esperar depois do último movimento para considerar a janela parada.
const PARADA: Duration = Duration::from_millis(300);
/// De quanto em quanto tempo conferir se os monitores mudaram.
const INTERVALO_MONITORES: Duration = Duration::from_secs(10);
const EVENTO_POSICAO: &str = "painel://posicao";
const ARQUIVO_POSICAO: &str = "posicao.json";

/// Retângulo em px físicos, coordenadas de tela.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
struct Retangulo {
    x: i32,
    y: i32,
    largura: i32,
    altura: i32,
}

impl Retangulo {
    fn de_area(m: &Monitor) -> Self {
        let a = m.work_area();
        Self { x: a.position.x, y: a.position.y, largura: a.size.width as i32, altura: a.size.height as i32 }
    }

    fn contem(&self, r: &Retangulo) -> bool {
        r.x >= self.x
            && r.y >= self.y
            && r.x + r.largura <= self.x + self.largura
            && r.y + r.altura <= self.y + self.altura
    }

    fn contem_ponto(&self, x: i32, y: i32) -> bool {
        x >= self.x && y >= self.y && x < self.x + self.largura && y < self.y + self.altura
    }

    /// Empurra `r` para dentro desta área (se for maior que ela, encosta no canto superior esquerdo).
    fn prender(&self, r: Retangulo) -> Retangulo {
        let x = r.x.min(self.x + self.largura - r.largura).max(self.x);
        let y = r.y.min(self.y + self.altura - r.altura).max(self.y);
        Retangulo { x, y, ..r }
    }
}

/// O que fica gravado em `%LOCALAPPDATA%\br.com.apice.gestor\posicao.json`. Nada de segredo.
#[derive(Serialize, Deserialize)]
struct PosicaoGravada {
    monitor: Option<String>,
    janela: Retangulo,
}

/// Payload de `painel_posicao` e do evento: janela relativa à área útil do monitor onde está.
#[derive(Clone, Serialize)]
pub struct Posicao {
    x: f64,
    y: f64,
    largura: f64,
    altura: f64,
    area: Area,
}

#[derive(Clone, Serialize)]
struct Area {
    largura: f64,
    altura: f64,
}

/// Contador de movimentos: só o último movimento de uma rajada grava e avisa a interface.
#[derive(Default)]
pub struct Movimento(Arc<AtomicU64>);

fn retangulo_atual<R: Runtime>(w: &WebviewWindow<R>) -> tauri::Result<Retangulo> {
    let p = w.outer_position()?;
    let s = w.outer_size()?;
    Ok(Retangulo { x: p.x, y: p.y, largura: s.width as i32, altura: s.height as i32 })
}

/// Monitor cuja área útil contém o ponto; senão o principal; senão qualquer um.
fn monitor_do_ponto<R: Runtime>(w: &WebviewWindow<R>, x: i32, y: i32) -> Option<Monitor> {
    let monitores = w.available_monitors().ok()?;
    monitores
        .iter()
        .find(|m| Retangulo::de_area(m).contem_ponto(x, y))
        .cloned()
        .or_else(|| w.primary_monitor().ok().flatten())
        .or_else(|| monitores.into_iter().next())
}

fn monitor_da_janela<R: Runtime>(w: &WebviewWindow<R>, r: &Retangulo) -> Option<Monitor> {
    monitor_do_ponto(w, r.x + r.largura / 2, r.y + r.altura / 2)
}

/// A janela inteira cabe na área útil de algum monitor ligado agora?
fn visivel<R: Runtime>(w: &WebviewWindow<R>, r: &Retangulo) -> bool {
    w.available_monitors()
        .map(|ms| ms.iter().any(|m| Retangulo::de_area(m).contem(r)))
        .unwrap_or(false)
}

/// Canto padrão: inferior direito da área útil do monitor principal, acima da barra de tarefas.
fn canto_padrao<R: Runtime>(w: &WebviewWindow<R>, largura: i32, altura: i32) -> Option<Retangulo> {
    let m = w.primary_monitor().ok().flatten()?;
    let a = Retangulo::de_area(&m);
    Some(Retangulo { x: a.x + a.largura - largura, y: a.y + a.altura - altura, largura, altura })
}

/// Redimensiona mantendo parado o canto `ancora` da janela, e prende na área útil.
pub fn redimensionar<R: Runtime>(
    w: &WebviewWindow<R>,
    largura: f64,
    altura: f64,
    ancora: &str,
) -> tauri::Result<()> {
    let atual = retangulo_atual(w)?;
    let monitor = monitor_da_janela(w, &atual);
    let escala = monitor.as_ref().map(Monitor::scale_factor).unwrap_or(1.0);
    let largura = (largura * escala).round() as i32;
    let altura = (altura * escala).round() as i32;

    let (direita, embaixo) = match ancora {
        "inferior-esquerdo" => (false, true),
        "superior-direito" => (true, false),
        "superior-esquerdo" => (false, false),
        _ => (true, true), // "inferior-direito", o padrão
    };
    let x = if direita { atual.x + atual.largura - largura } else { atual.x };
    let y = if embaixo { atual.y + atual.altura - altura } else { atual.y };
    let mut novo = Retangulo { x, y, largura, altura };
    if let Some(m) = &monitor {
        novo = Retangulo::de_area(m).prender(novo);
    }
    mover(w, novo)
}

/// Posição restaurada ao abrir: a gravada, se ainda couber em algum monitor; senão, o canto padrão.
pub fn posicionar_ao_abrir<R: Runtime>(w: &WebviewWindow<R>) -> tauri::Result<()> {
    let escala = w.primary_monitor()?.map(|m| m.scale_factor()).unwrap_or(1.0);
    let largura = (TAMANHO_INICIAL.0 * escala).round() as i32;
    let altura = (TAMANHO_INICIAL.1 * escala).round() as i32;
    let gravada = ler(w.app_handle()).map(|p| p.janela).filter(|r| visivel(w, r));
    match gravada.or_else(|| canto_padrao(w, largura, altura)) {
        Some(r) => mover(w, r),
        None => w.set_size(tauri::LogicalSize::new(TAMANHO_INICIAL.0, TAMANHO_INICIAL.1)),
    }
}

/// "Voltar ao canto" da bandeja: canto padrão no tamanho de agora, grava e avisa.
pub fn voltar_ao_canto<R: Runtime>(w: &WebviewWindow<R>) -> tauri::Result<()> {
    let atual = retangulo_atual(w)?;
    if let Some(r) = canto_padrao(w, atual.largura, atual.altura) {
        mover(w, r)?;
    }
    // O movimento chega como WindowEvent::Moved e passa por `ao_mover`, que grava e emite.
    Ok(())
}

/// Chamado a cada `WindowEvent::Moved`. Depois de `PARADA` sem movimento novo: prende na área
/// útil se precisar, grava a posição e emite `painel://posicao`.
pub fn ao_mover<R: Runtime>(w: &WebviewWindow<R>) {
    let contador = w.state::<Movimento>().0.clone();
    let meu = contador.fetch_add(1, Ordering::SeqCst) + 1;
    let w = w.clone();
    std::thread::spawn(move || {
        std::thread::sleep(PARADA);
        if contador.load(Ordering::SeqCst) != meu {
            return; // ainda se mexendo
        }
        let Ok(atual) = retangulo_atual(&w) else { return };
        if let Some(m) = monitor_da_janela(&w, &atual) {
            let preso = Retangulo::de_area(&m).prender(atual);
            if preso != atual {
                // Gera outro Moved; esse sim grava e emite.
                let _ = mover(&w, preso);
                return;
            }
        }
        gravar(&w, &atual);
        if let Ok(p) = posicao(&w) {
            let _ = w.emit(EVENTO_POSICAO, p);
        }
    });
}

/// A cada `INTERVALO_MONITORES`, se a lista de monitores mudou e a janela ficou fora da área
/// útil de todos, volta ao canto padrão (resolução trocada, monitor desligado, notebook fora da base).
pub fn vigiar_monitores<R: Runtime>(w: WebviewWindow<R>) {
    std::thread::spawn(move || {
        let mut anterior = assinatura(&w);
        loop {
            std::thread::sleep(INTERVALO_MONITORES);
            let agora = assinatura(&w);
            if agora == anterior {
                continue;
            }
            anterior = agora;
            if let Ok(r) = retangulo_atual(&w) {
                if !visivel(&w, &r) {
                    if let Some(canto) = canto_padrao(&w, r.largura, r.altura) {
                        let _ = mover(&w, canto);
                    }
                }
            }
        }
    });
}

fn assinatura<R: Runtime>(w: &WebviewWindow<R>) -> String {
    w.available_monitors()
        .map(|ms| {
            ms.iter()
                .map(|m| format!("{:?}{:?}{}", m.name(), Retangulo::de_area(m), m.scale_factor()))
                .collect::<Vec<_>>()
                .join("|")
        })
        .unwrap_or_default()
}

/// Payload de `painel_posicao`/`painel://posicao`, em px lógicos.
pub fn posicao<R: Runtime>(w: &WebviewWindow<R>) -> tauri::Result<Posicao> {
    let r = retangulo_atual(w)?;
    let monitor = monitor_da_janela(w, &r);
    let escala = monitor.as_ref().map(Monitor::scale_factor).unwrap_or(1.0);
    let area = monitor.as_ref().map(Retangulo::de_area).unwrap_or(r);
    let l = |v: i32| v as f64 / escala;
    Ok(Posicao {
        x: l(r.x - area.x),
        y: l(r.y - area.y),
        largura: l(r.largura),
        altura: l(r.altura),
        area: Area { largura: l(area.largura), altura: l(area.altura) },
    })
}

fn caminho<R: Runtime>(app: &AppHandle<R>) -> Option<PathBuf> {
    app.path().app_local_data_dir().ok().map(|d| d.join(ARQUIVO_POSICAO))
}

fn ler<R: Runtime>(app: &AppHandle<R>) -> Option<PosicaoGravada> {
    let texto = std::fs::read_to_string(caminho(app)?).ok()?;
    serde_json::from_str(&texto).ok()
}

fn gravar<R: Runtime>(w: &WebviewWindow<R>, r: &Retangulo) {
    let Some(arquivo) = caminho(w.app_handle()) else { return };
    let monitor = monitor_da_janela(w, r).and_then(|m| m.name().cloned());
    let Ok(texto) = serde_json::to_string_pretty(&PosicaoGravada { monitor, janela: *r }) else {
        return;
    };
    if let Some(pasta) = arquivo.parent() {
        let _ = std::fs::create_dir_all(pasta);
    }
    // Grava num temporário e renomeia: desligar no meio não deixa um arquivo pela metade.
    let temporario = arquivo.with_extension("json.tmp");
    if std::fs::write(&temporario, texto).is_ok() {
        let _ = std::fs::rename(&temporario, &arquivo);
    }
}

/// No Windows, posição e tamanho numa chamada só: sem o quadro em que a cápsula "pula".
#[cfg(windows)]
fn mover<R: Runtime>(w: &WebviewWindow<R>, r: Retangulo) -> tauri::Result<()> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SetWindowPos, SWP_NOACTIVATE, SWP_NOOWNERZORDER, SWP_NOZORDER,
    };
    let hwnd = w.hwnd()?;
    unsafe {
        SetWindowPos(
            hwnd.0 as _,
            std::ptr::null_mut(),
            r.x,
            r.y,
            r.largura,
            r.altura,
            SWP_NOZORDER | SWP_NOOWNERZORDER | SWP_NOACTIVATE,
        );
    }
    Ok(())
}

#[cfg(not(windows))]
fn mover<R: Runtime>(w: &WebviewWindow<R>, r: Retangulo) -> tauri::Result<()> {
    use tauri::{PhysicalPosition, PhysicalSize};
    w.set_size(PhysicalSize::new(r.largura as u32, r.altura as u32))?;
    w.set_position(PhysicalPosition::new(r.x, r.y))
}
