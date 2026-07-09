use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Key, Nonce,
};
use base64::{engine::general_purpose, Engine as _};
use keyring::Entry;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

const KEYCHAIN_SERVICE: &str = "com.valorantstore.app";

pub struct StoreManager {
    app_data_dir: PathBuf,
}

#[derive(Serialize, Deserialize, Default, Clone)]
pub struct ConfigData {
    pub puuid: Option<String>,
    pub token_expiry: Option<u64>,
}

impl StoreManager {
    pub fn new(app_data_dir: PathBuf) -> Self {
        fs::create_dir_all(&app_data_dir).ok();
        Self { app_data_dir }
    }

    fn config_path(&self) -> PathBuf {
        self.app_data_dir.join("config.json")
    }

    fn cookie_enc_path(&self) -> PathBuf {
        self.app_data_dir.join("cookies.enc")
    }

    pub fn save_config(&self, config: &ConfigData) {
        if let Ok(json) = serde_json::to_string_pretty(config) {
            let _ = fs::write(self.config_path(), json);
        }
    }

    pub fn load_config(&self) -> ConfigData {
        if let Ok(data) = fs::read_to_string(self.config_path()) {
            serde_json::from_str(&data).unwrap_or_default()
        } else {
            ConfigData::default()
        }
    }

    pub fn save_token(&self, key: &str, token: &str) -> Result<(), String> {
        let entry = Entry::new(KEYCHAIN_SERVICE, key).map_err(|e| e.to_string())?;
        entry.set_password(token).map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn load_token(&self, key: &str) -> Option<String> {
        Entry::new(KEYCHAIN_SERVICE, key).ok()?.get_password().ok()
    }

    pub fn delete_token(&self, key: &str) {
        if let Ok(entry) = Entry::new(KEYCHAIN_SERVICE, key) {
            let _ = entry.delete_credential();
        }
    }

    pub fn save_cookie_store(&self, cookie_json: &str) -> Result<(), String> {
        let b64 = general_purpose::STANDARD.encode(cookie_json);
        
        // Windows Credential Manager limit is ~2560 bytes. We use 2000 as a safe threshold.
        if b64.len() > 2000 {
            // Fallback to AES-GCM encrypted file
            let aes_key_str = match self.load_token("cookie_aes_key") {
                Some(k) => k,
                None => {
                    let mut key_bytes = [0u8; 32];
                    rand::thread_rng().fill_bytes(&mut key_bytes);
                    let k_str = general_purpose::STANDARD.encode(&key_bytes);
                    self.save_token("cookie_aes_key", &k_str)?;
                    k_str
                }
            };
            
            let key_bytes = general_purpose::STANDARD.decode(aes_key_str).map_err(|e| e.to_string())?;
            let key = Key::<Aes256Gcm>::clone_from_slice(&key_bytes);
            let cipher = Aes256Gcm::new(&key);
            
            let mut nonce_bytes = [0u8; 12];
            rand::thread_rng().fill_bytes(&mut nonce_bytes);
            let nonce = Nonce::clone_from_slice(&nonce_bytes); // 96-bits
            let ciphertext = cipher.encrypt(&nonce, cookie_json.as_bytes()).map_err(|e| e.to_string())?;
            
            let mut encrypted_data = nonce.to_vec();
            encrypted_data.extend_from_slice(&ciphertext);
            
            fs::write(self.cookie_enc_path(), encrypted_data).map_err(|e| e.to_string())?;
            
            // Delete plain cookie from keychain if it existed
            self.delete_token("cookie_store");
            
        } else {
            // Safe to store in Keychain directly
            self.save_token("cookie_store", &b64)?;
            // Clean up old encrypted fallback if existed
            let _ = fs::remove_file(self.cookie_enc_path());
        }
        
        Ok(())
    }

    pub fn load_cookie_store(&self) -> Option<String> {
        // Try direct keychain first
        if let Some(b64) = self.load_token("cookie_store") {
            if let Ok(json_bytes) = general_purpose::STANDARD.decode(b64) {
                if let Ok(json) = String::from_utf8(json_bytes) {
                    return Some(json);
                }
            }
        }
        
        // Try AES-GCM fallback
        if let Some(aes_key_str) = self.load_token("cookie_aes_key") {
            if let Ok(key_bytes) = general_purpose::STANDARD.decode(aes_key_str) {
                if let Ok(encrypted_data) = fs::read(self.cookie_enc_path()) {
                    if encrypted_data.len() > 12 {
                        let key = Key::<Aes256Gcm>::clone_from_slice(&key_bytes);
                        let cipher = Aes256Gcm::new(&key);
                        let nonce = Nonce::clone_from_slice(&encrypted_data[0..12]);
                        let ciphertext = &encrypted_data[12..];
                        
                        if let Ok(plaintext) = cipher.decrypt(&nonce, ciphertext) {
                            if let Ok(json) = String::from_utf8(plaintext) {
                                return Some(json);
                            }
                        }
                    }
                }
            }
        }
        
        None
    }
    
    pub fn clear_all(&self) {
        self.delete_token("access_token");
        self.delete_token("entitlements_token");
        self.delete_token("cookie_store");
        self.delete_token("cookie_aes_key");
        self.save_config(&ConfigData::default());
        let _ = fs::remove_file(self.cookie_enc_path());
    }
}
