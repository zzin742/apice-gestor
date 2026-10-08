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
    code: String,
    message: String,
    retry_after_seconds: Option<u64>,
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
            .build()
            .expect("cliente HTTP");
        Self { cliente, cache: Mutex::default() }
    }

    /// GET autenticado com `If-None-Match`. Sempre devolve o corpo completo: o `304` vira o
    /// corpo guardado, e a interface nunca vê um `304`.
    pub async fn get(&self, rota: &'static str) -> Result<Value, ErroApi> {
        let base = base_url().ok_or_else(|| {
            ErroApi::novo("server_error", "Endereço do Ápice Hub não configurado nesta versão.")
        })?;
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

        let resposta = pedido.send().await.map_err(|_| {
            ErroApi::novo("unavailable", "Sem conexão com o Ápice Hub.")
        })?;
        let status = resposta.status();

        if status == StatusCode::NOT_MODIFIED {
            let cache = self.cache.lock().unwrap();
            return match cache.rotas.get(rota) {
                Some((_, corpo)) if cache.token == Some(dono) => Ok(corpo.clone()),
                _ => Err(ErroApi::novo("server_error", "Resposta 304 sem cópia guardada.")),
            };
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
    ErroApi::novo("server_error", "Não foi possível ler o cofre de credenciais do Windows.")
}

fn ler_token() -> Result<String, ErroApi> {
    match keychain()?.get_password() {
        Ok(token) => Ok(token),
        Err(keyring::Error::NoEntry) => Err(ErroApi::novo(
            "unauthorized",
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
