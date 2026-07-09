use crate::store::{ConfigData, StoreManager};
use reqwest_cookie_store::CookieStoreMutex;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, State, WebviewUrl, WebviewWindowBuilder};

#[derive(Serialize, Clone)]
pub enum LoginResult {
    Success,
    Require2FA,
    Error(String),
}

pub struct AppState {
    pub client: Mutex<reqwest::Client>,
    pub cookie_store: Arc<CookieStoreMutex>,
    pub store: StoreManager,
    pub access_token: Mutex<Option<String>>,
    pub entitlements_token: Mutex<Option<String>>,
    pub config: Mutex<ConfigData>,
}

impl AppState {
    pub fn new(store: StoreManager) -> Self {
        let cookie_store = if let Some(json) = store.load_cookie_store() {
            let mut cursor = std::io::Cursor::new(json);
            cookie_store::CookieStore::load_json(&mut cursor).unwrap_or_default()
        } else {
            cookie_store::CookieStore::default()
        };
        
        let cookie_store = Arc::new(CookieStoreMutex::new(cookie_store));
        
        let client = reqwest::Client::builder()
            .cookie_provider(Arc::clone(&cookie_store))
            .build()
            .expect("Failed to build reqwest client");
            
        let config = store.load_config();
        let access_token = store.load_token("access_token");
        let entitlements_token = store.load_token("entitlements_token");
            
        Self {
            client: Mutex::new(client),
            cookie_store,
            store,
            access_token: Mutex::new(access_token),
            entitlements_token: Mutex::new(entitlements_token),
            config: Mutex::new(config),
        }
    }
    
    pub fn save_cookies(&self) {
        let store_guard = self.cookie_store.lock().unwrap();
        let mut buffer = Vec::new();
        if store_guard.save_json(&mut buffer).is_ok() {
            if let Ok(json) = String::from_utf8(buffer) {
                let _ = self.store.save_cookie_store(&json);
            }
        }
    }
}

async fn process_webview_url(state: &State<'_, AppState>, url: &str) -> Result<LoginResult, String> {
    let fragment = url.split('#').nth(1).ok_or("No fragment in URI")?;
    let mut access_token = None;
    let mut id_token = None;
    let mut expires_in = None;
    
    for pair in fragment.split('&') {
        let mut parts = pair.splitn(2, '=');
        if let (Some(key), Some(value)) = (parts.next(), parts.next()) {
            match key {
                "access_token" => access_token = Some(value.to_string()),
                "id_token" => id_token = Some(value.to_string()),
                "expires_in" => expires_in = Some(value.to_string()),
                _ => {}
            }
        }
    }
    
    let access_token = access_token.ok_or("Missing access_token in URI fragment")?;
    let exp_seconds: u64 = expires_in.unwrap_or("3600".to_string()).parse().unwrap_or(3600);
    
    let client = state.client.lock().unwrap().clone();
    
    let ent_resp = client.post("https://entitlements.auth.riotgames.com/api/token/v1")
        .header("Authorization", format!("Bearer {}", access_token))
        .json(&serde_json::json!({}))
        .send()
        .await
        .map_err(|e| format!("Failed to fetch entitlements token: {}", e))?;
        
    let ent_json: serde_json::Value = ent_resp.json().await.map_err(|e| format!("Failed to parse entitlements token JSON: {}", e))?;
    let ent_token = ent_json.get("entitlements_token")
        .and_then(|v| v.as_str())
        .ok_or("Missing entitlements_token in response")?
        .to_string();
        
    let userinfo_resp = client.get("https://auth.riotgames.com/userinfo")
        .header("Authorization", format!("Bearer {}", access_token))
        .send()
        .await
        .map_err(|e| format!("Failed to fetch userinfo: {}", e))?;
        
    let userinfo_json: serde_json::Value = userinfo_resp.json().await.map_err(|e| format!("Failed to parse userinfo JSON: {}", e))?;
    let puuid = userinfo_json.get("sub")
        .and_then(|v| v.as_str())
        .ok_or("Missing sub (puuid) in userinfo")?
        .to_string();
        
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
    let expiry = now + exp_seconds;
    
    let _ = state.store.save_token("access_token", &access_token);
    let _ = state.store.save_token("entitlements_token", &ent_token);
    
    let mut config = state.config.lock().unwrap();
    config.puuid = Some(puuid.clone());
    config.token_expiry = Some(expiry);
    state.store.save_config(&config);
    
    state.save_cookies();
    
    *state.access_token.lock().unwrap() = Some(access_token);
    *state.entitlements_token.lock().unwrap() = Some(ent_token);
    
    Ok(LoginResult::Success)
}

#[tauri::command]
pub async fn check_login(state: State<'_, AppState>, app: AppHandle) -> Result<bool, String> {
    let has_token = state.access_token.lock().unwrap().is_some();
    if has_token {
        if check_and_refresh_token(&state, &app).await.is_ok() {
            return Ok(true);
        }
    }
    Ok(false)
}

#[tauri::command]
pub async fn start_webview_login(app: AppHandle, state: State<'_, AppState>) -> Result<LoginResult, String> {
    let login_url = "https://auth.riotgames.com/authorize?client_id=play-valorant-web-prod&nonce=1&redirect_uri=https%3A%2F%2Fplayvalorant.com%2Fopt_in&response_type=token%20id_token&scope=account%20openid";
    
    let window = WebviewWindowBuilder::new(
        &app,
        "login_webview",
        WebviewUrl::External(login_url.parse().unwrap())
    )
    .title("Riot Games Login")
    .inner_size(400.0, 700.0)
    .resizable(false)
    .build()
    .map_err(|e| format!("Failed to open login window: {}", e))?;
    
    let mut success_url = None;
    
    // Poll the URL for up to 5 minutes
    for _ in 0..600 {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        
        if let Ok(url) = window.url() {
            let current_url = url.to_string();
            if current_url.starts_with("https://playvalorant.com/opt_in") && current_url.contains("access_token=") {
                success_url = Some(current_url);
                break;
            }
        }
        
        // Break if user closed the window
        if let Ok(visible) = window.is_visible() {
            if !visible {
                break;
            }
        } else {
            break; // window probably destroyed
        }
    }
    
    if let Some(url) = success_url {
        let _ = window.close();
        return process_webview_url(&state, &url).await;
    }
    
    let _ = window.close();
    Err("Login cancelled or timed out".to_string())
}

#[tauri::command]
pub async fn logout(state: State<'_, AppState>) -> Result<(), String> {
    state.store.clear_all();
    
    *state.access_token.lock().unwrap() = None;
    *state.entitlements_token.lock().unwrap() = None;
    *state.config.lock().unwrap() = ConfigData::default();
    
    let cookie_store = cookie_store::CookieStore::default();
    let new_cookie_store = Arc::new(CookieStoreMutex::new(cookie_store));
    *state.client.lock().unwrap() = reqwest::Client::builder()
        .cookie_provider(Arc::clone(&new_cookie_store))
        .build()
        .map_err(|e| format!("Failed to reset client: {}", e))?;
    
    Ok(())
}

pub async fn check_and_refresh_token(state: &State<'_, AppState>, app: &AppHandle) -> Result<(), String> {
    let config = state.config.lock().unwrap().clone();
    let expiry = config.token_expiry.unwrap_or(0);
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
    
    if expiry > 0 && now + 300 < expiry {
        return Ok(()); // Token is still valid
    }
    
    let client = state.client.lock().unwrap().clone();
    
    let init_req = serde_json::json!({
        "client_id": "play-valorant-web-prod",
        "nonce": "1",
        "redirect_uri": "https://playvalorant.com/opt_in",
        "response_type": "token id_token",
        "scope": "account openid"
    });
    
    let init_resp = client.post("https://auth.riotgames.com/api/v1/authorization")
        .json(&init_req)
        .send()
        .await
        .map_err(|e| format!("Silent refresh init failed: {}", e))?;
        
    let resp_json: serde_json::Value = init_resp.json().await.map_err(|e| format!("Failed to parse silent refresh response: {}", e))?;
    
    let auth_type = resp_json.get("type").and_then(|v| v.as_str()).unwrap_or("");
    if auth_type == "response" {
        let uri = resp_json.get("response")
            .and_then(|r| r.get("parameters"))
            .and_then(|p| p.get("uri"))
            .and_then(|u| u.as_str())
            .ok_or("Missing URI in auth response")?;
            
        if let Ok(LoginResult::Success) = process_webview_url(state, uri).await {
            return Ok(());
        }
    }
    
    let _ = app.emit("token_expired", ());
    Err("Token expired and silent refresh failed".to_string())
}
