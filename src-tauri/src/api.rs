//! Chamadas autenticadas à API do Ápice Hub (docs/contrato-api.md).
//! O token sai do keychain só para o cabeçalho `Authorization`: nunca vai para a interface,
//! nunca vai para log.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use reqwest::{header, StatusCode};
use serde::Serialize;
use serde_json::Value;

/// Onde o token do dispositivo fica guardado (Credential Manager no Windows, Keychain no Mac).
const KEYCHAIN_SERVICO: &str = "br.com.apice.gestor";
const KEYCHAIN_CONTA: &str = "token-dispositivo";

/// Versão do contrato que o agente fala (§1.1).
const API_VERSAO: &str = "1";

/// Base da API. No build de release vem de `GESTOR_API_BASE` (o CI define); em dev, o mock local.
fn base_url() -> Option<&'static str> {
    match option_env!("GESTOR_API_BASE") {
        Some(base) => Some(base),
        None if cfg!(debug_assertions) => Some("http://127.0.0.1:8787/api/gestor"),
        None => None,
    }
}

/// Erro como a interface espera: `{ code, message }`, com os `code` do contrato (§1.4).
#[derive(Debug, Serialize)]
pub struct ErroApi {
    pub code: String,
    pub message: String,
    pub retry_after_seconds: Option<u64>,
}

impl ErroApi {
    fn novo(code: &str, message: &str) -> Self {
        Self { code: code.into(), message: message.into(), retry_after_seconds: None }
    }
}

/// Último corpo e ETag de cada rota, de um token só. Trocar de token apaga tudo (§1.3).
#[derive(Default)]
struct Cache {
    token: Option<u64>,
    rotas: HashMap<&'static str, (String, Value)>,
}

pub struct Api {
    cliente: reqwest::Client,
    cache: Mutex<Cache>,
    /// Para o heartbeat (§5): última busca de tarefas que deu certo e último `error.code`.
    pub tarefas_ok_em: Mutex<Option<String>>,
    pub ultimo_erro: Mutex<Option<String>>,
}

impl Api {
    pub fn new() -> Self {
        let cliente = reqwest::Client::builder()
            .user_agent(format!(
                "ApiceGestor/{} ({})",
                env!("CARGO_PKG_VERSION"),
                std::env::consts::OS
            ))
            .timeout(Duration::from_secs(20))
            // Nunca seguir redirecionamento: o reqwest descarta o Authorization ao trocar de host
            // (sem www → www devolveria 401 em laço) e o token não pode viajar para host inesperado.
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("cliente HTTP");
        Self {
            cliente,
            cache: Mutex::default(),
            tarefas_ok_em: Mutex::default(),
            ultimo_erro: Mutex::default(),
        }
    }

    /// GET autenticado com `If-None-Match`. Sempre devolve o corpo completo: o `304` vira o
    /// corpo guardado, e a interface nunca vê um `304`.
    pub async fn get(&self, rota: &'static str) -> Result<Value, ErroApi> {
        let resultado = self.get_sem_registro(rota).await;
        match &resultado {
            Ok(_) if rota == "me/tasks" => {
                *self.tarefas_ok_em.lock().unwrap() = Some(agora());
            }
            Ok(_) => {}
            Err(erro) => self.registrar_erro(erro),
        }
        resultado
    }

    /// POST autenticado, sem cache (heartbeat, §5).
    pub async fn post(&self, rota: &str, corpo: &Value) -> Result<Value, ErroApi> {
        let resultado = self.post_sem_registro(rota, corpo).await;
        if let Err(erro) = &resultado {
            self.registrar_erro(erro);
        }
        resultado
    }

    async fn post_sem_registro(&self, rota: &str, corpo: &Value) -> Result<Value, ErroApi> {
        let base = base_url().ok_or_else(sem_endereco)?;
        let token = ler_token()?;
        let resposta = self
            .cliente
            .post(format!("{base}/{rota}"))
            .bearer_auth(&token)
            .header("X-Gestor-Api", API_VERSAO)
            .json(corpo)
            .send()
            .await
            .map_err(|_| sem_conexao())?;
        if let Some(erro) = redirecionou(&resposta) {
            return Err(erro);
        }
        let status = resposta.status();
        let corpo: Option<Value> = resposta.json().await.ok();
        if status.is_success() {
            return Ok(corpo.unwrap_or_default());
        }
        Err(erro_do_corpo(status, corpo.as_ref()))
    }

    /// GET sem cache (ordens, §8.2: sem ETag, no-store). Não registra erro: a ausência de
    /// ordens é o caso normal e um erro aqui não deve ocupar `last_error_code`.
    pub async fn get_sem_cache(&self, rota: &str) -> Result<Value, ErroApi> {
        let base = base_url().ok_or_else(sem_endereco)?;
        let token = ler_token()?;
        let resposta = self
            .cliente
            .get(format!("{base}/{rota}"))
            .bearer_auth(&token)
            .header("X-Gestor-Api", API_VERSAO)
            .send()
            .await
            .map_err(|_| sem_conexao())?;
        if let Some(erro) = redirecionou(&resposta) {
            return Err(erro);
        }
        let status = resposta.status();
        let corpo: Option<Value> = resposta.json().await.ok();
        if status.is_success() {
            return corpo.ok_or_else(|| ErroApi::novo("server_error", "Resposta ilegível."));
        }
        Err(erro_do_corpo(status, corpo.as_ref()))
    }

    /// Reivindica a ordem (§8.2). `true` só com `200`: é a trava (a). 409, rede, 5xx, corpo
    /// ilegível — tudo vira `false`, e nada é executado nem repetido. Não registra erro
    /// (command_gone é esperado).
    pub async fn claim(&self, id: &str) -> bool {
        let Some(base) = base_url() else { return false };
        let Ok(token) = ler_token() else { return false };
        let resposta = self
            .cliente
            .post(format!("{base}/me/commands/{id}/claim"))
            .bearer_auth(&token)
            .header("X-Gestor-Api", API_VERSAO)
            .json(&serde_json::json!({}))
            .send()
            .await;
        matches!(resposta, Ok(r) if r.status() == StatusCode::OK)
    }

    /// Avisa que a execução falhou (§8.2, result). Melhor esforço; nada depende do retorno.
    pub async fn result_falhou(&self, id: &str, erro: &str) {
        let recorte: String = erro.chars().take(500).collect();
        let _ = self
            .post_sem_registro(
                &format!("me/commands/{id}/result"),
                &serde_json::json!({ "status": "failed", "error": recorte }),
            )
            .await;
    }

    /// `not_enrolled` é nosso, não veio do Hub: não entra em `last_error_code`.
    fn registrar_erro(&self, erro: &ErroApi) {
        if erro.code != "not_enrolled" {
            *self.ultimo_erro.lock().unwrap() = Some(erro.code.clone());
        }
    }

    async fn get_sem_registro(&self, rota: &'static str) -> Result<Value, ErroApi> {
        let base = base_url().ok_or_else(sem_endereco)?;
        let token = ler_token()?;
        let dono = impressao(&token);

        let etag = {
            let mut cache = self.cache.lock().unwrap();
            if cache.token != Some(dono) {
                *cache = Cache { token: Some(dono), ..Default::default() };
            }
            cache.rotas.get(rota).map(|(etag, _)| etag.clone())
        };

        let mut pedido = self
            .cliente
            .get(format!("{base}/{rota}"))
            .bearer_auth(&token)
            .header("X-Gestor-Api", API_VERSAO);
        if let Some(etag) = &etag {
            pedido = pedido.header(header::IF_NONE_MATCH, etag);
        }

        let resposta = pedido.send().await.map_err(|_| sem_conexao())?;
        let status = resposta.status();

        if status == StatusCode::NOT_MODIFIED {
            let cache = self.cache.lock().unwrap();
            return match cache.rotas.get(rota) {
                Some((_, corpo)) if cache.token == Some(dono) => Ok(corpo.clone()),
                _ => Err(ErroApi::novo("server_error", "Resposta 304 sem cópia guardada.")),
            };
        }

        if let Some(erro) = redirecionou(&resposta) {
            return Err(erro);
        }
        let novo_etag = resposta
            .headers()
            .get(header::ETAG)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        let corpo: Option<Value> = resposta.json().await.ok();

        if status.is_success() {
            let corpo = corpo.ok_or_else(|| {
                ErroApi::novo("server_error", "Resposta do Ápice Hub ilegível.")
            })?;
            if let Some(etag) = novo_etag {
                let mut cache = self.cache.lock().unwrap();
                if cache.token == Some(dono) {
                    cache.rotas.insert(rota, (etag, corpo.clone()));
                }
            }
            return Ok(corpo);
        }

        let erro = erro_do_corpo(status, corpo.as_ref());
        if erro.code == "token_revoked" || erro.code == "unauthorized" {
            *self.cache.lock().unwrap() = Cache::default();
        }
        if erro.code == "token_revoked" {
            // §1.2: revogado → apaga o token e volta ao enrolamento.
            let _ = keychain().and_then(|e| e.delete_credential().map_err(|_| erro_keychain()));
        }
        Err(erro)
    }
}

/// Quem o Hub reconheceu no enrolamento.
pub struct Enrolado {
    pub nome: String,
    pub departamento: String,
}

/// `POST enroll` (contrato §2): troca o código pelo token e grava o token no keychain.
/// Não repete nada sozinho; quem decide repetir é o chamador (`enrolar.rs`).
pub async fn enrolar(codigo: &str, device: Value) -> Result<Enrolado, ErroApi> {
    let base = base_url().ok_or_else(sem_endereco)?;
    let resposta = Api::new()
        .cliente
        .post(format!("{base}/enroll"))
        .header("X-Gestor-Api", API_VERSAO)
        .json(&serde_json::json!({ "code": codigo, "device": device }))
        .send()
        .await
        .map_err(|_| sem_conexao())?;
    if let Some(erro) = redirecionou(&resposta) {
        return Err(erro);
    }
    let status = resposta.status();
    let corpo: Option<Value> = resposta.json().await.ok();
    if !status.is_success() {
        return Err(erro_do_corpo(status, corpo.as_ref()));
    }
    let corpo = corpo.unwrap_or_default();
    let token = corpo
        .get("token")
        .and_then(Value::as_str)
        .ok_or_else(|| ErroApi::novo("server_error", "O Ápice Hub não devolveu o token."))?;
    keychain()?.set_password(token).map_err(|_| erro_keychain())?;
    let pessoa = |campo: &str| {
        corpo["person"].get(campo).and_then(Value::as_str).unwrap_or("").to_owned()
    };
    Ok(Enrolado { nome: pessoa("display_name"), departamento: pessoa("department") })
}

/// 3xx que não é o 304 do ETag: o endereço compilado está errado (ex.: sem www). Erro explícito
/// com o destino, para quem instala corrigir o GESTOR_API_BASE.
fn redirecionou(resposta: &reqwest::Response) -> Option<ErroApi> {
    let status = resposta.status();
    if !status.is_redirection() || status == StatusCode::NOT_MODIFIED {
        return None;
    }
    let destino = resposta
        .headers()
        .get(header::LOCATION)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("outro endereço");
    Some(ErroApi::novo(
        "server_error",
        &format!("O endereço do Ápice Hub redireciona para {destino}; confira o endereço (www)."),
    ))
}

fn sem_endereco() -> ErroApi {
    ErroApi::novo("server_error", "Endereço do Ápice Hub não configurado nesta versão.")
}

fn sem_conexao() -> ErroApi {
    ErroApi::novo("unavailable", "Sem conexão com o Ápice Hub.")
}

/// Agora, ISO 8601 com offset (contrato §1: instantes sempre com offset).
pub fn agora() -> String {
    chrono::Local::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, false)
}

/// Corpo de erro do contrato; se não vier, o `code` sai do status HTTP.
fn erro_do_corpo(status: StatusCode, corpo: Option<&Value>) -> ErroApi {
    let erro = corpo.and_then(|c| c.get("error"));
    let texto = |campo: &str| erro.and_then(|e| e.get(campo)).and_then(Value::as_str);
    let code = texto("code").unwrap_or(match status.as_u16() {
        401 => "unauthorized",
        426 => "upgrade_required",
        429 => "rate_limited",
        503 => "unavailable",
        _ => "server_error",
    });
    ErroApi {
        code: code.into(),
        message: texto("message").unwrap_or("Falha ao falar com o Ápice Hub.").into(),
        retry_after_seconds: erro
            .and_then(|e| e.get("retry_after_seconds"))
            .and_then(Value::as_u64),
    }
}

fn keychain() -> Result<keyring::Entry, ErroApi> {
    keyring::Entry::new(KEYCHAIN_SERVICO, KEYCHAIN_CONTA).map_err(|_| erro_keychain())
}

fn erro_keychain() -> ErroApi {
    ErroApi::novo("server_error", "Não foi possível usar o cofre de credenciais do Windows.")
}

fn ler_token() -> Result<String, ErroApi> {
    match keychain()?.get_password() {
        Ok(token) => Ok(token),
        // Código local, fora do contrato: sem token aqui não é o 401 do Hub (combinado com a Prisma).
        Err(keyring::Error::NoEntry) => Err(ErroApi::novo(
            "not_enrolled",
            "Este computador ainda não foi conectado ao Ápice Hub.",
        )),
        Err(_) => Err(erro_keychain()),
    }
}

/// Identifica o token no cache sem guardar uma segunda cópia dele.
fn impressao(token: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    token.hash(&mut h);
    h.finish()
}
