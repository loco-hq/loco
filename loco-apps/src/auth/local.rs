use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::RwLock;

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

use super::secret;
use super::{
    Account, AccountType, ApiKey, ApiKeyInfo, AuthAdapter, AuthError, AuthSession, AuthUser,
    CreateUserRequest, LoginCredentials, OrgMember, OrgRole, ProjectMember, ProjectRole,
    UpdateUserRequest, PUBLIC_USERNAME, TEST_PASSWORD,
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
    /// Argon2id PHC string, never the password itself. `alias` reads files
    /// written before hashing landed; [`LocalAuthAdapter::load_from_disk`]
    /// re-hashes those in place.
    #[serde(rename = "password_hash", alias = "password")]
    password_hash: String,
    created_at: String,
    last_login_at: Option<String>,
}

/// How long a login is good for. Sessions are absolute — there is no refresh,
/// so past this point the client logs in again.
pub const SESSION_TTL_DAYS: i64 = 7;

#[derive(Serialize, Deserialize, Clone)]
struct StoredSession {
    token: String,
    identity_id: String,
    created_at: String,
    /// Absolute expiry, RFC 3339. Sessions written before expiry landed have
    /// no such field; those fall back to `created_at` + [`SESSION_TTL_DAYS`].
    #[serde(default)]
    expires_at: Option<String>,
}

impl StoredSession {
    fn new(token: String, identity_id: String, now: DateTime<Utc>) -> Self {
        StoredSession {
            token,
            identity_id,
            created_at: now.to_rfc3339(),
            expires_at: Some((now + Duration::days(SESSION_TTL_DAYS)).to_rfc3339()),
        }
    }

    /// A session whose dates cannot be parsed is treated as expired: we cannot
    /// tell how old it is, so it does not get to authenticate anything.
    fn is_expired(&self, now: DateTime<Utc>) -> bool {
        match self.expiry() {
            Some(expiry) => now >= expiry,
            None => true,
        }
    }

    fn expiry(&self) -> Option<DateTime<Utc>> {
        match &self.expires_at {
            Some(expires_at) => parse_rfc3339(expires_at),
            None => parse_rfc3339(&self.created_at).map(|at| at + Duration::days(SESSION_TTL_DAYS)),
        }
    }
}

fn parse_rfc3339(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|at| at.with_timezone(&Utc))
}

#[derive(Serialize, Deserialize, Clone)]
struct StoredApiKey {
    id: String,
    /// SHA-256 of the key, never the key itself. The plaintext key is returned
    /// once, from [`LocalAuthAdapter::create_api_key`].
    key_hash: String,
    identity_id: String,
    label: String,
    created_at: String,
    last_used_at: Option<String>,
    revoked: bool,
}

#[derive(Serialize, Deserialize, Clone)]
struct StoredOrgMember {
    org: String,
    handle: String,
    role: OrgRole,
    created_at: String,
}

#[derive(Serialize, Deserialize, Clone)]
struct StoredProjectMember {
    project: String,
    handle: String,
    role: ProjectRole,
    created_at: String,
}

/// Filesystem layout (global, not site-scoped):
///
/// ```text
/// {base}/accounts/{handle}.json
/// {base}/identities/{handle}.json
/// {base}/sessions/{token}.json
/// {base}/api_keys/{id}.json
/// {base}/org_members/{org}/{handle}.json
/// {base}/project_members/{account}/{project}/{handle}.json
/// ```
pub struct LocalAuthAdapter {
    base_dir: PathBuf,
    /// Login of an unknown handle creates a person account. Off unless the
    /// caller asks for it — see [`LocalAuthAdapter::with_auto_create`].
    auto_create: bool,
    accounts: RwLock<HashMap<String, StoredAccount>>,
    identities: RwLock<HashMap<String, StoredIdentity>>, // handle → identity
    sessions: RwLock<HashMap<String, StoredSession>>,    // token → session
    api_keys: RwLock<HashMap<String, StoredApiKey>>,     // id → key
    org_members: RwLock<HashMap<(String, String), StoredOrgMember>>, // (org, handle)
    project_members: RwLock<HashMap<(String, String), StoredProjectMember>>, // (project, handle)
}

impl LocalAuthAdapter {
    /// Auto-create comes from the environment: off unless
    /// `LOCO_AUTH_AUTO_CREATE=1` (or `cfg(test)`, for this crate's own tests).
    pub fn new(base_dir: &Path) -> Self {
        Self::with_auto_create(base_dir, Self::auto_create_from_env())
    }

    pub fn with_auto_create(base_dir: &Path, auto_create: bool) -> Self {
        let adapter = LocalAuthAdapter {
            base_dir: base_dir.to_path_buf(),
            auto_create,
            accounts: RwLock::new(HashMap::new()),
            identities: RwLock::new(HashMap::new()),
            sessions: RwLock::new(HashMap::new()),
            api_keys: RwLock::new(HashMap::new()),
            org_members: RwLock::new(HashMap::new()),
            project_members: RwLock::new(HashMap::new()),
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
        self.load_json_dir("identities", |this, mut identity: StoredIdentity| {
            if !secret::is_password_hash(&identity.password_hash) {
                // Written before credentials were hashed: the field holds the
                // password itself. Hash it and rewrite the file so the
                // plaintext stops living on disk.
                identity.password_hash = secret::hash_password(&identity.password_hash);
                this.persist_identity(&identity);
            }
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
        self.sweep_expired_sessions(Utc::now());
        self.load_json_dir("api_keys", |this, mut key: StoredApiKey| {
            if !secret::is_api_key_hash(&key.key_hash) {
                // Same upgrade as identities: the field used to hold the key.
                key.key_hash = secret::hash_api_key(&key.key_hash);
                this.persist_api_key(&key);
            }
            this.api_keys.write().unwrap().insert(key.id.clone(), key);
        });
        self.load_member_tree("org_members", |this, member: StoredOrgMember| {
            this.org_members
                .write()
                .unwrap()
                .insert((member.org.clone(), member.handle.clone()), member);
        });
        self.load_member_tree("project_members", |this, member: StoredProjectMember| {
            this.project_members
                .write()
                .unwrap()
                .insert((member.project.clone(), member.handle.clone()), member);
        });
    }

    fn load_member_tree<T: for<'de> Deserialize<'de>>(
        &self,
        dirname: &str,
        insert: impl Fn(&Self, T),
    ) {
        let dir = self.base_dir.join(dirname);
        Self::walk_json_files(&dir, |path| {
            let Ok(contents) = std::fs::read_to_string(path) else {
                return;
            };
            if let Ok(value) = serde_json::from_str::<T>(&contents) {
                insert(self, value);
            }
        });
    }

    fn walk_json_files(dir: &Path, visit: impl FnMut(&Path)) {
        let mut visit = visit;
        let mut stack = vec![dir.to_path_buf()];
        while let Some(current) = stack.pop() {
            let Ok(entries) = std::fs::read_dir(&current) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                } else if path.extension().and_then(|e| e.to_str()) == Some("json") {
                    visit(&path);
                }
            }
        }
    }

    fn load_json_dir<T: for<'de> Deserialize<'de>>(
        &self,
        dirname: &str,
        insert: impl Fn(&Self, T),
    ) {
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
            password_hash: secret::hash_password(TEST_PASSWORD),
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
        let _ = std::fs::remove_file(
            self.base_dir
                .join("accounts")
                .join(format!("{handle}.json")),
        );
        let _ = std::fs::remove_file(
            self.base_dir
                .join("identities")
                .join(format!("{handle}.json")),
        );
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

    /// Drop a session from the cache and from disk. Used when a session turns
    /// out to be expired, so the sweep is not the only thing keeping the store
    /// from growing forever.
    fn forget_session(&self, token: &str) {
        self.sessions.write().unwrap().remove(token);
        self.delete_session_file(token);
    }

    /// Boot-time cleanup: expired sessions never come back, so there is no
    /// reason to carry them in memory or leave their files on disk.
    fn sweep_expired_sessions(&self, now: DateTime<Utc>) {
        let expired: Vec<String> = {
            let sessions = self.sessions.read().unwrap();
            sessions
                .values()
                .filter(|session| session.is_expired(now))
                .map(|session| session.token.clone())
                .collect()
        };
        for token in &expired {
            self.forget_session(token);
        }
    }

    fn persist_api_key(&self, key: &StoredApiKey) {
        let dir = self.base_dir.join("api_keys");
        std::fs::create_dir_all(&dir).ok();
        let path = dir.join(format!("{}.json", key.id));
        std::fs::write(path, serde_json::to_string_pretty(key).unwrap()).ok();
    }

    fn persist_org_member(&self, member: &StoredOrgMember) {
        let dir = self.base_dir.join("org_members").join(&member.org);
        std::fs::create_dir_all(&dir).ok();
        let path = dir.join(format!("{}.json", member.handle));
        std::fs::write(path, serde_json::to_string_pretty(member).unwrap()).ok();
    }

    fn delete_org_member_file(&self, org: &str, handle: &str) {
        let path = self
            .base_dir
            .join("org_members")
            .join(org)
            .join(format!("{handle}.json"));
        let _ = std::fs::remove_file(path);
    }

    fn persist_project_member(&self, member: &StoredProjectMember) {
        let dir = self.base_dir.join("project_members").join(&member.project);
        std::fs::create_dir_all(&dir).ok();
        let path = dir.join(format!("{}.json", member.handle));
        std::fs::write(path, serde_json::to_string_pretty(member).unwrap()).ok();
    }

    fn delete_project_member_file(&self, project_id: &str, handle: &str) {
        let path = self
            .base_dir
            .join("project_members")
            .join(project_id)
            .join(format!("{handle}.json"));
        let _ = std::fs::remove_file(path);
    }

    fn to_account(account: &StoredAccount) -> Account {
        Account {
            handle: account.handle.clone(),
            account_type: account.account_type,
            created_at: account.created_at.clone(),
        }
    }

    fn member_pending(&self, handle: &str) -> bool {
        !self.identities.read().unwrap().contains_key(handle)
    }

    fn account_type(&self, handle: &str) -> Option<AccountType> {
        self.accounts
            .read()
            .unwrap()
            .get(handle)
            .map(|a| a.account_type)
    }

    fn is_valid_handle(handle: &str) -> bool {
        !handle.is_empty()
            && handle != PUBLIC_USERNAME
            && !handle.contains('/')
            && handle
                .chars()
                .all(|c| c.is_ascii_lowercase() || c == '_' || c.is_ascii_digit())
            && handle
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_lowercase() || c == '_')
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

    /// First login of an unknown handle creates a person account + identity.
    /// Only when `cfg(test)` or `LOCO_AUTH_AUTO_CREATE=1` — Hurl suites use
    /// the env flag so they do not need per-suite auth fixtures. Off by
    /// default: an unknown handle would otherwise take the `{handle}/*`
    /// namespace, because owning the person account implies developer on it.
    fn auto_create_from_env() -> bool {
        cfg!(test)
            || std::env::var("LOCO_AUTH_AUTO_CREATE")
                .ok()
                .is_some_and(|v| v == "1" || v.eq_ignore_ascii_case("true"))
    }

    fn auto_create_person(
        &self,
        handle: &str,
        password: Option<&str>,
    ) -> Result<StoredIdentity, AuthError> {
        if !Self::is_valid_handle(handle) {
            return Err(AuthError::InvalidCredentials);
        }
        let Some(password) = password.map(str::trim).filter(|s| !s.is_empty()) else {
            return Err(AuthError::InvalidCredentials);
        };
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
            password_hash: secret::hash_password(password),
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

    /// Orgs this handle solely owns. Deleting them would leave the org
    /// with no owner.
    fn sole_owned_org(&self, handle: &str) -> Option<String> {
        let members = self.org_members.read().unwrap();
        let owned: Vec<String> = members
            .values()
            .filter(|m| m.handle == handle && m.role == OrgRole::Owner)
            .map(|m| m.org.clone())
            .collect();
        owned.into_iter().find(|org| {
            members
                .values()
                .filter(|m| m.org == *org && m.role == OrgRole::Owner)
                .count()
                == 1
        })
    }

    fn purge_memberships(&self, handle: &str) {
        let org_keys: Vec<(String, String)> = {
            let members = self.org_members.read().unwrap();
            members
                .keys()
                .filter(|(_, h)| h == handle)
                .cloned()
                .collect()
        };
        {
            let mut members = self.org_members.write().unwrap();
            for key in &org_keys {
                members.remove(key);
            }
        }
        for (org, h) in &org_keys {
            self.delete_org_member_file(org, h);
        }

        let project_keys: Vec<(String, String)> = {
            let members = self.project_members.read().unwrap();
            members
                .keys()
                .filter(|(_, h)| h == handle)
                .cloned()
                .collect()
        };
        {
            let mut members = self.project_members.write().unwrap();
            for key in &project_keys {
                members.remove(key);
            }
        }
        for (project, h) in &project_keys {
            self.delete_project_member_file(project, h);
        }
    }
}

impl AuthAdapter for LocalAuthAdapter {
    fn login(&self, credentials: &LoginCredentials) -> Result<AuthSession, AuthError> {
        if credentials.username.is_empty() || credentials.username == PUBLIC_USERNAME {
            return Err(AuthError::InvalidCredentials);
        }

        let issued_at = Utc::now();
        let now = issued_at.to_rfc3339();

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
                    // Verify outside the write lock: argon2 is deliberately
                    // slow, and holding the map would serialize every login.
                    let stored = self
                        .identities
                        .read()
                        .unwrap()
                        .get(&credentials.username)
                        .cloned()
                        .ok_or(AuthError::UserNotFound)?;
                    if !secret::verify_password(
                        &stored.password_hash,
                        credentials.password.as_deref(),
                    ) {
                        return Err(AuthError::InvalidCredentials);
                    }
                    let mut identities = self.identities.write().unwrap();
                    let identity = identities
                        .get_mut(&credentials.username)
                        .ok_or(AuthError::UserNotFound)?;
                    identity.last_login_at = Some(now.clone());
                    identity.clone()
                }
                None => {
                    if !self.auto_create {
                        return Err(AuthError::InvalidCredentials);
                    }
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
        let session = StoredSession::new(token.clone(), identity.id.clone(), issued_at);
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
        let session = {
            let sessions = self.sessions.read().unwrap();
            sessions
                .get(token)
                .cloned()
                .ok_or(AuthError::SessionNotFound)?
        };
        if session.is_expired(Utc::now()) {
            self.forget_session(token);
            return Err(AuthError::SessionExpired);
        }
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

    fn create_user(&self, req: &CreateUserRequest) -> Result<AuthUser, AuthError> {
        if !Self::is_valid_handle(&req.username) {
            return Err(AuthError::InvalidCredentials);
        }
        let password = req.password.trim();
        if password.is_empty() {
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
            password_hash: secret::hash_password(password),
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
        let handle = self
            .identity_by_id(user_id)
            .map(|i| i.handle)
            .ok_or(AuthError::UserNotFound)?;
        if let Some(org) = self.sole_owned_org(&handle) {
            return Err(AuthError::SoleOrgOwner(org));
        }
        self.purge_memberships(&handle);
        self.identities.write().unwrap().remove(&handle);
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
            key_hash: secret::hash_api_key(&key),
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
        let hash = secret::hash_api_key(key);
        let api_keys = self.api_keys.read().unwrap();
        let stored = api_keys
            .values()
            .find(|k| k.key_hash == hash && !k.revoked)
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

    fn get_account(&self, handle: &str) -> Result<Option<Account>, AuthError> {
        Ok(self
            .accounts
            .read()
            .unwrap()
            .get(handle)
            .map(Self::to_account))
    }

    fn create_org(&self, handle: &str, creator_handle: &str) -> Result<Account, AuthError> {
        if !Self::is_valid_handle(handle) {
            return Err(AuthError::InvalidCredentials);
        }
        if !self.identities.read().unwrap().contains_key(creator_handle) {
            return Err(AuthError::UserNotFound);
        }
        if self.accounts.read().unwrap().contains_key(handle) {
            return Err(AuthError::UserAlreadyExists);
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
            .insert(handle.to_string(), account.clone());
        self.add_org_member(handle, creator_handle, OrgRole::Owner)?;
        Ok(Self::to_account(&account))
    }

    fn project_access(
        &self,
        identity_handle: &str,
        project_id: &str,
    ) -> Result<Option<ProjectRole>, AuthError> {
        let Some((account, _)) = project_id.split_once('/') else {
            return Ok(None);
        };

        if self
            .org_members
            .read()
            .unwrap()
            .get(&(account.to_string(), identity_handle.to_string()))
            .is_some_and(|m| m.role == OrgRole::Owner)
        {
            return Ok(Some(ProjectRole::Developer));
        }

        if let Some(member) = self
            .project_members
            .read()
            .unwrap()
            .get(&(project_id.to_string(), identity_handle.to_string()))
        {
            return Ok(Some(member.role));
        }

        if self.account_type(account) == Some(AccountType::Person) && identity_handle == account {
            return Ok(Some(ProjectRole::Developer));
        }

        Ok(None)
    }

    fn add_project_member(
        &self,
        project_id: &str,
        handle: &str,
        role: ProjectRole,
    ) -> Result<ProjectMember, AuthError> {
        if !Self::is_valid_handle(handle) || project_id.split_once('/').is_none() {
            return Err(AuthError::InvalidCredentials);
        }
        let key = (project_id.to_string(), handle.to_string());
        if self.project_members.read().unwrap().contains_key(&key) {
            return Err(AuthError::UserAlreadyExists);
        }
        let member = StoredProjectMember {
            project: project_id.to_string(),
            handle: handle.to_string(),
            role,
            created_at: chrono::Utc::now().to_rfc3339(),
        };
        self.persist_project_member(&member);
        self.project_members
            .write()
            .unwrap()
            .insert(key, member.clone());
        Ok(ProjectMember {
            project: member.project,
            handle: member.handle,
            role: member.role,
            pending: self.member_pending(handle),
        })
    }

    fn update_project_member(
        &self,
        project_id: &str,
        handle: &str,
        role: ProjectRole,
    ) -> Result<ProjectMember, AuthError> {
        let key = (project_id.to_string(), handle.to_string());
        let member = {
            let mut members = self.project_members.write().unwrap();
            let member = members.get_mut(&key).ok_or(AuthError::UserNotFound)?;
            member.role = role;
            member.clone()
        };
        self.persist_project_member(&member);
        Ok(ProjectMember {
            project: member.project,
            handle: member.handle,
            role: member.role,
            pending: self.member_pending(handle),
        })
    }

    fn remove_project_member(&self, project_id: &str, handle: &str) -> Result<(), AuthError> {
        let key = (project_id.to_string(), handle.to_string());
        self.project_members
            .write()
            .unwrap()
            .remove(&key)
            .ok_or(AuthError::UserNotFound)?;
        self.delete_project_member_file(project_id, handle);
        Ok(())
    }

    fn list_project_members(&self, project_id: &str) -> Result<Vec<ProjectMember>, AuthError> {
        let members = self.project_members.read().unwrap();
        Ok(members
            .values()
            .filter(|m| m.project == project_id)
            .map(|m| ProjectMember {
                project: m.project.clone(),
                handle: m.handle.clone(),
                role: m.role,
                pending: self.member_pending(&m.handle),
            })
            .collect())
    }

    fn add_org_member(
        &self,
        org: &str,
        handle: &str,
        role: OrgRole,
    ) -> Result<OrgMember, AuthError> {
        if !Self::is_valid_handle(handle) || !Self::is_valid_handle(org) {
            return Err(AuthError::InvalidCredentials);
        }
        match self.account_type(org) {
            Some(AccountType::Org) => {}
            Some(AccountType::Person) => return Err(AuthError::Unauthorized),
            None => return Err(AuthError::UserNotFound),
        }
        let key = (org.to_string(), handle.to_string());
        if self.org_members.read().unwrap().contains_key(&key) {
            return Err(AuthError::UserAlreadyExists);
        }
        let member = StoredOrgMember {
            org: org.to_string(),
            handle: handle.to_string(),
            role,
            created_at: chrono::Utc::now().to_rfc3339(),
        };
        self.persist_org_member(&member);
        self.org_members
            .write()
            .unwrap()
            .insert(key, member.clone());
        Ok(OrgMember {
            org: member.org,
            handle: member.handle,
            role: member.role,
            pending: self.member_pending(handle),
        })
    }

    fn remove_org_member(&self, org: &str, handle: &str) -> Result<(), AuthError> {
        let key = (org.to_string(), handle.to_string());
        self.org_members
            .write()
            .unwrap()
            .remove(&key)
            .ok_or(AuthError::UserNotFound)?;
        self.delete_org_member_file(org, handle);
        Ok(())
    }

    fn list_org_members(&self, org: &str) -> Result<Vec<OrgMember>, AuthError> {
        let members = self.org_members.read().unwrap();
        Ok(members
            .values()
            .filter(|m| m.org == org)
            .map(|m| OrgMember {
                org: m.org.clone(),
                handle: m.handle.clone(),
                role: m.role,
                pending: self.member_pending(&m.handle),
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

    /// `cfg(test)` turns auto-create on for `new`, so the production default
    /// has to be asked for explicitly.
    fn adapter_without_auto_create() -> (tempfile::TempDir, LocalAuthAdapter) {
        let dir = tempfile::TempDir::new().unwrap();
        let adapter = LocalAuthAdapter::with_auto_create(dir.path(), false);
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

    fn login_ok(adapter: &LocalAuthAdapter, username: &str) -> AuthSession {
        login(adapter, username, Some(TEST_PASSWORD))
    }

    #[test]
    fn seed_creates_alice_bob_persons_and_loco_org() {
        let (_dir, adapter) = adapter();
        let accounts = adapter.accounts.read().unwrap();
        assert_eq!(
            accounts.get("alice").unwrap().account_type,
            AccountType::Person
        );
        assert_eq!(
            accounts.get("bob").unwrap().account_type,
            AccountType::Person
        );
        assert_eq!(accounts.get("loco").unwrap().account_type, AccountType::Org);
        assert!(adapter.identities.read().unwrap().contains_key("alice"));
        assert!(adapter.identities.read().unwrap().contains_key("bob"));
        assert!(!adapter.identities.read().unwrap().contains_key("loco"));
    }

    #[test]
    fn login_does_not_need_a_site() {
        let (_dir, adapter) = adapter();
        let session = login_ok(&adapter, "alice");
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
                password: Some(TEST_PASSWORD.to_string()),
            })
            .unwrap_err();
        assert!(matches!(err, AuthError::InvalidCredentials));
    }

    #[test]
    fn login_without_password_fails() {
        let (_dir, adapter) = adapter();
        let err = adapter
            .login(&LoginCredentials {
                username: "alice".to_string(),
                password: None,
            })
            .unwrap_err();
        assert!(matches!(err, AuthError::InvalidCredentials));
    }

    #[test]
    fn login_unknown_handle_creates_person_when_auto_create_on() {
        let dir = tempfile::TempDir::new().unwrap();
        let adapter = LocalAuthAdapter::with_auto_create(dir.path(), true);
        let session = login_ok(&adapter, "testuser");
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
        let session = login_ok(&adapter, "alice");
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

    /// Rewrite a live session so it looks like it was issued `days` ago —
    /// cheaper than waiting out a real TTL.
    fn backdate_session(adapter: &LocalAuthAdapter, token: &str, days: i64) {
        let mut session = adapter
            .sessions
            .read()
            .unwrap()
            .get(token)
            .cloned()
            .expect("session is live");
        let issued_at = Utc::now() - Duration::days(days);
        session.created_at = issued_at.to_rfc3339();
        session.expires_at = Some((issued_at + Duration::days(SESSION_TTL_DAYS)).to_rfc3339());
        adapter.persist_session(&session);
        adapter
            .sessions
            .write()
            .unwrap()
            .insert(token.to_string(), session);
    }

    fn session_file(dir: &Path, token: &str) -> PathBuf {
        dir.join("sessions").join(format!("{token}.json"))
    }

    #[test]
    fn session_past_ttl_is_rejected_and_forgotten() {
        let (dir, adapter) = adapter();
        let token = login_ok(&adapter, "alice").token;
        backdate_session(&adapter, &token, SESSION_TTL_DAYS + 1);

        let err = adapter.validate_session(&token).unwrap_err();
        assert!(matches!(err, AuthError::SessionExpired));

        // Rejecting it also drops it, so it cannot be retried and cannot keep
        // occupying the cache or the disk.
        assert!(!adapter.sessions.read().unwrap().contains_key(&token));
        assert!(!session_file(dir.path(), &token).exists());
        assert!(matches!(
            adapter.validate_session(&token).unwrap_err(),
            AuthError::SessionNotFound
        ));
    }

    #[test]
    fn session_within_ttl_still_validates() {
        let (_dir, adapter) = adapter();
        let token = login_ok(&adapter, "alice").token;
        backdate_session(&adapter, &token, SESSION_TTL_DAYS - 1);

        let validated = adapter.validate_session(&token).unwrap();
        assert_eq!(validated.user.username, "alice");
    }

    #[test]
    fn expired_sessions_are_swept_on_load() {
        let (dir, adapter) = adapter();
        let fresh = login_ok(&adapter, "alice").token;
        let stale = login_ok(&adapter, "bob").token;
        backdate_session(&adapter, &stale, SESSION_TTL_DAYS + 1);

        drop(adapter);
        let reloaded = LocalAuthAdapter::new(dir.path());

        assert!(!reloaded.sessions.read().unwrap().contains_key(&stale));
        assert!(!session_file(dir.path(), &stale).exists());
        assert_eq!(
            reloaded.validate_session(&fresh).unwrap().user.username,
            "alice"
        );
    }

    /// Session files written before expiry landed carry only `created_at`.
    #[test]
    fn session_file_without_expires_at_ages_out_from_created_at() {
        let (dir, adapter) = adapter();
        let identity_id = login_ok(&adapter, "alice").user.id;
        drop(adapter);

        let write_legacy = |token: &str, age_days: i64| {
            let created_at = (Utc::now() - Duration::days(age_days)).to_rfc3339();
            std::fs::write(
                session_file(dir.path(), token),
                serde_json::json!({
                    "token": token,
                    "identity_id": identity_id,
                    "created_at": created_at,
                })
                .to_string(),
            )
            .unwrap();
        };
        write_legacy("legacy-fresh", 1);
        write_legacy("legacy-stale", SESSION_TTL_DAYS + 1);

        let adapter = LocalAuthAdapter::new(dir.path());
        assert_eq!(
            adapter
                .validate_session("legacy-fresh")
                .unwrap()
                .user
                .username,
            "alice"
        );
        assert!(!session_file(dir.path(), "legacy-stale").exists());
    }

    /// An undateable session is not a session we can vouch for.
    #[test]
    fn session_with_unparseable_dates_is_expired() {
        let session = StoredSession {
            token: "t".to_string(),
            identity_id: "i".to_string(),
            created_at: "whenever".to_string(),
            expires_at: None,
        };
        assert!(session.is_expired(Utc::now()));
        assert!(StoredSession {
            expires_at: Some("whenever".to_string()),
            ..session
        }
        .is_expired(Utc::now()));
    }

    #[test]
    fn api_key_hangs_off_identity() {
        let (_dir, adapter) = adapter();
        let session = login_ok(&adapter, "alice");
        let key = adapter.create_api_key(&session.user.id, "ci").unwrap();
        let via_key = adapter.validate_api_key(&key.key).unwrap();
        assert_eq!(via_key.user.username, "alice");
        assert_eq!(via_key.user.id, session.user.id);

        adapter.revoke_api_key(&session.user.id, &key.id).unwrap();
        assert!(adapter.validate_api_key(&key.key).is_err());
    }

    #[test]
    fn api_key_file_stores_a_digest_not_the_key() {
        let (dir, adapter) = adapter();
        let session = login_ok(&adapter, "alice");
        let key = adapter.create_api_key(&session.user.id, "ci").unwrap();

        let path = dir.path().join("api_keys").join(format!("{}.json", key.id));
        let contents = std::fs::read_to_string(&path).unwrap();
        assert!(
            !contents.contains(&key.key),
            "api key file holds the bearer token: {contents}"
        );
        let stored: StoredApiKey = serde_json::from_str(&contents).unwrap();
        assert_eq!(stored.key_hash, secret::hash_api_key(&key.key));
    }

    /// Files written before hashing landed hold the password itself. Loading
    /// them upgrades the file in place — the login still works, and the
    /// plaintext stops living on disk.
    #[test]
    fn legacy_plaintext_identity_is_rehashed_on_load() {
        let dir = tempfile::TempDir::new().unwrap();
        {
            let adapter = LocalAuthAdapter::new(dir.path());
            let identity = adapter.identities.read().unwrap()["alice"].clone();
            let legacy = serde_json::json!({
                "id": identity.id,
                "handle": "alice",
                "name": "Alice",
                "password": TEST_PASSWORD,
                "created_at": identity.created_at,
                "last_login_at": null,
            });
            std::fs::write(
                dir.path().join("identities/alice.json"),
                serde_json::to_string_pretty(&legacy).unwrap(),
            )
            .unwrap();
        }

        let adapter = LocalAuthAdapter::new(dir.path());
        assert_eq!(login_ok(&adapter, "alice").user.username, "alice");
        assert!(adapter
            .login(&LoginCredentials {
                username: "alice".to_string(),
                password: Some("nope".to_string()),
            })
            .is_err());

        let contents = std::fs::read_to_string(dir.path().join("identities/alice.json")).unwrap();
        assert!(
            !contents.contains(&format!("\"{TEST_PASSWORD}\"")),
            "identity file still holds the plaintext password: {contents}"
        );
        let stored: StoredIdentity = serde_json::from_str(&contents).unwrap();
        assert!(secret::is_password_hash(&stored.password_hash));
    }

    #[test]
    fn legacy_plaintext_api_key_is_rehashed_on_load() {
        let dir = tempfile::TempDir::new().unwrap();
        let raw_key = {
            let adapter = LocalAuthAdapter::new(dir.path());
            let session = login_ok(&adapter, "alice");
            let key = adapter.create_api_key(&session.user.id, "ci").unwrap();
            let mut stored = adapter.api_keys.read().unwrap()[&key.id].clone();
            stored.key_hash = key.key.clone();
            adapter.persist_api_key(&stored);
            key.key
        };

        let adapter = LocalAuthAdapter::new(dir.path());
        assert_eq!(
            adapter.validate_api_key(&raw_key).unwrap().user.username,
            "alice"
        );
        for entry in std::fs::read_dir(dir.path().join("api_keys")).unwrap() {
            let contents = std::fs::read_to_string(entry.unwrap().path()).unwrap();
            assert!(
                !contents.contains(&raw_key),
                "api key file still holds the bearer token: {contents}"
            );
        }
    }

    #[test]
    fn public_session_is_not_site_scoped() {
        let session = AuthSession::public();
        assert_eq!(session.user.username, PUBLIC_USERNAME);
        let json = serde_json::to_value(&session.user).unwrap();
        assert!(json.get("site_id").is_none());
    }

    #[test]
    fn person_is_implicit_developer_of_own_projects() {
        let (_dir, adapter) = adapter();
        assert_eq!(
            adapter.project_access("alice", "alice/testapp").unwrap(),
            Some(ProjectRole::Developer)
        );
        assert_eq!(
            adapter.project_access("bob", "alice/testapp").unwrap(),
            None
        );
    }

    #[test]
    fn org_owner_is_developer_on_every_org_project() {
        let (_dir, adapter) = adapter();
        adapter.create_org("acme", "alice").unwrap();
        assert_eq!(
            adapter.project_access("alice", "acme/crm").unwrap(),
            Some(ProjectRole::Developer)
        );
        assert_eq!(adapter.project_access("bob", "acme/crm").unwrap(), None);
    }

    #[test]
    fn project_editor_cannot_develop() {
        let (_dir, adapter) = adapter();
        adapter
            .add_project_member("alice/testapp", "bob", ProjectRole::Editor)
            .unwrap();
        assert_eq!(
            adapter.project_access("bob", "alice/testapp").unwrap(),
            Some(ProjectRole::Editor)
        );
        assert!(!adapter
            .project_access("bob", "alice/testapp")
            .unwrap()
            .unwrap()
            .can_develop());
        assert!(adapter
            .project_access("bob", "alice/testapp")
            .unwrap()
            .unwrap()
            .can_edit_data());
    }

    #[test]
    fn invite_unknown_handle_is_pending() {
        let (_dir, adapter) = adapter();
        let member = adapter
            .add_project_member("alice/testapp", "carol", ProjectRole::Editor)
            .unwrap();
        assert!(member.pending);
        let listed = adapter.list_project_members("alice/testapp").unwrap();
        assert!(listed.iter().any(|m| m.handle == "carol" && m.pending));
    }

    #[test]
    fn create_org_rejects_taken_handle() {
        let (_dir, adapter) = adapter();
        let err = adapter.create_org("alice", "bob").unwrap_err();
        assert!(matches!(err, AuthError::UserAlreadyExists));
    }

    #[test]
    fn login_unknown_handle_without_auto_create_squats_nothing() {
        let (dir, adapter) = adapter_without_auto_create();
        let err = adapter
            .login(&LoginCredentials {
                username: "acme".to_string(),
                password: Some(TEST_PASSWORD.to_string()),
            })
            .unwrap_err();
        assert!(matches!(err, AuthError::InvalidCredentials));
        assert!(!adapter.accounts.read().unwrap().contains_key("acme"));
        assert!(!adapter.identities.read().unwrap().contains_key("acme"));
        assert!(!dir.path().join("identities/acme.json").exists());
        assert!(!dir.path().join("accounts/acme.json").exists());
        assert_eq!(adapter.project_access("acme", "acme/crm").unwrap(), None);
    }

    #[test]
    fn seeded_login_still_works_without_auto_create() {
        let (_dir, adapter) = adapter_without_auto_create();
        let session = login_ok(&adapter, "alice");
        assert_eq!(session.user.username, "alice");
    }

    #[test]
    fn auto_create_rejects_missing_password() {
        let (_dir, adapter) = adapter();
        let err = adapter
            .login(&LoginCredentials {
                username: "newbie".to_string(),
                password: None,
            })
            .unwrap_err();
        assert!(matches!(err, AuthError::InvalidCredentials));
        assert!(!adapter.identities.read().unwrap().contains_key("newbie"));
    }

    #[test]
    fn create_user_rejects_empty_password() {
        let (_dir, adapter) = adapter();
        let err = adapter
            .create_user(&CreateUserRequest {
                username: "newbie".to_string(),
                name: "Newbie".to_string(),
                password: "  ".to_string(),
            })
            .unwrap_err();
        assert!(matches!(err, AuthError::InvalidCredentials));
    }

    #[test]
    fn delete_user_purges_memberships_so_handle_reuse_does_not_inherit() {
        let (_dir, adapter) = adapter();
        let bob = login_ok(&adapter, "bob");
        adapter.create_org("acme", "alice").unwrap();
        adapter
            .add_org_member("acme", "bob", OrgRole::Member)
            .unwrap();
        adapter
            .add_project_member("alice/testapp", "bob", ProjectRole::Editor)
            .unwrap();

        adapter.delete_user(&bob.user.id).unwrap();
        assert!(adapter
            .list_org_members("acme")
            .unwrap()
            .iter()
            .all(|m| m.handle != "bob"));
        assert!(adapter
            .list_project_members("alice/testapp")
            .unwrap()
            .iter()
            .all(|m| m.handle != "bob"));

        adapter
            .create_user(&CreateUserRequest {
                username: "bob".to_string(),
                name: "Bob".to_string(),
                password: TEST_PASSWORD.to_string(),
            })
            .unwrap();
        assert_eq!(
            adapter.project_access("bob", "alice/testapp").unwrap(),
            None
        );
        assert!(adapter
            .list_org_members("acme")
            .unwrap()
            .iter()
            .all(|m| m.handle != "bob"));
    }

    #[test]
    fn delete_user_refuses_sole_org_owner() {
        let (_dir, adapter) = adapter();
        let alice = login_ok(&adapter, "alice");
        adapter.create_org("acme", "alice").unwrap();
        let err = adapter.delete_user(&alice.user.id).unwrap_err();
        assert!(matches!(err, AuthError::SoleOrgOwner(org) if org == "acme"));
        assert!(adapter.identities.read().unwrap().contains_key("alice"));

        adapter
            .add_org_member("acme", "bob", OrgRole::Owner)
            .unwrap();
        adapter.delete_user(&alice.user.id).unwrap();
        assert!(!adapter.identities.read().unwrap().contains_key("alice"));
        assert!(adapter
            .list_org_members("acme")
            .unwrap()
            .iter()
            .any(|m| m.handle == "bob" && m.role == OrgRole::Owner));
    }
}
