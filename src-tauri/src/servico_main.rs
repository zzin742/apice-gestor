// Binário do serviço do Windows (apice-gestor-servico.exe). Sem janela: nunca sobe a webview.
#![cfg_attr(all(not(debug_assertions), windows), windows_subsystem = "windows")]

fn main() {
    std::process::exit(apice_gestor_lib::rodar_servico());
}
