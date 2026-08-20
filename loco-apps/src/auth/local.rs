use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::RwLock;

use serde::{Deserialize, Serialize};

use super::{
    AccountType, ApiKey, ApiKeyInfo, AuthAdapter, AuthError, AuthSession, AuthUser,
    CreateUserRequest, LoginCredentials, UpdateUserRequest, PUBLIC_USERNAME, TEST_PASSWORD,
};

#[derive(Serialize, Deserialize, Clone)]
struct StoredAccount {
    handle: String,
    #[serde(rename = "type")]
    account_type: AccountType,
    created_at: String,
}

/// 1:1 with a person account. Org accounts have no identity and cannot log in.
#[derive(Serialize, Deserialize, Clone)]
struct StoredIdentity {
    id: String,
    handle: String,
    name: String,
    /// Plaintext on the local adapter (same as API keys).
    password: String,
    created_at: String,
    last_login_at: Option<String>,
}

#[derive(Serialize, Deserialize, Clone)]
struct StoredSession {
    token: String,
    identity_id: String,
    created_at: String,
}

#[derive(Serialize, Deserialize, Clone)]
struct StoredApiKey {
    id: String,
    key_hash: String,
    identity_id: String,
    label: String,
    created_at: String,
    last_used_at: Option<String>,
    revoked: bool,
}

/// Filesystem layout (global, not site-scoped):
///
/// ```text
/// {base}/accounts/{handle}.json
/// {base}/identities/{handle}.json
/// {base}/sessions/{token}.json
/// {base}/api_keys/{id}.json
/// ```
pub struct LocalAuthAdapter {
    base_dir: PathBuf,
    accounts: RwLock<HashMap<String, StoredAccount>>,
    identities: RwLock<HashMap<String, StoredIdentity>>, // handle → identity
    sessions: RwLock<HashMap<String, StoredSession>>,    // token → session
    api_keys: RwLock<HashMap<String, StoredApiKey>>,     // id → key
}

impl LocalAuthAdapter {
    pub fn new(base_dir: &Path) -> Self {
        let adapter = LocalAuthAdapter {
            base_dir: base_dir.to_path_buf(),
            accounts: RwLock::new(HashMap::new()),
            identities: RwLock::new(HashMap::new()),
            sessions: RwLock::new(HashMap::new()),
            api_keys: RwLock::new(HashMap::new()),
        };
        adapter.load_from_disk();
        adapter.seed_defaults();
        adapter
    }

    fn load_from_disk(&self) {
        self.load_json_dir("accounts", |this, account: StoredAccount| {
            this.accounts
                .write()
                .unwrap()
                .insert(account.handle.clone(), account);
        });
        self.load_json_dir("identities", |this, identity: StoredIdentity| {
            this.identities
                .write()
                .unwrap()
                .insert(identity.handle.clone(), identity);
        });
        self.load_json_dir("sessions", |this, session: StoredSession| {
            this.sessions
                .write()
                .unwrap()
                .insert(session.token.clone(), session);
        });
        self.load_json_dir("api_keys", |this, key: StoredApiKey| {
            this.api_keys.write().unwrap().insert(key.id.clone(), key);
        });
    }

    fn load_json_dir<T: for<'de> Deserialize<'de>>(&self, dirname: &str, insert: impl Fn(&Self, T)) {
        let dir = self.base_dir.join(dirname);
        let Ok(entries) = std::fs::read_dir(&dir) else {
            return;
        };
        for entry in entries.flatten() {
            if entry.path().extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let Ok(contents) = std::fs::read_to_string(entry.path()) else {
                continue;
            };
            if let Ok(value) = serde_json::from_str::<T>(&contents) {
                insert(self, value);
            }
        }
    }

    fn seed_defaults(&self) {
        self.ensure_person("alice", "Alice");
        self.ensure_person("bob", "Bob");
        self.ensure_org("loco");
    }

    fn ensure_person(&self, handle: &str, name: &str) {
        if self.accounts.read().unwrap().contains_key(handle) {
            return;
        }
        let now = chrono::Utc::now().to_rfc3339();
        let account = StoredAccount {
            handle: handle.to_string(),
            account_type: AccountType::Person,
            created_at: now.clone(),
        };
        let identity = StoredIdentity {
            id: uuid::Uuid::new_v4().to_string(),
            handle: handle.to_string(),
            name: name.to_string(),
            password: TEST_PASSWORD.to_string(),
            created_at: now,
            last_login_at: None,
        };
        self.persist_account(&account);
        self.persist_identity(&identity);
        self.accounts
            .write()
            .unwrap()
            .insert(handle.to_string(), account);
        self.identities
            .write()
            .unwrap()
            .insert(handle.to_string(), identity);
    }

    fn ensure_org(&self, handle: &str) {
        if self.accounts.read().unwrap().contains_key(handle) {
            return;
        }
        let account = StoredAccount {
            handle: handle.to_string(),
            account_type: AccountType::Org,
            created_at: chrono::Utc::now().to_rfc3339(),
        };
        self.persist_account(&account);
        self.accounts
            .write()
            .unwrap()
            .insert(handle.to_string(), account);
    }

    fn persist_account(&self, account: &StoredAccount) {
        let dir = self.base_dir.join("accounts");
        std::fs::create_dir_all(&dir).ok();
        let path = dir.join(format!("{}.json", account.handle));
        std::fs::write(path, serde_json::to_string_pretty(account).unwrap()).ok();
    }

    fn persist_identity(&self, identity: &StoredIdentity) {
        let dir = self.base_dir.join("identities");
        std::fs::create_dir_all(&dir).ok();
        let path = dir.join(format!("{}.json", identity.handle));
        std::fs::write(path, serde_json::to_string_pretty(identity).unwrap()).ok();
    }

    fn delete_identity_files(&self, handle: &str) {
        let _ = std::fs::remove_file(self.base_dir.join("accounts").join(format!("{handle}.json")));
        let _ = std::fs::remove_file(self.base_dir.join("identities").join(format!("{handle}.json")));
    }

    fn persist_session(&self, session: &StoredSession) {
        let dir = self.base_dir.join("sessions");
        std::fs::create_dir_all(&dir).ok();
        let path = dir.join(format!("{}.json", session.token));
        std::fs::write(path, serde_json::to_string_pretty(session).unwrap()).ok();
    }

    fn delete_session_file(&self, token: &str) {
        let path = self.base_dir.join("sessions").join(format!("{token}.json"));
        let _ = std::fs::remove_file(path);
    }

    fn persist_api_key(&self, key: &StoredApiKey) {
        let dir = self.base_dir.join("api_keys");
        std::fs::create_dir_all(&dir).ok();
        let path = dir.join(format!("{}.json", key.id));
        std::fs::write(path, serde_json::to_string_pretty(key).unwrap()).ok();
    }

    fn to_auth_user(identity: &StoredIdentity) -> AuthUser {
        AuthUser {
            id: identity.id.clone(),
            username: identity.handle.clone(),
            name: identity.name.clone(),
            account_type: AccountType::Person.as_str().to_string(),
            created_at: identity.created_at.clone(),
            last_login_at: identity.last_login_at.clone(),
        }
    }

    fn identity_by_id(&self, id: &str) -> Option<StoredIdentity> {
        self.identities
            .read()
            .unwrap()
            .values()
            .find(|i| i.id == id)
            .cloned()
    }

    fn password_ok(stored: &str, provided: Option<&str>) -> bool {
        match provided {
            // Test-only bypass — login body may omit password. Removed in PR 2.
            None => true,
            Some(password) => password == stored,
        }
    }

    /// First login of an unknown handle creates a person account + identity.
    /// Lets Hurl suites keep working without per-suite auth fixtures.
    fn auto_create_person(
        &self,
        handle: &str,
        password: Option<&str>,
    ) -> Result<StoredIdentity, AuthError> {
        if handle.is_empty() || handle == PUBLIC_USERNAME {
            return Err(AuthError::InvalidCredentials);
        }
        {
            let accounts = self.accounts.read().unwrap();
            if let Some(existing) = accounts.get(handle) {
                return match existing.account_type {
                    AccountType::Org => Err(AuthError::InvalidCredentials),
                    AccountType::Person => Err(AuthError::UserAlreadyExists),
                };
            }
        }
        let now = chrono::Utc::now().to_rfc3339();
        let account = StoredAccount {
            handle: handle.to_string(),
            account_type: AccountType::Person,
            created_at: now.clone(),
        };
        let identity = StoredIdentity {
            id: uuid::Uuid::new_v4().to_string(),
            handle: handle.to_string(),
            name: handle.to_string(),
            password: password
                .map(str::to_string)
                .unwrap_or_else(|| TEST_PASSWORD.to_string()),
            created_at: now,
            last_login_at: None,
        };
        self.persist_account(&account);
        self.persist_identity(&identity);
        self.accounts
            .write()
            .unwrap()
            .insert(handle.to_string(), account);
        self.identities
            .write()
            .unwrap()
            .insert(handle.to_string(), identity.clone());
        Ok(identity)
    }
}

impl AuthAdapter for LocalAuthAdapter {
    fn login(&self, credentials: &LoginCredentials) -> Result<AuthSession, AuthError> {
        if credentials.username.is_empty() || credentials.username == PUBLIC_USERNAME {
            return Err(AuthError::InvalidCredentials);
        }

        let now = chrono::Utc::now().to_rfc3339();

        let identity = {
            let account = self
                .accounts
                .read()
                .unwrap()
                .get(&credentials.username)
                .cloned();

            match account {
                Some(account) if account.account_type == AccountType::Org => {
                    return Err(AuthError::InvalidCredentials);
                }
                Some(_) => {
                    let mut identities = self.identities.write().unwrap();
                    let identity = identities
                        .get_mut(&credentials.username)
                        .ok_or(AuthError::UserNotFound)?;
                    if !Self::password_ok(&identity.password, credentials.password.as_deref()) {
                        return Err(AuthError::InvalidCredentials);
                    }
                    identity.last_login_at = Some(now.clone());
                    identity.clone()
                }
                None => {
                    let mut identity = self.auto_create_person(
                        &credentials.username,
                        credentials.password.as_deref(),
                    )?;
                    identity.last_login_at = Some(now.clone());
                    self.identities
                        .write()
                        .unwrap()
                        .insert(identity.handle.clone(), identity.clone());
                    identity
                }
            }
        };

        self.persist_identity(&identity);

        let token = uuid::Uuid::new_v4().to_string();
        let session = StoredSession {
            token: token.clone(),
            identity_id: identity.id.clone(),
            created_at: now,
        };
        self.persist_session(&session);
        self.sessions
            .write()
            .unwrap()
            .insert(token.clone(), session);

        Ok(AuthSession {
            token,
            user: Self::to_auth_user(&identity),
        })
    }

    fn validate_session(&self, token: &str) -> Result<AuthSession, AuthError> {
        let sessions = self.sessions.read().unwrap();
        let session = sessions.get(token).ok_or(AuthError::SessionNotFound)?;
        let identity = self
            .identity_by_id(&session.identity_id)
            .ok_or(AuthError::UserNotFound)?;
        Ok(AuthSession {
            token: token.to_string(),
            user: Self::to_auth_user(&identity),
        })
    }

    fn logout(&self, token: &str) -> Result<(), AuthError> {
        self.sessions
            .write()
            .unwrap()
            .remove(token)
            .ok_or(AuthError::SessionNotFound)?;
        self.delete_session_file(token);
        Ok(())
    }

    fn revoke_all_sessions(&self, identity_id: &str) -> Result<(), AuthError> {
        let tokens: Vec<String> = {
            let sessions = self.sessions.read().unwrap();
            sessions
                .iter()
                .filter(|(_, s)| s.identity_id == identity_id)
                .map(|(token, _)| token.clone())
                .collect()
        };
        {
            let mut sessions = self.sessions.write().unwrap();
            for token in &tokens {
                sessions.remove(token);
            }
        }
        for token in &tokens {
            self.delete_session_file(token);
        }
        Ok(())
    }

    fn get_user(&self, user_id: &str) -> Result<Option<AuthUser>, AuthError> {
        Ok(self.identity_by_id(user_id).map(|i| Self::to_auth_user(&i)))
    }

    fn list_users(&self) -> Result<Vec<AuthUser>, AuthError> {
        let identities = self.identities.read().unwrap();
        Ok(identities.values().map(Self::to_auth_user).collect())
    }

    fn create_user(&self, req: &CreateUserRequest) -> Result<AuthUser, AuthError> {
        if req.username.is_empty() || req.username == PUBLIC_USERNAME {
            return Err(AuthError::InvalidCredentials);
        }
        if self.accounts.read().unwrap().contains_key(&req.username) {
            return Err(AuthError::UserAlreadyExists);
        }

        let now = chrono::Utc::now().to_rfc3339();
        let account = StoredAccount {
            handle: req.username.clone(),
            account_type: AccountType::Person,
            created_at: now.clone(),
        };
        let identity = StoredIdentity {
            id: uuid::Uuid::new_v4().to_string(),
            handle: req.username.clone(),
            name: req.name.clone(),
            password: req
                .password
                .clone()
                .unwrap_or_else(|| TEST_PASSWORD.to_string()),
            created_at: now,
            last_login_at: None,
        };
        self.persist_account(&account);
        self.persist_identity(&identity);
        self.accounts
            .write()
            .unwrap()
            .insert(req.username.clone(), account);
        self.identities
            .write()
            .unwrap()
            .insert(req.username.clone(), identity.clone());
        Ok(Self::to_auth_user(&identity))
    }

    fn update_user(
        &self,
        user_id: &str,
        updates: &UpdateUserRequest,
    ) -> Result<AuthUser, AuthError> {
        let identity = {
            let mut identities = self.identities.write().unwrap();
            let identity = identities
                .values_mut()
                .find(|i| i.id == user_id)
                .ok_or(AuthError::UserNotFound)?;
            if let Some(name) = &updates.name {
                identity.name = name.clone();
            }
            identity.clone()
        };
        self.persist_identity(&identity);
        Ok(Self::to_auth_user(&identity))
    }

    fn delete_user(&self, user_id: &str) -> Result<(), AuthError> {
        let handle = {
            let mut identities = self.identities.write().unwrap();
            let handle = identities
                .values()
                .find(|i| i.id == user_id)
                .map(|i| i.handle.clone())
                .ok_or(AuthError::UserNotFound)?;
            identities.remove(&handle);
            handle
        };
        self.accounts.write().unwrap().remove(&handle);
        self.delete_identity_files(&handle);
        self.revoke_all_sessions(user_id)?;
        Ok(())
    }

    fn create_api_key(&self, identity_id: &str, label: &str) -> Result<ApiKey, AuthError> {
        if self.identity_by_id(identity_id).is_none() {
            return Err(AuthError::UserNotFound);
        }
        let now = chrono::Utc::now().to_rfc3339();
        let key = uuid::Uuid::new_v4().to_string();
        let id = uuid::Uuid::new_v4().to_string();
        let stored = StoredApiKey {
            id: id.clone(),
            key_hash: key.clone(),
            identity_id: identity_id.to_string(),
            label: label.to_string(),
            created_at: now.clone(),
            last_used_at: None,
            revoked: false,
        };
        self.api_keys
            .write()
            .unwrap()
            .insert(id.clone(), stored.clone());
        self.persist_api_key(&stored);
        Ok(ApiKey {
            id,
            key,
            label: label.to_string(),
            created_at: now,
        })
    }

    fn validate_api_key(&self, key: &str) -> Result<AuthSession, AuthError> {
        let api_keys = self.api_keys.read().unwrap();
        let stored = api_keys
            .values()
            .find(|k| k.key_hash == key && !k.revoked)
            .ok_or(AuthError::InvalidCredentials)?;
        let identity = self
            .identity_by_id(&stored.identity_id)
            .ok_or(AuthError::UserNotFound)?;
        Ok(AuthSession {
            token: key.to_string(),
            user: Self::to_auth_user(&identity),
        })
    }

    fn revoke_api_key(&self, identity_id: &str, key_id: &str) -> Result<(), AuthError> {
        let key = {
            let mut api_keys = self.api_keys.write().unwrap();
            let key = api_keys
                .get_mut(key_id)
                .ok_or(AuthError::Internal("api key not found".to_string()))?;
            if key.identity_id != identity_id {
                return Err(AuthError::Unauthorized);
            }
            key.revoked = true;
            key.clone()
        };
        self.persist_api_key(&key);
        Ok(())
    }

    fn list_api_keys(&self, identity_id: &str) -> Result<Vec<ApiKeyInfo>, AuthError> {
        let api_keys = self.api_keys.read().unwrap();
        Ok(api_keys
            .values()
            .filter(|k| k.identity_id == identity_id)
            .map(|k| ApiKeyInfo {
                id: k.id.clone(),
                label: k.label.clone(),
                created_at: k.created_at.clone(),
                last_used_at: k.last_used_at.clone(),
                revoked: k.revoked,
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::AuthAdapter;

    fn adapter() -> (tempfile::TempDir, LocalAuthAdapter) {
        let dir = tempfile::TempDir::new().unwrap();
        let adapter = LocalAuthAdapter::new(dir.path());
        (dir, adapter)
    }

    fn login(adapter: &LocalAuthAdapter, username: &str, password: Option<&str>) -> AuthSession {
        adapter
            .login(&LoginCredentials {
                username: username.to_string(),
                password: password.map(str::to_string),
            })
            .unwrap()
    }

    #[test]
    fn seed_creates_alice_bob_persons_and_loco_org() {
        let (_dir, adapter) = adapter();
        let accounts = adapter.accounts.read().unwrap();
        assert_eq!(accounts.get("alice").unwrap().account_type, AccountType::Person);
        assert_eq!(accounts.get("bob").unwrap().account_type, AccountType::Person);
        assert_eq!(accounts.get("loco").unwrap().account_type, AccountType::Org);
        assert!(adapter.identities.read().unwrap().contains_key("alice"));
        assert!(adapter.identities.read().unwrap().contains_key("bob"));
        assert!(!adapter.identities.read().unwrap().contains_key("loco"));
    }

    #[test]
    fn login_does_not_need_a_site() {
        let (_dir, adapter) = adapter();
        let session = login(&adapter, "alice", None);
        assert_eq!(session.user.username, "alice");
        assert_eq!(session.user.account_type, "person");
        assert!(!session.token.is_empty());
        let json = serde_json::to_value(&session.user).unwrap();
        assert!(json.get("site_id").is_none());
    }

    #[test]
    fn login_with_correct_password_succeeds() {
        let (_dir, adapter) = adapter();
        let session = login(&adapter, "alice", Some(TEST_PASSWORD));
        assert_eq!(session.user.username, "alice");
    }

    #[test]
    fn login_with_wrong_password_fails() {
        let (_dir, adapter) = adapter();
        let err = adapter
            .login(&LoginCredentials {
                username: "alice".to_string(),
                password: Some("nope".to_string()),
            })
            .unwrap_err();
        assert!(matches!(err, AuthError::InvalidCredentials));
    }

    #[test]
    fn login_as_org_fails() {
        let (_dir, adapter) = adapter();
        let err = adapter
            .login(&LoginCredentials {
                username: "loco".to_string(),
                password: None,
            })
            .unwrap_err();
        assert!(matches!(err, AuthError::InvalidCredentials));
    }

    #[test]
    fn login_unknown_handle_creates_person() {
        let (_dir, adapter) = adapter();
        let session = login(&adapter, "testuser", None);
        assert_eq!(session.user.username, "testuser");
        assert_eq!(session.user.account_type, "person");
        let accounts = adapter.accounts.read().unwrap();
        assert_eq!(
            accounts.get("testuser").unwrap().account_type,
            AccountType::Person
        );
    }

    #[test]
    fn session_hangs_off_identity_and_reloads() {
        let (dir, adapter) = adapter();
        let session = login(&adapter, "alice", None);
        let token = session.token.clone();
        let identity_id = session.user.id.clone();

        drop(adapter);
        let reloaded = LocalAuthAdapter::new(dir.path());
        let validated = reloaded.validate_session(&token).unwrap();
        assert_eq!(validated.user.id, identity_id);
        assert_eq!(validated.user.username, "alice");
        assert!(serde_json::to_value(&validated.user)
            .unwrap()
            .get("site_id")
            .is_none());
    }

    #[test]
    fn api_key_hangs_off_identity() {
        let (_dir, adapter) = adapter();
        let session = login(&adapter, "alice", None);
        let key = adapter
            .create_api_key(&session.user.id, "ci")
            .unwrap();
        let via_key = adapter.validate_api_key(&key.key).unwrap();
        assert_eq!(via_key.user.username, "alice");
        assert_eq!(via_key.user.id, session.user.id);

        adapter.revoke_api_key(&session.user.id, &key.id).unwrap();
        assert!(adapter.validate_api_key(&key.key).is_err());
    }

    #[test]
    fn public_session_is_not_site_scoped() {
        let session = AuthSession::public();
        assert_eq!(session.user.username, PUBLIC_USERNAME);
        let json = serde_json::to_value(&session.user).unwrap();
        assert!(json.get("site_id").is_none());
    }
}
