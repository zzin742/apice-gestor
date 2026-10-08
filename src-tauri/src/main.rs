// Sem console extra no Windows em release.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    let args: Vec<String> = std::env::args().collect();
    // Enrolamento pelo script de instalação: sem janela, só stdout e código de saída.
    if args.get(1).map(String::as_str) == Some("--enrolar") {
        apice_gestor_lib::usar_console_do_pai();
        let codigo = args.get(2).map(String::as_str).unwrap_or("");
        std::process::exit(apice_gestor_lib::enrolar(codigo));
    }
    apice_gestor_lib::run()
}
