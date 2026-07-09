use crate::auth::{check_and_refresh_token, AppState};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::{Mutex, OnceLock};
use tauri::{AppHandle, State};

const CLIENT_PLATFORM: &str = "ew0KCSJwbGF0Zm9ybVR5cGUiOiAiUEMiLA0KCSJwbGF0Zm9ybU9TIjogIldpbmRvd3MiLA0KCSJwbGF0Zm9ybU9TVmVyc2lvbiI6ICIxMC4wLjE5MDQyLjEuMjU2LjY0Yml0IiwNCgkicGxhdGZvcm1DaGlwc2V0IjogIkludGVsIg0KfQ==";

fn get_client_version_cache() -> &'static Mutex<Option<String>> {
    static CACHE: OnceLock<Mutex<Option<String>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(None))
}

async fn get_client_version(client: &reqwest::Client) -> Result<String, String> {
    if let Some(v) = get_client_version_cache().lock().unwrap().clone() {
        return Ok(v);
    }

    let resp = client
        .get("https://valorant-api.com/v1/version")
        .send()
        .await
        .map_err(|e| format!("Failed to fetch version: {}", e))?;

    let json: Value = resp
        .json()
        .await
        .map_err(|e| format!("Failed to parse version JSON: {}", e))?;
        
    let version = json
        .get("data")
        .and_then(|d| d.get("riotClientVersion"))
        .and_then(|v| v.as_str())
        .ok_or("Missing riotClientVersion in valorant-api.com response")?
        .to_string();

    *get_client_version_cache().lock().unwrap() = Some(version.clone());
    Ok(version)
}

#[tauri::command]
pub async fn get_storefront(
    region: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Vec<String>, String> {
    check_and_refresh_token(&state, &app).await?;

    let access_token = state
        .access_token
        .lock()
        .unwrap()
        .clone()
        .ok_or("Not logged in (Missing access token)")?;
    let ent_token = state
        .entitlements_token
        .lock()
        .unwrap()
        .clone()
        .ok_or("Not logged in (Missing entitlements token)")?;
    let puuid = state
        .config
        .lock()
        .unwrap()
        .puuid
        .clone()
        .ok_or("Missing PUUID")?;

    let client = state.client.lock().unwrap().clone();
    let version = get_client_version(&client).await?;

    let url = format!(
        "https://pd.{}.a.pvp.net/store/v3/storefront/{}",
        region, puuid
    );
    let resp = client
        .get(&url)
        .header("Authorization", format!("Bearer {}", access_token))
        .header("X-Riot-Entitlements-JWT", ent_token)
        .header("X-Riot-ClientPlatform", CLIENT_PLATFORM)
        .header("X-Riot-ClientVersion", version)
        .send()
        .await
        .map_err(|e| format!("Failed to fetch storefront: {}", e))?;

    let json: Value = resp
        .json()
        .await
        .map_err(|e| format!("Failed to parse storefront JSON: {}", e))?;

    let offers = json
        .get("SkinsPanelLayout")
        .and_then(|s| s.get("SingleItemOffers"))
        .and_then(|o| o.as_array())
        .ok_or("Missing SingleItemOffers in JSON")?;

    let uuids = offers
        .iter()
        .filter_map(|v| v.as_str().map(|s| s.to_string()))
        .collect();

    Ok(uuids)
}

#[tauri::command]
pub async fn get_match_history(
    region: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Value, String> {
    check_and_refresh_token(&state, &app).await?;

    let access_token = state
        .access_token
        .lock()
        .unwrap()
        .clone()
        .ok_or("Not logged in (Missing access token)")?;
    let ent_token = state
        .entitlements_token
        .lock()
        .unwrap()
        .clone()
        .ok_or("Not logged in (Missing entitlements token)")?;
    let puuid = state
        .config
        .lock()
        .unwrap()
        .puuid
        .clone()
        .ok_or("Missing PUUID")?;

    let client = state.client.lock().unwrap().clone();
    let version = get_client_version(&client).await?;

    let url = format!(
        "https://pd.{}.a.pvp.net/match-history/v1/history/{}?startIndex=0&endIndex=10",
        region, puuid
    );
    let resp = client
        .get(&url)
        .header("Authorization", format!("Bearer {}", access_token))
        .header("X-Riot-Entitlements-JWT", ent_token)
        .header("X-Riot-ClientPlatform", CLIENT_PLATFORM)
        .header("X-Riot-ClientVersion", version)
        .send()
        .await
        .map_err(|e| format!("Failed to fetch match history: {}", e))?;

    let json: Value = resp
        .json()
        .await
        .map_err(|e| format!("Failed to parse match history JSON: {}", e))?;

    Ok(json)
}

// Helper endpoint to get Match Details (needed for K/D/A and Map info)
#[tauri::command]
pub async fn get_match_details(
    region: String,
    match_id: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Value, String> {
    check_and_refresh_token(&state, &app).await?;

    let access_token = state
        .access_token
        .lock()
        .unwrap()
        .clone()
        .ok_or("Not logged in (Missing access token)")?;
    let ent_token = state
        .entitlements_token
        .lock()
        .unwrap()
        .clone()
        .ok_or("Not logged in (Missing entitlements token)")?;

    let client = state.client.lock().unwrap().clone();
    let version = get_client_version(&client).await?;

    let url = format!(
        "https://pd.{}.a.pvp.net/match-details/v1/matches/{}",
        region, match_id
    );
    let resp = client
        .get(&url)
        .header("Authorization", format!("Bearer {}", access_token))
        .header("X-Riot-Entitlements-JWT", ent_token)
        .header("X-Riot-ClientPlatform", CLIENT_PLATFORM)
        .header("X-Riot-ClientVersion", version)
        .send()
        .await
        .map_err(|e| format!("Failed to fetch match details: {}", e))?;

    let json: Value = resp
        .json()
        .await
        .map_err(|e| format!("Failed to parse match details JSON: {}", e))?;

    Ok(json)
}

#[derive(Serialize)]
pub struct SkinDetails {
    pub uuid: String,
    pub display_name: Option<String>,
    pub display_icon: Option<String>,
}

#[tauri::command]
pub async fn get_skin_details(
    uuids: Vec<String>,
    language: Option<String>,
    state: State<'_, AppState>,
) -> Result<Vec<SkinDetails>, String> {
    let client = state.client.lock().unwrap().clone();
    let lang = language.unwrap_or_else(|| "zh-TW".to_string());
    
    let mut handles = Vec::new();
    
    for uuid in uuids {
        let client_clone = client.clone();
        let lang_clone = lang.clone();
        let uuid_clone = uuid.clone();
        
        let handle = tokio::spawn(async move {
            let url = format!("https://valorant-api.com/v1/weapons/skinlevels/{}?language={}", uuid_clone, lang_clone);
            let resp = client_clone.get(&url).send().await;
            
            let mut details = SkinDetails {
                uuid: uuid_clone,
                display_name: None,
                display_icon: None,
            };
            
            if let Ok(r) = resp {
                if let Ok(json) = r.json::<Value>().await {
                    if let Some(data) = json.get("data") {
                        if let Some(name) = data.get("displayName").and_then(|v| v.as_str()) {
                            details.display_name = Some(name.to_string());
                        }
                        if let Some(icon) = data.get("displayIcon").and_then(|v| v.as_str()) {
                            details.display_icon = Some(icon.to_string());
                        }
                    }
                }
            }
            
            details
        });
        handles.push(handle);
    }
    
    let mut results = Vec::new();
    for handle in handles {
        if let Ok(res) = handle.await {
            results.push(res);
        }
    }
    
    Ok(results)
}
