use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::RwLock;

use serde::{Deserialize, Serialize};

use super::{
    ApiKey, ApiKeyInfo, AuthAdapter, AuthError, AuthSession, AuthUser, CreateUserRequest,
    LoginCredentials, UpdateUserRequest,
};

#[derive(Serialize, Deserialize, Clone)]
struct StoredUser {
    id: String,
    site_id: String,
    username: String,
    name: String,
    role: String,
    status: String,
    created_at: String,
    last_login_at: Option<String>,
}

#[derive(Serialize, Deserialize, Clone)]
struct StoredSession {
    token: String,
    user_id: String,
    site_id: String,
    created_at: String,
}

#[derive(Serialize, Deserialize, Clone)]
struct StoredApiKey {
    id: String,
    key_hash: String,
    user_id: String,
    site_id: String,
    label: String,
    created_at: String,
    last_used_at: Option<String>,
    revoked: bool,
}

pub struct LocalAuthAdapter {
    base_dir: PathBuf,
    // In-memory caches keyed by fully-qualified site_id (e.g. "alice/testapp/cards")
    users: RwLock<HashMap<String, Vec<StoredUser>>>,
    sessions: RwLock<HashMap<String, StoredSession>>,  // token → session
    api_keys: RwLock<HashMap<String, Vec<StoredApiKey>>>,
}

impl LocalAuthAdapter {
    pub fn new(base_dir: &Path) -> Self {
        let adapter = LocalAuthAdapter {
            base_dir: base_dir.to_path_buf(),
            users: RwLock::new(HashMap::new()),
            sessions: RwLock::new(HashMap::new()),
            api_keys: RwLock::new(HashMap::new()),
        };
        adapter.load_from_disk();
        adapter
    }

    /// Path to the directory for a given fully-qualified site_id.
    /// e.g. "alice/testapp/cards" → base_dir/alice/testapp/cards/
    fn site_dir(&self, site_id: &str) -> PathBuf {
        self.base_dir.join(site_id)
    }

    fn load_from_disk(&self) {
        // Site directories are exactly 3 levels deep: {user}/{project}/{site}
        let base = &self.base_dir;
        let Ok(l1) = std::fs::read_dir(base) else { return };
        for e1 in l1.flatten() {
            if !e1.file_type().map(|t| t.is_dir()).unwrap_or(false) { continue; }
            let Ok(l2) = std::fs::read_dir(e1.path()) else { continue };
            for e2 in l2.flatten() {
                if !e2.file_type().map(|t| t.is_dir()).unwrap_or(false) { continue; }
                let Ok(l3) = std::fs::read_dir(e2.path()) else { continue };
                for e3 in l3.flatten() {
                    if !e3.file_type().map(|t| t.is_dir()).unwrap_or(false) { continue; }
                    let site_dir = e3.path();
                    let site_id = site_dir
                        .strip_prefix(base)
                        .map(|p| p.to_string_lossy().replace('\\', "/"))
                        .unwrap_or_default();
                    if !site_id.is_empty() {
                        self.load_site(&site_dir, &site_id);
                    }
                }
            }
        }
    }

    fn load_site(&self, site_dir: &Path, site_id: &str) {
        // Users: one file per user at users/{username}.json
        let users_dir = site_dir.join("users");
        if users_dir.is_dir() {
            if let Ok(entries) = std::fs::read_dir(&users_dir) {
                let mut users = self.users.write().unwrap();
                let site_users = users.entry(site_id.to_string()).or_default();
                for entry in entries.flatten() {
                    if entry.path().extension().and_then(|e| e.to_str()) != Some("json") { continue; }
                    if let Ok(contents) = std::fs::read_to_string(entry.path()) {
                        if let Ok(user) = serde_json::from_str::<StoredUser>(&contents) {
                            site_users.push(user);
                        }
                    }
                }
            }
        }

        // Sessions: one file per session at sessions/{token}.json
        let sessions_dir = site_dir.join("sessions");
        if sessions_dir.is_dir() {
            if let Ok(entries) = std::fs::read_dir(&sessions_dir) {
                let mut sessions = self.sessions.write().unwrap();
                for entry in entries.flatten() {
                    if entry.path().extension().and_then(|e| e.to_str()) != Some("json") { continue; }
                    if let Ok(contents) = std::fs::read_to_string(entry.path()) {
                        if let Ok(session) = serde_json::from_str::<StoredSession>(&contents) {
                            sessions.insert(session.token.clone(), session);
                        }
                    }
                }
            }
        }

        // API keys: one file per key at api_keys/{id}.json
        let keys_dir = site_dir.join("api_keys");
        if keys_dir.is_dir() {
            if let Ok(entries) = std::fs::read_dir(&keys_dir) {
                let mut api_keys = self.api_keys.write().unwrap();
                let site_keys = api_keys.entry(site_id.to_string()).or_default();
                for entry in entries.flatten() {
                    if entry.path().extension().and_then(|e| e.to_str()) != Some("json") { continue; }
                    if let Ok(contents) = std::fs::read_to_string(entry.path()) {
                        if let Ok(key) = serde_json::from_str::<StoredApiKey>(&contents) {
                            site_keys.push(key);
                        }
                    }
                }
            }
        }
    }

    fn save_user(&self, site_id: &str, user: &StoredUser) {
        let dir = self.site_dir(site_id).join("users");
        std::fs::create_dir_all(&dir).ok();
        let path = dir.join(format!("{}.json", user.username));
        std::fs::write(path, serde_json::to_string_pretty(user).unwrap()).ok();
    }

    fn delete_user_file(&self, site_id: &str, username: &str) {
        let path = self.site_dir(site_id).join("users").join(format!("{username}.json"));
        std::fs::remove_file(path).ok();
    }

    fn save_session(&self, site_id: &str, session: &StoredSession) {
        let dir = self.site_dir(site_id).join("sessions");
        std::fs::create_dir_all(&dir).ok();
        let path = dir.join(format!("{}.json", session.token));
        std::fs::write(path, serde_json::to_string_pretty(session).unwrap()).ok();
    }

    fn delete_session_file(&self, site_id: &str, token: &str) {
        let path = self.site_dir(site_id).join("sessions").join(format!("{token}.json"));
        std::fs::remove_file(path).ok();
    }

    fn save_api_key(&self, site_id: &str, key: &StoredApiKey) {
        let dir = self.site_dir(site_id).join("api_keys");
        std::fs::create_dir_all(&dir).ok();
        let path = dir.join(format!("{}.json", key.id));
        std::fs::write(path, serde_json::to_string_pretty(key).unwrap()).ok();
    }

    fn to_auth_user(u: &StoredUser) -> AuthUser {
        AuthUser {
            id: u.id.clone(),
            site_id: u.site_id.clone(),
            username: u.username.clone(),
            name: u.name.clone(),
            role: u.role.clone(),
            status: u.status.clone(),
            created_at: u.created_at.clone(),
            last_login_at: u.last_login_at.clone(),
        }
    }
}

impl AuthAdapter for LocalAuthAdapter {
    fn login(
        &self,
        site_id: &str,
        credentials: &LoginCredentials,
    ) -> Result<AuthSession, AuthError> {
        let now = chrono::Utc::now().to_rfc3339();

        let user = {
            let mut users = self.users.write().unwrap();
            let site_users = users.entry(site_id.to_string()).or_default();

            match site_users.iter_mut().find(|u| u.username == credentials.username) {
                Some(user) => {
                    user.last_login_at = Some(now.clone());
                    user.clone()
                }
                None => {
                    let new_user = StoredUser {
                        id: uuid::Uuid::new_v4().to_string(),
                        site_id: site_id.to_string(),
                        username: credentials.username.clone(),
                        name: credentials.username.clone(),
                        role: "member".to_string(),
                        status: "active".to_string(),
                        created_at: now.clone(),
                        last_login_at: Some(now.clone()),
                    };
                    site_users.push(new_user.clone());
                    new_user
                }
            }
        };

        self.save_user(site_id, &user);

        let token = uuid::Uuid::new_v4().to_string();
        let session = StoredSession {
            token: token.clone(),
            user_id: user.id.clone(),
            site_id: site_id.to_string(),
            created_at: now,
        };

        self.save_session(site_id, &session);

        {
            let mut sessions = self.sessions.write().unwrap();
            sessions.insert(token.clone(), session);
        }

        Ok(AuthSession {
            token,
            user: Self::to_auth_user(&user),
        })
    }

    fn validate_session(&self, token: &str) -> Result<AuthSession, AuthError> {
        let sessions = self.sessions.read().unwrap();
        let session = sessions.get(token).ok_or(AuthError::SessionNotFound)?;

        let users = self.users.read().unwrap();
        let site_users = users.get(&session.site_id).ok_or(AuthError::UserNotFound)?;
        let user = site_users
            .iter()
            .find(|u| u.id == session.user_id)
            .ok_or(AuthError::UserNotFound)?;

        Ok(AuthSession {
            token: token.to_string(),
            user: Self::to_auth_user(user),
        })
    }

    fn logout(&self, token: &str) -> Result<(), AuthError> {
        let site_id = {
            let mut sessions = self.sessions.write().unwrap();
            let session = sessions.remove(token).ok_or(AuthError::SessionNotFound)?;
            session.site_id
        };
        self.delete_session_file(&site_id, token);
        Ok(())
    }

    fn revoke_all_sessions(&self, site_id: &str, user_id: &str) -> Result<(), AuthError> {
        let tokens_to_remove: Vec<String> = {
            let sessions = self.sessions.read().unwrap();
            sessions
                .iter()
                .filter(|(_, s)| s.site_id == site_id && s.user_id == user_id)
                .map(|(token, _)| token.clone())
                .collect()
        };

        {
            let mut sessions = self.sessions.write().unwrap();
            for token in &tokens_to_remove {
                sessions.remove(token);
            }
        }

        for token in &tokens_to_remove {
            self.delete_session_file(site_id, token);
        }

        Ok(())
    }

    fn get_user(&self, site_id: &str, user_id: &str) -> Result<Option<AuthUser>, AuthError> {
        let users = self.users.read().unwrap();
        Ok(users.get(site_id).and_then(|site_users| {
            site_users
                .iter()
                .find(|u| u.id == user_id)
                .map(Self::to_auth_user)
        }))
    }

    fn list_users(&self, site_id: &str) -> Result<Vec<AuthUser>, AuthError> {
        let users = self.users.read().unwrap();
        Ok(users
            .get(site_id)
            .map(|site_users| site_users.iter().map(Self::to_auth_user).collect())
            .unwrap_or_default())
    }

    fn create_user(
        &self,
        site_id: &str,
        req: &CreateUserRequest,
    ) -> Result<AuthUser, AuthError> {
        let now = chrono::Utc::now().to_rfc3339();

        let user = {
            let mut users = self.users.write().unwrap();
            let site_users = users.entry(site_id.to_string()).or_default();

            if site_users.iter().any(|u| u.username == req.username) {
                return Err(AuthError::UserAlreadyExists);
            }

            let new_user = StoredUser {
                id: uuid::Uuid::new_v4().to_string(),
                site_id: site_id.to_string(),
                username: req.username.clone(),
                name: req.name.clone(),
                role: req.role.clone().unwrap_or_else(|| "member".to_string()),
                status: "active".to_string(),
                created_at: now,
                last_login_at: None,
            };
            site_users.push(new_user.clone());
            new_user
        };

        self.save_user(site_id, &user);
        Ok(Self::to_auth_user(&user))
    }

    fn update_user(
        &self,
        site_id: &str,
        user_id: &str,
        updates: &UpdateUserRequest,
    ) -> Result<AuthUser, AuthError> {
        let user = {
            let mut users = self.users.write().unwrap();
            let site_users = users
                .get_mut(site_id)
                .ok_or(AuthError::UserNotFound)?;
            let user = site_users
                .iter_mut()
                .find(|u| u.id == user_id)
                .ok_or(AuthError::UserNotFound)?;

            if let Some(name) = &updates.name { user.name = name.clone(); }
            if let Some(role) = &updates.role { user.role = role.clone(); }
            if let Some(status) = &updates.status { user.status = status.clone(); }
            user.clone()
        };

        self.save_user(site_id, &user);
        Ok(Self::to_auth_user(&user))
    }

    fn delete_user(&self, site_id: &str, user_id: &str) -> Result<(), AuthError> {
        let username = {
            let mut users = self.users.write().unwrap();
            let site_users = users
                .get_mut(site_id)
                .ok_or(AuthError::UserNotFound)?;
            let username = site_users
                .iter()
                .find(|u| u.id == user_id)
                .map(|u| u.username.clone())
                .ok_or(AuthError::UserNotFound)?;
            site_users.retain(|u| u.id != user_id);
            username
        };

        self.delete_user_file(site_id, &username);
        self.revoke_all_sessions(site_id, user_id)?;
        Ok(())
    }

    fn create_api_key(
        &self,
        site_id: &str,
        user_id: &str,
        label: &str,
    ) -> Result<ApiKey, AuthError> {
        let now = chrono::Utc::now().to_rfc3339();
        let key = uuid::Uuid::new_v4().to_string();
        let id = uuid::Uuid::new_v4().to_string();

        let stored = StoredApiKey {
            id: id.clone(),
            key_hash: key.clone(), // local adapter: store plaintext (no security needed)
            user_id: user_id.to_string(),
            site_id: site_id.to_string(),
            label: label.to_string(),
            created_at: now.clone(),
            last_used_at: None,
            revoked: false,
        };

        {
            let mut api_keys = self.api_keys.write().unwrap();
            api_keys.entry(site_id.to_string()).or_default().push(stored.clone());
        }

        self.save_api_key(site_id, &stored);

        Ok(ApiKey {
            id,
            key,
            label: label.to_string(),
            created_at: now,
        })
    }

    fn validate_api_key(&self, key: &str) -> Result<AuthSession, AuthError> {
        let api_keys = self.api_keys.read().unwrap();

        for keys in api_keys.values() {
            if let Some(stored_key) = keys.iter().find(|k| k.key_hash == key && !k.revoked) {
                let users = self.users.read().unwrap();
                let site_users = users
                    .get(&stored_key.site_id)
                    .ok_or(AuthError::UserNotFound)?;
                let user = site_users
                    .iter()
                    .find(|u| u.id == stored_key.user_id)
                    .ok_or(AuthError::UserNotFound)?;

                return Ok(AuthSession {
                    token: key.to_string(),
                    user: Self::to_auth_user(user),
                });
            }
        }

        Err(AuthError::InvalidCredentials)
    }

    fn revoke_api_key(&self, site_id: &str, key_id: &str) -> Result<(), AuthError> {
        let key = {
            let mut api_keys = self.api_keys.write().unwrap();
            let keys = api_keys
                .get_mut(site_id)
                .ok_or(AuthError::Internal("site not found".to_string()))?;
            let key = keys
                .iter_mut()
                .find(|k| k.id == key_id)
                .ok_or(AuthError::Internal("api key not found".to_string()))?;
            key.revoked = true;
            key.clone()
        };

        self.save_api_key(site_id, &key);
        Ok(())
    }

    fn list_api_keys(
        &self,
        site_id: &str,
        user_id: &str,
    ) -> Result<Vec<ApiKeyInfo>, AuthError> {
        let api_keys = self.api_keys.read().unwrap();
        Ok(api_keys
            .get(site_id)
            .map(|keys| {
                keys.iter()
                    .filter(|k| k.user_id == user_id)
                    .map(|k| ApiKeyInfo {
                        id: k.id.clone(),
                        label: k.label.clone(),
                        created_at: k.created_at.clone(),
                        last_used_at: k.last_used_at.clone(),
                        revoked: k.revoked,
                    })
                    .collect()
            })
            .unwrap_or_default())
    }
}
