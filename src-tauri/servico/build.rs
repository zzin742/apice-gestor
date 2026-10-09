// O build.rs da casca (tauri-build 2.7.1 → static_vcruntime) grava um msvcrt.lib vazio no OUT_DIR
// e emite um rustc-link-search para ele. Esse link-search PROPAGA para quem depende da lib da
// casca (este crate do serviço), mas os rustc-link-arg que desabilitam o msvcrt NÃO propagam —
// então o serviço resolveria /defaultlib:msvcrt no .lib vazio e ficaria sem runtime do C
// (LNK2001 mainCRTStartup, __CxxFrameHandler3). Aqui repetimos os mesmos link-args, só no MSVC,
// para o binário do serviço linkar o CRT estático, igual à casca.
// Lista copiada de tauri-build 2.7.1, src/static_vcruntime.rs — reconferir se o tauri-build subir.
fn main() {
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        for arg in [
            "/NODEFAULTLIB:libvcruntimed.lib",
            "/NODEFAULTLIB:vcruntime.lib",
            "/NODEFAULTLIB:vcruntimed.lib",
            "/NODEFAULTLIB:libcmtd.lib",
            "/NODEFAULTLIB:msvcrt.lib",
            "/NODEFAULTLIB:msvcrtd.lib",
            "/NODEFAULTLIB:libucrt.lib",
            "/NODEFAULTLIB:libucrtd.lib",
            "/DEFAULTLIB:libcmt.lib",
            "/DEFAULTLIB:libvcruntime.lib",
            "/DEFAULTLIB:ucrt.lib",
        ] {
            println!("cargo:rustc-link-arg={arg}");
        }
    }
}
