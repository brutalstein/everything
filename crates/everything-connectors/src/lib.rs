mod crypto;
mod http;
mod providers;
mod store;
mod vault;

use anyhow::{Context, Result, anyhow};
use everything_domain::{
    ConnectorActionRequest, ConnectorActionResponse, ConnectorAuditRecord,
    ConnectorConfigureRequest, ConnectorDescriptor, ConnectorProvider, ConnectorRisk,
    ConnectorStatus, OAuthCallbackRequest, OAuthStartRequest, OAuthStartResponse,
};
use http::{CurlHttpClient, HttpRequest};
use providers::{builtin_actions, provider_spec};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use store::{ConnectorStore, StoredConnectorConfig, StoredIdempotencyResult, StoredOAuthSession};
use vault::PlatformSecretVault;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TokenBundle {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    token_type: Option<String>,
    #[serde(default)]
    scope: Option<String>,
    #[serde(default)]
    expires_in: Option<u64>,
    #[serde(default)]
    refresh_expires_in: Option<u64>,
    #[serde(default)]
    open_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ConnectorRuntime {
    store: ConnectorStore,
    vault: PlatformSecretVault,
    http: CurlHttpClient,
    callback_base: String,
    allow_custom_connectors: bool,
    refresh_lock: Arc<Mutex<()>>,
}

impl ConnectorRuntime {
    pub fn new(
        database_path: impl Into<PathBuf>,
        callback_port: u16,
        timeout_millis: u64,
        max_response_bytes: u64,
        allow_custom_connectors: bool,
    ) -> Result<Self> {
        Ok(Self {
            store: ConnectorStore::new(database_path)?,
            vault: PlatformSecretVault::new("dev.everything.connectors"),
            http: CurlHttpClient::new(timeout_millis, max_response_bytes),
            callback_base: format!("http://127.0.0.1:{callback_port}/v1/connectors/oauth/callback"),
            allow_custom_connectors,
            refresh_lock: Arc::new(Mutex::new(())),
        })
    }

    pub fn callback_base(&self) -> &str {
        &self.callback_base
    }

    pub fn vault_backend(&self) -> &'static str {
        self.vault.backend_name()
    }

    pub fn vault_available(&self) -> bool {
        self.vault.available()
    }

    pub fn list(&self) -> Result<Vec<ConnectorDescriptor>> {
        let configs = self
            .store
            .list_configs()?
            .into_iter()
            .map(|config| (config.provider, config))
            .collect::<BTreeMap<_, _>>();
        let providers = [
            ConnectorProvider::Gmail,
            ConnectorProvider::Spotify,
            ConnectorProvider::Instagram,
            ConnectorProvider::TikTok,
            ConnectorProvider::GitHub,
        ];
        providers
            .into_iter()
            .map(|provider| self.describe(provider, configs.get(&provider)))
            .collect()
    }

    pub fn get(&self, provider: ConnectorProvider) -> Result<ConnectorDescriptor> {
        let config = self.store.get_config(provider)?;
        self.describe(provider, config.as_ref())
    }

    pub fn configure(&self, request: ConnectorConfigureRequest) -> Result<ConnectorDescriptor> {
        anyhow::ensure!(
            request.provider != ConnectorProvider::Custom || self.allow_custom_connectors,
            "custom connectors are disabled by runtime policy"
        );
        let client_id = request.client_id.trim();
        let supplied_access_token = request
            .access_token
            .as_deref()
            .map(str::trim)
            .filter(|token| !token.is_empty())
            .map(str::to_owned);
        anyhow::ensure!(
            !client_id.is_empty()
                || supplied_access_token.is_some()
                || env_token_available(request.provider),
            "client_id must not be empty unless an access token is supplied"
        );
        let spec = provider_spec(request.provider);
        if spec.requires_client_secret
            && supplied_access_token.is_none()
            && !env_token_available(request.provider)
        {
            anyhow::ensure!(
                request
                    .client_secret
                    .as_ref()
                    .is_some_and(|secret| !secret.trim().is_empty())
                    || self
                        .vault
                        .get(&secret_key(request.provider, "client_secret"))?
                        .is_some(),
                "{} requires a client secret for OAuth authorization",
                spec.display_name
            );
        }
        anyhow::ensure!(
            self.vault.available()
                || (request.client_secret.is_none() && supplied_access_token.is_none()),
            "OS secret vault is unavailable; configure secrets through EVERYTHING_SECRET_* environment variables"
        );
        if let Some(secret) = request.client_secret.as_deref() {
            self.vault.set(
                &secret_key(request.provider, "client_secret"),
                secret.trim(),
            )?;
        }
        if let Some(token) = supplied_access_token.as_deref() {
            self.vault
                .set(&secret_key(request.provider, "access_token"), token)?;
        }
        let mut scopes = if request.scopes.is_empty() {
            spec.default_scopes
                .iter()
                .map(|scope| (*scope).to_owned())
                .collect()
        } else {
            deduplicate(request.scopes)
        };
        if request.provider == ConnectorProvider::GitHub
            && (supplied_access_token.is_some() || env_token_available(request.provider))
        {
            scopes.push("github:token".to_owned());
            scopes = deduplicate(scopes);
        }
        let granted_scopes =
            if supplied_access_token.is_some() || env_token_available(request.provider) {
                scopes.clone()
            } else {
                Vec::new()
            };
        let redirect_uri = request
            .redirect_uri
            .unwrap_or_else(|| format!("{}/{}", self.callback_base, request.provider.as_str()));
        validate_redirect_uri(&redirect_uri)?;
        let config = StoredConnectorConfig {
            provider: request.provider,
            client_id: client_id.to_owned(),
            redirect_uri,
            scopes,
            metadata: request.metadata,
            configured_at_epoch_millis: now_millis(),
            account_label: None,
            granted_scopes,
            token_expires_at_epoch_millis: None,
            last_error: None,
        };
        self.store.upsert_config(&config)?;
        self.describe(request.provider, Some(&config))
    }

    pub fn disconnect(&self, provider: ConnectorProvider) -> Result<ConnectorDescriptor> {
        for name in ["access_token", "refresh_token", "client_secret"] {
            self.vault.delete(&secret_key(provider, name))?;
        }
        self.store.delete_config(provider)?;
        self.describe(provider, None)
    }

    pub fn start_oauth(&self, request: OAuthStartRequest) -> Result<OAuthStartResponse> {
        self.purge_expired_oauth_secrets()?;
        let config = self.store.get_config(request.provider)?.ok_or_else(|| {
            anyhow!(
                "connector '{}' is not configured",
                request.provider.as_str()
            )
        })?;
        let spec = provider_spec(request.provider);
        anyhow::ensure!(
            !spec.authorization_endpoint.is_empty(),
            "connector has no OAuth endpoint"
        );
        anyhow::ensure!(
            self.vault.available(),
            "OS secret vault '{}' is unavailable; OAuth authorization needs a writable vault for short-lived PKCE state and refresh tokens",
            self.vault.backend_name()
        );
        validate_provider_url(&spec, spec.authorization_endpoint)?;
        validate_provider_url(&spec, spec.token_endpoint)?;
        let state = crypto::random_token(32)?;
        let verifier = if spec.supports_pkce {
            crypto::pkce_verifier()?
        } else {
            String::new()
        };
        let challenge = if !spec.supports_pkce {
            String::new()
        } else if spec.pkce_hex {
            crypto::pkce_challenge_tiktok(&verifier)
        } else {
            crypto::pkce_challenge_s256(&verifier)
        };
        let expires_at = now_millis().saturating_add(10 * 60 * 1000);
        let verifier_key = oauth_verifier_key(request.provider, &state);
        if spec.supports_pkce {
            self.vault.set(&verifier_key, &verifier)?;
        }
        let session_result = self.store.save_oauth_session(&StoredOAuthSession {
            state: state.clone(),
            provider: request.provider,
            redirect_uri: config.redirect_uri.clone(),
            // Kept for backward-compatible deserialization only. New sessions never
            // persist PKCE material outside the operating-system secret vault.
            verifier: String::new(),
            created_at_epoch_millis: now_millis(),
            expires_at_epoch_millis: expires_at,
        });
        if let Err(error) = session_result {
            if spec.supports_pkce {
                let _ = self.vault.delete(&verifier_key);
            }
            return Err(error);
        }

        let mut parameters = vec![
            (spec.client_id_parameter, config.client_id.as_str()),
            ("response_type", "code"),
            ("redirect_uri", config.redirect_uri.as_str()),
            ("state", state.as_str()),
        ];
        let scope = config.scopes.join(spec.scope_separator);
        parameters.push(("scope", scope.as_str()));
        if spec.supports_pkce {
            parameters.push(("code_challenge", challenge.as_str()));
            parameters.push(("code_challenge_method", "S256"));
        }
        let mut owned_extras = Vec::new();
        match request.provider {
            ConnectorProvider::Gmail => {
                owned_extras.push(("access_type".to_owned(), "offline".to_owned()));
                owned_extras.push(("include_granted_scopes".to_owned(), "true".to_owned()));
                if request.force_consent {
                    owned_extras.push(("prompt".to_owned(), "consent".to_owned()));
                }
            }
            ConnectorProvider::TikTok => {
                owned_extras.push((
                    "disable_auto_auth".to_owned(),
                    if request.force_consent { "1" } else { "0" }.to_owned(),
                ));
            }
            _ => {}
        }
        let mut url = format!("{}?", spec.authorization_endpoint);
        let mut first = true;
        for (name, value) in parameters
            .into_iter()
            .map(|(name, value)| (name.to_owned(), value.to_owned()))
            .chain(owned_extras)
        {
            if !first {
                url.push('&');
            }
            first = false;
            url.push_str(&crypto::percent_encode(&name));
            url.push('=');
            url.push_str(&crypto::percent_encode(&value));
        }
        Ok(OAuthStartResponse {
            provider: request.provider,
            authorization_url: url,
            redirect_uri: config.redirect_uri,
            state,
            expires_at_epoch_millis: expires_at,
        })
    }

    pub fn complete_oauth(&self, request: OAuthCallbackRequest) -> Result<ConnectorDescriptor> {
        self.purge_expired_oauth_secrets()?;
        let session = self
            .store
            .consume_oauth_session(request.provider, &request.state, now_millis())?
            .ok_or_else(|| anyhow!("OAuth state is invalid, expired, or already consumed"))?;
        let spec = provider_spec(request.provider);
        let verifier_key = oauth_verifier_key(request.provider, &request.state);
        let verifier = if spec.supports_pkce {
            let protected = self.vault.get(&verifier_key)?;
            let _ = self.vault.delete(&verifier_key);
            protected
                .filter(|value| !value.is_empty())
                .or_else(|| (!session.verifier.is_empty()).then_some(session.verifier.clone()))
                .ok_or_else(|| anyhow!("OAuth PKCE verifier is missing; restart authorization"))?
        } else {
            String::new()
        };
        if let Some(error) = request.error.as_deref() {
            anyhow::bail!(
                "OAuth authorization failed: {error}: {}",
                request.error_description.unwrap_or_default()
            );
        }
        let code = request
            .code
            .as_deref()
            .map(str::trim)
            .filter(|code| !code.is_empty())
            .ok_or_else(|| anyhow!("OAuth callback did not include an authorization code"))?;
        let mut config = self
            .store
            .get_config(request.provider)?
            .ok_or_else(|| anyhow!("connector configuration disappeared during OAuth"))?;
        let client_secret = self
            .vault
            .get(&secret_key(request.provider, "client_secret"))?;
        let mut form = vec![
            (
                spec.client_id_parameter.to_owned(),
                config.client_id.clone(),
            ),
            ("code".to_owned(), code.to_owned()),
            ("grant_type".to_owned(), "authorization_code".to_owned()),
            ("redirect_uri".to_owned(), session.redirect_uri),
        ];
        if spec.supports_pkce {
            form.push(("code_verifier".to_owned(), verifier));
        }
        if let Some(secret) = client_secret {
            form.push(("client_secret".to_owned(), secret));
        }
        let response = self.http.execute(&HttpRequest {
            method: "POST".to_owned(),
            url: spec.token_endpoint.to_owned(),
            headers: vec![
                (
                    "Content-Type".to_owned(),
                    "application/x-www-form-urlencoded".to_owned(),
                ),
                ("Accept".to_owned(), "application/json".to_owned()),
            ],
            form,
            json_body: None,
        })?;
        let body = parse_json_response(&response, "OAuth token exchange")?;
        let token = parse_token_bundle(&body)?;
        self.persist_token(request.provider, &token)?;
        config.granted_scopes = token
            .scope
            .as_deref()
            .map(parse_scopes)
            .unwrap_or_else(|| config.scopes.clone());
        if request.provider == ConnectorProvider::GitHub {
            config.granted_scopes.push("github:token".to_owned());
            config.granted_scopes = deduplicate(config.granted_scopes);
        }
        config.token_expires_at_epoch_millis = token
            .expires_in
            .map(|seconds| now_millis().saturating_add(u128::from(seconds) * 1000));
        config.last_error = None;
        self.store.upsert_config(&config)?;

        if let Ok(label) = self.fetch_account_label(request.provider) {
            config.account_label = label;
            self.store.upsert_config(&config)?;
        }
        self.describe(request.provider, Some(&config))
    }

    pub fn execute(&self, request: ConnectorActionRequest) -> Result<ConnectorActionResponse> {
        let descriptor = self.get(request.provider)?;
        anyhow::ensure!(
            descriptor.connected,
            "connector '{}' is not connected",
            request.provider.as_str()
        );
        let action = descriptor
            .actions
            .iter()
            .find(|action| action.action_id == request.action_id)
            .cloned()
            .ok_or_else(|| anyhow!("unknown connector action '{}'", request.action_id))?;
        validate_action_input(&request.input, &action.input_schema)?;
        validate_idempotency_key(request.idempotency_key.as_deref())?;
        if action.risk.requires_explicit_approval() && !action.idempotent && !request.dry_run {
            anyhow::ensure!(
                request.idempotency_key.is_some(),
                "non-idempotent external actions require an idempotency key"
            );
        }
        let granted = descriptor
            .granted_scopes
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>();
        let missing = action
            .required_scopes
            .iter()
            .filter(|scope| !granted.contains(*scope))
            .cloned()
            .collect::<Vec<_>>();
        anyhow::ensure!(
            missing.is_empty(),
            "connector is missing required scopes: {}",
            missing.join(", ")
        );
        if action.risk.requires_explicit_approval() && !request.approval_granted {
            return Ok(ConnectorActionResponse {
                provider: request.provider,
                action_id: request.action_id,
                status: "awaiting_approval".to_owned(),
                risk: action.risk,
                executed: false,
                output: json!({"preview": request.input, "reason": "external account mutation requires explicit approval"}),
                external_reference: None,
                retry_after_millis: None,
                warnings: descriptor.limitations,
            });
        }
        if request.dry_run {
            return Ok(ConnectorActionResponse {
                provider: request.provider,
                action_id: request.action_id,
                status: "dry_run".to_owned(),
                risk: action.risk,
                executed: false,
                output: json!({"validated": true, "input": request.input}),
                external_reference: None,
                retry_after_millis: None,
                warnings: descriptor.limitations,
            });
        }
        if let Some(key) = request.idempotency_key.as_deref() {
            if let Some(record) = self.store.get_idempotency(key)? {
                anyhow::ensure!(
                    record.provider == request.provider && record.action_id == request.action_id,
                    "idempotency key belongs to a different connector action"
                );
                return serde_json::from_value(record.response)
                    .context("stored connector response is corrupt");
            }
        }

        let started = now_millis();
        let audit_id = format!("ca-{}", crypto::random_token(18)?);
        let input_hash = blake3::hash(&serde_json::to_vec(&request.input)?)
            .to_hex()
            .to_string();
        let result = self.execute_inner(&request, action.risk);
        let finished = now_millis();
        let (status, error_code) = match &result {
            Ok(response) => (response.status.clone(), None),
            Err(error) => ("failed".to_owned(), Some(classify_error(error))),
        };
        self.store.save_audit(&ConnectorAuditRecord {
            audit_id,
            provider: request.provider,
            action_id: request.action_id.clone(),
            risk: action.risk,
            started_at_epoch_millis: started,
            finished_at_epoch_millis: finished,
            status,
            approval_granted: request.approval_granted,
            input_hash,
            idempotency_key: request.idempotency_key.clone(),
            error_code,
        })?;
        let response = result?;
        if let Some(key) = request.idempotency_key {
            self.store.save_idempotency(&StoredIdempotencyResult {
                key,
                provider: request.provider,
                action_id: request.action_id,
                created_at_epoch_millis: finished,
                response: serde_json::to_value(&response)?,
            })?;
        }
        Ok(response)
    }

    pub fn audits(&self, limit: usize) -> Result<Vec<ConnectorAuditRecord>> {
        self.store.list_audits(limit)
    }

    fn describe(
        &self,
        provider: ConnectorProvider,
        config: Option<&StoredConnectorConfig>,
    ) -> Result<ConnectorDescriptor> {
        let spec = provider_spec(provider);
        let configured = config.is_some();
        let token_exists = if self.vault.available() || env_token_available(provider) {
            self.vault
                .get(&secret_key(provider, "access_token"))?
                .is_some()
        } else {
            false
        };
        let connected = configured && token_exists;
        let status = match (configured, connected, self.vault.available()) {
            (false, _, _) => ConnectorStatus::NotConfigured,
            (true, false, false) => ConnectorStatus::Degraded,
            (true, false, true) => ConnectorStatus::ReadyToConnect,
            (true, true, _) => ConnectorStatus::Connected,
        };
        let mut metadata = config
            .map(|value| value.metadata.clone())
            .unwrap_or_default();
        metadata.insert(
            "vault_backend".to_owned(),
            self.vault.backend_name().to_owned(),
        );
        metadata.insert("oauth_callback_base".to_owned(), self.callback_base.clone());
        Ok(ConnectorDescriptor {
            provider,
            display_name: spec.display_name.to_owned(),
            description: spec.description.to_owned(),
            status,
            configured,
            connected,
            account_label: config.and_then(|value| value.account_label.clone()),
            granted_scopes: config
                .map(|value| value.granted_scopes.clone())
                .unwrap_or_default(),
            token_expires_at_epoch_millis: config
                .and_then(|value| value.token_expires_at_epoch_millis),
            actions: builtin_actions(provider),
            limitations: spec
                .limitations
                .iter()
                .map(|value| (*value).to_owned())
                .collect(),
            last_error: config.and_then(|value| value.last_error.clone()),
            metadata,
        })
    }

    fn execute_inner(
        &self,
        request: &ConnectorActionRequest,
        risk: ConnectorRisk,
    ) -> Result<ConnectorActionResponse> {
        let access_token = self.fresh_access_token(request.provider)?;
        let spec = provider_spec(request.provider);
        validate_provider_url(&spec, spec.api_base)?;
        let mut warnings = spec
            .limitations
            .iter()
            .map(|value| (*value).to_owned())
            .collect::<Vec<_>>();
        let (method, url, body, external_reference) = match request.provider {
            ConnectorProvider::Gmail => {
                self.gmail_action(&request.action_id, &request.input, &access_token)?
            }
            ConnectorProvider::Spotify => {
                self.spotify_action(&request.action_id, &request.input, &access_token)?
            }
            ConnectorProvider::TikTok => {
                self.tiktok_action(&request.action_id, &request.input, &access_token)?
            }
            ConnectorProvider::Instagram => {
                self.instagram_action(&request.action_id, &request.input, &access_token)?
            }
            ConnectorProvider::GitHub => {
                self.github_action(&request.action_id, &request.input, &access_token)?
            }
            ConnectorProvider::Custom => anyhow::bail!("custom connector execution is disabled"),
        };
        if body.get("_warning").is_some() {
            warnings.push(
                body.get("_warning")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
            );
        }
        Ok(ConnectorActionResponse {
            provider: request.provider,
            action_id: request.action_id.clone(),
            status: "completed".to_owned(),
            risk,
            executed: true,
            output: json!({"method": method, "url": redact_url(&url), "data": body}),
            external_reference,
            retry_after_millis: None,
            warnings,
        })
    }

    fn gmail_action(
        &self,
        action: &str,
        input: &Value,
        token: &str,
    ) -> Result<(String, String, Value, Option<String>)> {
        let base = provider_spec(ConnectorProvider::Gmail).api_base;
        match action {
            "profile" => {
                let url = format!("{base}/users/me/profile");
                let value = self.authorized_json("GET", &url, token, None)?;
                Ok(("GET".to_owned(), url, value, None))
            }
            "unread_summary" => {
                let query = input
                    .get("query")
                    .and_then(Value::as_str)
                    .unwrap_or("is:unread in:inbox");
                let max = input
                    .get("max_results")
                    .and_then(Value::as_u64)
                    .unwrap_or(25)
                    .clamp(1, 100);
                let url = format!(
                    "{base}/users/me/messages?q={}&maxResults={max}",
                    crypto::percent_encode(query)
                );
                let value = self.authorized_json("GET", &url, token, None)?;
                Ok(("GET".to_owned(), url, value, None))
            }
            "latest_messages" => {
                let query = input
                    .get("query")
                    .and_then(Value::as_str)
                    .unwrap_or("in:inbox");
                let max = input
                    .get("max_results")
                    .and_then(Value::as_u64)
                    .unwrap_or(10)
                    .clamp(1, 20);
                let list_url = format!(
                    "{base}/users/me/messages?q={}&maxResults={max}",
                    crypto::percent_encode(query)
                );
                let list = self.authorized_json("GET", &list_url, token, None)?;
                let mut messages = Vec::new();
                for id in list
                    .get("messages")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(|message| message.get("id").and_then(Value::as_str))
                    .take(max as usize)
                {
                    let url = format!(
                        "{base}/users/me/messages/{id}?format=metadata&metadataHeaders=From&metadataHeaders=Subject&metadataHeaders=Date"
                    );
                    messages.push(self.authorized_json("GET", &url, token, None)?);
                }
                Ok((
                    "GET".to_owned(),
                    list_url,
                    json!({"query": query, "messages": messages}),
                    None,
                ))
            }
            "mark_read" | "archive" => {
                let message_id = required_identifier(input, "message_id")?;
                let url = format!("{base}/users/me/messages/{message_id}/modify");
                let label = if action == "mark_read" {
                    "UNREAD"
                } else {
                    "INBOX"
                };
                let value = self.authorized_json(
                    "POST",
                    &url,
                    token,
                    Some(json!({"removeLabelIds":[label]})),
                )?;
                Ok(("POST".to_owned(), url, value, Some(message_id.to_owned())))
            }
            "send_email" => {
                let to = required_header(input, "to")?;
                let subject = required_header(input, "subject")?;
                let cc = optional_header(input, "cc")?;
                let bcc = optional_header(input, "bcc")?;
                let body = required_string(input, "body")?;
                let reply_to_message_id = input
                    .get("reply_to_message_id")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty());
                let mut thread_id = input
                    .get("thread_id")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_owned);
                let mut in_reply_to = None;
                let mut references = None;
                if let Some(message_id) = reply_to_message_id {
                    validate_identifier(message_id, "reply_to_message_id")?;
                    let metadata_url = format!(
                        "{base}/users/me/messages/{message_id}?format=metadata&metadataHeaders=Message-ID&metadataHeaders=References"
                    );
                    let metadata = self.authorized_json("GET", &metadata_url, token, None)?;
                    thread_id = thread_id.or_else(|| {
                        metadata
                            .get("threadId")
                            .and_then(Value::as_str)
                            .map(str::to_owned)
                    });
                    in_reply_to = gmail_header(&metadata, "Message-ID").map(str::to_owned);
                    references = gmail_header(&metadata, "References")
                        .map(str::to_owned)
                        .or_else(|| in_reply_to.clone());
                }
                if let Some(value) = thread_id.as_deref() {
                    validate_identifier(value, "thread_id")?;
                }
                let raw = build_plain_text_email(
                    to,
                    cc,
                    bcc,
                    subject,
                    body,
                    in_reply_to.as_deref(),
                    references.as_deref(),
                )?;
                let url = format!("{base}/users/me/messages/send");
                let mut payload = json!({"raw": crypto::base64_url_no_pad(raw.as_bytes())});
                if let Some(thread_id) = thread_id {
                    payload["threadId"] = Value::String(thread_id);
                }
                let value = self.authorized_json("POST", &url, token, Some(payload))?;
                let message_id = value.get("id").and_then(Value::as_str).map(str::to_owned);
                Ok(("POST".to_owned(), url, value, message_id))
            }
            _ => anyhow::bail!("unknown Gmail action '{action}'"),
        }
    }

    fn spotify_action(
        &self,
        action: &str,
        input: &Value,
        token: &str,
    ) -> Result<(String, String, Value, Option<String>)> {
        let base = provider_spec(ConnectorProvider::Spotify).api_base;
        let device = input.get("device_id").and_then(Value::as_str);
        let device_query = device
            .map(|value| format!("?device_id={}", crypto::percent_encode(value)))
            .unwrap_or_default();
        match action {
            "profile" => {
                let url = format!("{base}/me");
                let value = self.authorized_json("GET", &url, token, None)?;
                Ok(("GET".to_owned(), url, value, None))
            }
            "playback" => {
                let url = format!("{base}/me/player");
                let value = self.authorized_json_allow_empty("GET", &url, token, None)?;
                Ok(("GET".to_owned(), url, value, None))
            }
            "play" => {
                let url = format!("{base}/me/player/play{device_query}");
                let body = input.get("uri").and_then(Value::as_str).map(|uri| {
                    if uri.starts_with("spotify:track:") {
                        json!({"uris":[uri]})
                    } else {
                        json!({"context_uri":uri})
                    }
                });
                let value = self.authorized_json_allow_empty("PUT", &url, token, body)?;
                Ok(("PUT".to_owned(), url, value, None))
            }
            "pause" | "next" | "previous" => {
                let endpoint = match action {
                    "pause" => "pause",
                    "next" => "next",
                    _ => "previous",
                };
                let method = if action == "pause" { "PUT" } else { "POST" };
                let url = format!("{base}/me/player/{endpoint}{device_query}");
                let value = self.authorized_json_allow_empty(method, &url, token, None)?;
                Ok((method.to_owned(), url, value, None))
            }
            "queue" => {
                let uri = required_string(input, "uri")?;
                let separator = if device.is_some() { "&" } else { "?" };
                let url = format!(
                    "{base}/me/player/queue{device_query}{separator}uri={}",
                    crypto::percent_encode(uri)
                );
                let value = self.authorized_json_allow_empty("POST", &url, token, None)?;
                Ok(("POST".to_owned(), url, value, Some(uri.to_owned())))
            }
            _ => anyhow::bail!("unknown Spotify action '{action}'"),
        }
    }

    fn tiktok_action(
        &self,
        action: &str,
        input: &Value,
        token: &str,
    ) -> Result<(String, String, Value, Option<String>)> {
        let base = provider_spec(ConnectorProvider::TikTok).api_base;
        match action {
            "profile" => {
                let url = format!(
                    "{base}/v2/user/info/?fields=open_id,union_id,avatar_url,display_name,profile_deep_link,bio_description"
                );
                let value = self.authorized_json("GET", &url, token, None)?;
                Ok(("GET".to_owned(), url, value, None))
            }
            "videos" => {
                let max = input
                    .get("max_count")
                    .and_then(Value::as_u64)
                    .unwrap_or(10)
                    .clamp(1, 20);
                let url = format!(
                    "{base}/v2/video/list/?fields=id,title,video_description,duration,cover_image_url,share_url,create_time&max_count={max}"
                );
                let value =
                    self.authorized_json("POST", &url, token, Some(json!({"max_count":max})))?;
                Ok(("POST".to_owned(), url, value, None))
            }
            "creator_info" => {
                let url = format!("{base}/v2/post/publish/creator_info/query/");
                let value = self.authorized_json("POST", &url, token, Some(json!({})))?;
                Ok(("POST".to_owned(), url, value, None))
            }
            "publish_video_url" => {
                let video_url = required_https_url(input, "video_url")?;
                let privacy = required_string(input, "privacy_level")?;
                let url = format!("{base}/v2/post/publish/video/init/");
                let payload = json!({
                    "post_info": {
                        "title": input.get("title").and_then(Value::as_str).unwrap_or(""),
                        "privacy_level": privacy,
                        "disable_comment": input.get("disable_comment").and_then(Value::as_bool).unwrap_or(false),
                        "disable_duet": input.get("disable_duet").and_then(Value::as_bool).unwrap_or(false),
                        "disable_stitch": input.get("disable_stitch").and_then(Value::as_bool).unwrap_or(false),
                        "brand_content_toggle": false,
                        "brand_organic_toggle": false,
                        "is_aigc": input.get("is_aigc").and_then(Value::as_bool).unwrap_or(false)
                    },
                    "source_info": {"source":"PULL_FROM_URL","video_url":video_url}
                });
                let value = self.authorized_json("POST", &url, token, Some(payload))?;
                let publish_id = value
                    .pointer("/data/publish_id")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
                Ok(("POST".to_owned(), url, value, publish_id))
            }
            "publish_status" => {
                let publish_id = required_string(input, "publish_id")?;
                let url = format!("{base}/v2/post/publish/status/fetch/");
                let value = self.authorized_json(
                    "POST",
                    &url,
                    token,
                    Some(json!({"publish_id":publish_id})),
                )?;
                Ok(("POST".to_owned(), url, value, Some(publish_id.to_owned())))
            }
            _ => anyhow::bail!("unknown TikTok action '{action}'"),
        }
    }

    fn instagram_action(
        &self,
        action: &str,
        input: &Value,
        token: &str,
    ) -> Result<(String, String, Value, Option<String>)> {
        let base = provider_spec(ConnectorProvider::Instagram).api_base;
        match action {
            "profile" => {
                let url = format!(
                    "{base}/me?fields=id,user_id,username,name,profile_picture_url,followers_count,follows_count,media_count&access_token={}",
                    crypto::percent_encode(token)
                );
                let value = self.public_json("GET", &url, None)?;
                Ok(("GET".to_owned(), url, value, None))
            }
            "media" => {
                let limit = input
                    .get("limit")
                    .and_then(Value::as_u64)
                    .unwrap_or(12)
                    .clamp(1, 50);
                let url = format!(
                    "{base}/me/media?fields=id,caption,media_type,media_url,permalink,thumbnail_url,timestamp&limit={limit}&access_token={}",
                    crypto::percent_encode(token)
                );
                let value = self.public_json("GET", &url, None)?;
                Ok(("GET".to_owned(), url, value, None))
            }
            "publish_image_url" => {
                let image_url = required_https_url(input, "image_url")?;
                let caption = input.get("caption").and_then(Value::as_str).unwrap_or("");
                let create_url = format!("{base}/me/media");
                let create = self.http.execute(&HttpRequest {
                    method: "POST".to_owned(),
                    url: create_url.clone(),
                    headers: Vec::new(),
                    form: vec![
                        ("image_url".to_owned(), image_url.to_owned()),
                        ("caption".to_owned(), caption.to_owned()),
                        ("access_token".to_owned(), token.to_owned()),
                    ],
                    json_body: None,
                })?;
                let container = parse_json_response(&create, "Instagram media container creation")?;
                let creation_id = container
                    .get("id")
                    .and_then(Value::as_str)
                    .ok_or_else(|| anyhow!("Instagram container response did not include id"))?;
                let publish_url = format!("{base}/me/media_publish");
                let publish = self.http.execute(&HttpRequest {
                    method: "POST".to_owned(),
                    url: publish_url.clone(),
                    headers: Vec::new(),
                    form: vec![
                        ("creation_id".to_owned(), creation_id.to_owned()),
                        ("access_token".to_owned(), token.to_owned()),
                    ],
                    json_body: None,
                })?;
                let value = parse_json_response(&publish, "Instagram media publish")?;
                let media_id = value.get("id").and_then(Value::as_str).map(str::to_owned);
                Ok((
                    "POST".to_owned(),
                    publish_url,
                    json!({"container":container,"publish":value}),
                    media_id,
                ))
            }
            _ => anyhow::bail!("unknown Instagram action '{action}'"),
        }
    }

    fn github_action(
        &self,
        action: &str,
        input: &Value,
        token: &str,
    ) -> Result<(String, String, Value, Option<String>)> {
        let base = provider_spec(ConnectorProvider::GitHub).api_base;
        let repository_path = |suffix: &str| -> Result<String> {
            let owner = required_identifier(input, "owner")?;
            let repo = required_identifier(input, "repo")?;
            Ok(format!("/repos/{owner}/{repo}{suffix}"))
        };
        let (method, path, body) = match action {
            "profile" => {
                let profile_url = format!("{base}/user");
                let profile = self.github_authorized_json("GET", &profile_url, token, None)?;
                let rate_limit_url = format!("{base}/rate_limit");
                let rate_limit =
                    self.github_authorized_json("GET", &rate_limit_url, token, None)?;
                return Ok((
                    "GET".to_owned(),
                    profile_url,
                    json!({"profile": profile, "rate_limit": rate_limit}),
                    None,
                ));
            }
            "repository" => ("GET".to_owned(), repository_path("")?, None),
            "search_code" => {
                let query = required_string(input, "query")?;
                let per_page = input
                    .get("per_page")
                    .and_then(Value::as_u64)
                    .unwrap_or(30)
                    .clamp(1, 100);
                (
                    "GET".to_owned(),
                    format!(
                        "/search/code?q={}&per_page={per_page}",
                        crypto::percent_encode(query)
                    ),
                    None,
                )
            }
            "issues" | "pull_requests" => {
                let state = input.get("state").and_then(Value::as_str).unwrap_or("open");
                anyhow::ensure!(
                    matches!(state, "open" | "closed" | "all"),
                    "state must be open, closed, or all"
                );
                let per_page = input
                    .get("per_page")
                    .and_then(Value::as_u64)
                    .unwrap_or(30)
                    .clamp(1, 100);
                let suffix = if action == "issues" {
                    "/issues"
                } else {
                    "/pulls"
                };
                (
                    "GET".to_owned(),
                    format!(
                        "{}?state={}&per_page={per_page}",
                        repository_path(suffix)?,
                        crypto::percent_encode(state)
                    ),
                    None,
                )
            }
            "notifications" => {
                let all = input.get("all").and_then(Value::as_bool).unwrap_or(false);
                let participating = input
                    .get("participating")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                let per_page = input
                    .get("per_page")
                    .and_then(Value::as_u64)
                    .unwrap_or(30)
                    .clamp(1, 100);
                (
                    "GET".to_owned(),
                    format!(
                        "/notifications?all={all}&participating={participating}&per_page={per_page}"
                    ),
                    None,
                )
            }
            "workflow_runs" => {
                let per_page = input
                    .get("per_page")
                    .and_then(Value::as_u64)
                    .unwrap_or(30)
                    .clamp(1, 100);
                (
                    "GET".to_owned(),
                    format!("{}?per_page={per_page}", repository_path("/actions/runs")?),
                    None,
                )
            }
            "branches" | "releases" => {
                let per_page = input
                    .get("per_page")
                    .and_then(Value::as_u64)
                    .unwrap_or(30)
                    .clamp(1, 100);
                let suffix = if action == "branches" {
                    "/branches"
                } else {
                    "/releases"
                };
                (
                    "GET".to_owned(),
                    format!("{}?per_page={per_page}", repository_path(suffix)?),
                    None,
                )
            }
            "commits" => {
                let per_page = input
                    .get("per_page")
                    .and_then(Value::as_u64)
                    .unwrap_or(30)
                    .clamp(1, 100);
                let mut query = vec![format!("per_page={per_page}")];
                if let Some(sha) = input
                    .get("sha")
                    .and_then(Value::as_str)
                    .filter(|value| !value.trim().is_empty())
                {
                    query.push(format!("sha={}", crypto::percent_encode(sha.trim())));
                }
                if let Some(path) = input
                    .get("path")
                    .and_then(Value::as_str)
                    .filter(|value| !value.trim().is_empty())
                {
                    query.push(format!("path={}", crypto::percent_encode(path.trim())));
                }
                (
                    "GET".to_owned(),
                    format!("{}?{}", repository_path("/commits")?, query.join("&")),
                    None,
                )
            }
            "contents" => {
                let content_path = github_repository_content_path(required_string(input, "path")?)?;
                let mut path = repository_path(&format!("/contents/{content_path}"))?;
                if let Some(reference) = input
                    .get("ref")
                    .and_then(Value::as_str)
                    .filter(|value| !value.trim().is_empty())
                {
                    path.push_str("?ref=");
                    path.push_str(&crypto::percent_encode(reference.trim()));
                }
                ("GET".to_owned(), path, None)
            }
            "security_alerts" => {
                let kind = required_string(input, "kind")?;
                let suffix = match kind {
                    "dependabot" => "/dependabot/alerts",
                    "code_scanning" => "/code-scanning/alerts",
                    "secret_scanning" => "/secret-scanning/alerts",
                    _ => anyhow::bail!(
                        "security alert kind must be dependabot, code_scanning, or secret_scanning"
                    ),
                };
                let per_page = input
                    .get("per_page")
                    .and_then(Value::as_u64)
                    .unwrap_or(30)
                    .clamp(1, 100);
                let mut query = vec![format!("per_page={per_page}")];
                if let Some(state) = input
                    .get("state")
                    .and_then(Value::as_str)
                    .filter(|value| !value.trim().is_empty())
                {
                    query.push(format!("state={}", crypto::percent_encode(state.trim())));
                }
                (
                    "GET".to_owned(),
                    format!("{}?{}", repository_path(suffix)?, query.join("&")),
                    None,
                )
            }
            "create_issue" => (
                "POST".to_owned(),
                repository_path("/issues")?,
                Some(json!({
                    "title": required_string(input, "title")?,
                    "body": input.get("body").cloned().unwrap_or(Value::Null),
                    "labels": input.get("labels").cloned().unwrap_or_else(|| json!([])),
                    "assignees": input.get("assignees").cloned().unwrap_or_else(|| json!([])),
                })),
            ),
            "comment_issue" => {
                let number = input
                    .get("number")
                    .and_then(Value::as_u64)
                    .ok_or_else(|| anyhow!("connector input requires positive 'number'"))?;
                (
                    "POST".to_owned(),
                    repository_path(&format!("/issues/{number}/comments"))?,
                    Some(json!({"body": required_string(input, "body")?})),
                )
            }
            "create_pull_request" => (
                "POST".to_owned(),
                repository_path("/pulls")?,
                Some(json!({
                    "title": required_string(input, "title")?,
                    "head": required_string(input, "head")?,
                    "base": required_string(input, "base")?,
                    "body": input.get("body").cloned().unwrap_or(Value::Null),
                    "draft": input.get("draft").and_then(Value::as_bool).unwrap_or(false),
                })),
            ),
            "merge_pull_request" => {
                let number = input
                    .get("number")
                    .and_then(Value::as_u64)
                    .ok_or_else(|| anyhow!("connector input requires positive 'number'"))?;
                let merge_method = input
                    .get("merge_method")
                    .and_then(Value::as_str)
                    .unwrap_or("merge");
                anyhow::ensure!(
                    matches!(merge_method, "merge" | "squash" | "rebase"),
                    "merge_method must be merge, squash, or rebase"
                );
                (
                    "PUT".to_owned(),
                    repository_path(&format!("/pulls/{number}/merge"))?,
                    Some(json!({
                        "commit_title": input.get("commit_title").cloned().unwrap_or(Value::Null),
                        "commit_message": input.get("commit_message").cloned().unwrap_or(Value::Null),
                        "merge_method": merge_method,
                    })),
                )
            }
            "workflow_dispatch" => {
                let workflow = required_identifier(input, "workflow")?;
                (
                    "POST".to_owned(),
                    repository_path(&format!("/actions/workflows/{workflow}/dispatches"))?,
                    Some(json!({
                        "ref": required_string(input, "ref")?,
                        "inputs": input.get("inputs").cloned().unwrap_or_else(|| json!({})),
                    })),
                )
            }
            "create_release" => (
                "POST".to_owned(),
                repository_path("/releases")?,
                Some(json!({
                    "tag_name": required_string(input, "tag_name")?,
                    "target_commitish": input.get("target_commitish").cloned().unwrap_or(Value::Null),
                    "name": input.get("name").cloned().unwrap_or(Value::Null),
                    "body": input.get("body").cloned().unwrap_or(Value::Null),
                    "draft": input.get("draft").and_then(Value::as_bool).unwrap_or(true),
                    "prerelease": input.get("prerelease").and_then(Value::as_bool).unwrap_or(false),
                    "generate_release_notes": input.get("generate_release_notes").and_then(Value::as_bool).unwrap_or(false),
                })),
            ),
            "mark_notification_read" => {
                let thread_id = required_identifier(input, "thread_id")?;
                (
                    "PATCH".to_owned(),
                    format!("/notifications/threads/{thread_id}"),
                    Some(json!({})),
                )
            }
            "rerun_workflow" | "cancel_workflow" => {
                let run_id = input
                    .get("run_id")
                    .and_then(Value::as_u64)
                    .filter(|value| *value > 0)
                    .ok_or_else(|| anyhow!("connector input requires positive 'run_id'"))?;
                let operation = if action == "rerun_workflow" {
                    "rerun"
                } else {
                    "cancel"
                };
                (
                    "POST".to_owned(),
                    repository_path(&format!("/actions/runs/{run_id}/{operation}"))?,
                    Some(json!({})),
                )
            }
            "rest_read" => {
                let path = validate_github_api_path(required_string(input, "path")?)?;
                (
                    "GET".to_owned(),
                    append_github_query(path, input.get("query"))?,
                    None,
                )
            }
            "rest_write" => {
                let method = required_string(input, "method")?.to_ascii_uppercase();
                anyhow::ensure!(
                    matches!(method.as_str(), "POST" | "PUT" | "PATCH" | "DELETE"),
                    "GitHub REST mutation method must be POST, PUT, PATCH, or DELETE"
                );
                let path = validate_github_api_path(required_string(input, "path")?)?;
                (
                    method,
                    append_github_query(path, input.get("query"))?,
                    input.get("body").cloned(),
                )
            }
            "graphql_read" | "graphql_mutation" => {
                let query = required_string(input, "query")?;
                let mutation = looks_like_graphql_mutation(query);
                anyhow::ensure!(
                    action == "graphql_mutation" || !mutation,
                    "GraphQL mutations must use the approval-gated graphql_mutation action"
                );
                anyhow::ensure!(
                    action != "graphql_mutation" || mutation,
                    "graphql_mutation requires a mutation document"
                );
                (
                    "POST".to_owned(),
                    "/graphql".to_owned(),
                    Some(json!({
                        "query": query,
                        "variables": input.get("variables").cloned().unwrap_or_else(|| json!({})),
                    })),
                )
            }
            _ => anyhow::bail!("unknown GitHub action '{action}'"),
        };
        let url = format!("{base}{path}");
        validate_provider_url(&provider_spec(ConnectorProvider::GitHub), &url)?;
        let value = self.github_authorized_json(&method, &url, token, body)?;
        let external_reference = value
            .get("html_url")
            .or_else(|| value.get("url"))
            .and_then(Value::as_str)
            .map(str::to_owned);
        Ok((method, url, value, external_reference))
    }

    fn github_authorized_json(
        &self,
        method: &str,
        url: &str,
        token: &str,
        body: Option<Value>,
    ) -> Result<Value> {
        let response = self.http.execute(&HttpRequest {
            method: method.to_owned(),
            url: url.to_owned(),
            headers: vec![
                ("Authorization".to_owned(), format!("Bearer {token}")),
                (
                    "Accept".to_owned(),
                    "application/vnd.github+json".to_owned(),
                ),
                ("X-GitHub-Api-Version".to_owned(), "2026-03-10".to_owned()),
            ],
            form: Vec::new(),
            json_body: body,
        })?;
        if (200..300).contains(&response.status) && response.body.is_empty() {
            return Ok(json!({"ok": true, "http_status": response.status}));
        }
        parse_json_response(&response, "GitHub API request")
    }

    fn authorized_json(
        &self,
        method: &str,
        url: &str,
        token: &str,
        body: Option<Value>,
    ) -> Result<Value> {
        let response = self.http.execute(&HttpRequest {
            method: method.to_owned(),
            url: url.to_owned(),
            headers: vec![("Authorization".to_owned(), format!("Bearer {token}"))],
            form: Vec::new(),
            json_body: body,
        })?;
        parse_json_response(&response, "connector API request")
    }

    fn authorized_json_allow_empty(
        &self,
        method: &str,
        url: &str,
        token: &str,
        body: Option<Value>,
    ) -> Result<Value> {
        let response = self.http.execute(&HttpRequest {
            method: method.to_owned(),
            url: url.to_owned(),
            headers: vec![("Authorization".to_owned(), format!("Bearer {token}"))],
            form: Vec::new(),
            json_body: body,
        })?;
        if (200..300).contains(&response.status) && response.body.is_empty() {
            return Ok(json!({"ok":true,"http_status":response.status}));
        }
        parse_json_response(&response, "connector API request")
    }

    fn public_json(&self, method: &str, url: &str, body: Option<Value>) -> Result<Value> {
        let response = self.http.execute(&HttpRequest {
            method: method.to_owned(),
            url: url.to_owned(),
            headers: Vec::new(),
            form: Vec::new(),
            json_body: body,
        })?;
        parse_json_response(&response, "connector API request")
    }

    fn fetch_account_label(&self, provider: ConnectorProvider) -> Result<Option<String>> {
        let response = self.execute(ConnectorActionRequest {
            provider,
            action_id: "profile".to_owned(),
            input: Value::Null,
            approval_granted: false,
            dry_run: false,
            idempotency_key: None,
        })?;
        let data = response.output.get("data").unwrap_or(&response.output);
        let label = match provider {
            ConnectorProvider::Gmail => data.get("emailAddress").and_then(Value::as_str),
            ConnectorProvider::Spotify => data
                .get("display_name")
                .and_then(Value::as_str)
                .or_else(|| data.get("id").and_then(Value::as_str)),
            ConnectorProvider::TikTok => data
                .pointer("/data/user/display_name")
                .and_then(Value::as_str)
                .or_else(|| data.pointer("/data/display_name").and_then(Value::as_str)),
            ConnectorProvider::Instagram => data
                .get("username")
                .and_then(Value::as_str)
                .or_else(|| data.get("name").and_then(Value::as_str)),
            ConnectorProvider::GitHub => data
                .pointer("/profile/login")
                .and_then(Value::as_str)
                .or_else(|| data.pointer("/profile/name").and_then(Value::as_str))
                .or_else(|| data.get("login").and_then(Value::as_str)),
            ConnectorProvider::Custom => None,
        };
        Ok(label.map(str::to_owned))
    }

    fn purge_expired_oauth_secrets(&self) -> Result<()> {
        for session in self.store.take_expired_oauth_sessions(now_millis())? {
            if provider_spec(session.provider).supports_pkce {
                let _ = self
                    .vault
                    .delete(&oauth_verifier_key(session.provider, &session.state));
            }
        }
        Ok(())
    }

    fn fresh_access_token(&self, provider: ConnectorProvider) -> Result<String> {
        let (config, current) = self.current_token_state(provider)?;
        if !token_expires_soon(&config) {
            return Ok(current);
        }

        // Token refresh is intentionally single-flight. Refreshes are rare, while a
        // global lock prevents multiple scheduler workers from rotating the same
        // refresh token concurrently and invalidating one another.
        let _guard = self
            .refresh_lock
            .lock()
            .map_err(|_| anyhow!("connector token refresh lock is poisoned"))?;
        let (config, current) = self.current_token_state(provider)?;
        if !token_expires_soon(&config) {
            return Ok(current);
        }
        if provider == ConnectorProvider::Instagram {
            anyhow::bail!("Instagram access token has expired; reconnect the account");
        }
        let refresh = self
            .vault
            .get(&secret_key(provider, "refresh_token"))?
            .ok_or_else(|| anyhow!("connector refresh token is missing; reconnect the account"))?;
        let spec = provider_spec(provider);
        let mut form = vec![
            (
                spec.client_id_parameter.to_owned(),
                config.client_id.clone(),
            ),
            ("grant_type".to_owned(), "refresh_token".to_owned()),
            ("refresh_token".to_owned(), refresh),
        ];
        if let Some(secret) = self.vault.get(&secret_key(provider, "client_secret"))? {
            form.push(("client_secret".to_owned(), secret));
        }
        let response = self.http.execute(&HttpRequest {
            method: "POST".to_owned(),
            url: spec.token_endpoint.to_owned(),
            headers: vec![
                (
                    "Content-Type".to_owned(),
                    "application/x-www-form-urlencoded".to_owned(),
                ),
                ("Accept".to_owned(), "application/json".to_owned()),
            ],
            form,
            json_body: None,
        })?;
        let body = parse_json_response(&response, "OAuth token refresh")?;
        let token = parse_token_bundle(&body)?;
        self.persist_token(provider, &token)?;
        let mut updated = config;
        updated.token_expires_at_epoch_millis = token
            .expires_in
            .map(|seconds| now_millis().saturating_add(u128::from(seconds) * 1000));
        if let Some(scope) = token.scope.as_deref() {
            updated.granted_scopes = parse_scopes(scope);
        }
        if provider == ConnectorProvider::GitHub {
            updated.granted_scopes.push("github:token".to_owned());
            updated.granted_scopes = deduplicate(updated.granted_scopes);
        }
        self.store.upsert_config(&updated)?;
        Ok(token.access_token)
    }

    fn current_token_state(
        &self,
        provider: ConnectorProvider,
    ) -> Result<(StoredConnectorConfig, String)> {
        let config = self
            .store
            .get_config(provider)?
            .ok_or_else(|| anyhow!("connector is not configured"))?;
        let current = self
            .vault
            .get(&secret_key(provider, "access_token"))?
            .ok_or_else(|| anyhow!("connector access token is missing"))?;
        Ok((config, current))
    }

    fn persist_token(&self, provider: ConnectorProvider, token: &TokenBundle) -> Result<()> {
        self.vault
            .set(&secret_key(provider, "access_token"), &token.access_token)?;
        if let Some(refresh) = token.refresh_token.as_deref() {
            self.vault
                .set(&secret_key(provider, "refresh_token"), refresh)?;
        }
        Ok(())
    }
}

fn validate_action_input(input: &Value, schema: &Value) -> Result<()> {
    let serialized_size = serde_json::to_vec(input)?.len();
    anyhow::ensure!(
        serialized_size <= 256 * 1024,
        "connector input exceeds the 256 KiB limit"
    );
    validate_schema_value(input, schema, "$", 0)
}

fn validate_schema_value(value: &Value, schema: &Value, path: &str, depth: usize) -> Result<()> {
    anyhow::ensure!(
        depth <= 12,
        "connector input nesting exceeds the supported depth"
    );
    let schema_type = schema.get("type").and_then(Value::as_str);
    match schema_type {
        Some("object") => {
            let empty = serde_json::Map::new();
            let object = if value.is_null() {
                &empty
            } else {
                value
                    .as_object()
                    .ok_or_else(|| anyhow!("connector input {path} must be an object"))?
            };
            if let Some(required) = schema.get("required").and_then(Value::as_array) {
                for key in required.iter().filter_map(Value::as_str) {
                    anyhow::ensure!(
                        object.contains_key(key),
                        "connector input {path}.{key} is required"
                    );
                }
            }
            let properties = schema.get("properties").and_then(Value::as_object);
            if schema.get("additionalProperties").and_then(Value::as_bool) == Some(false) {
                for key in object.keys() {
                    anyhow::ensure!(
                        properties.is_some_and(|items| items.contains_key(key)),
                        "connector input {path}.{key} is not supported"
                    );
                }
            }
            if let Some(properties) = properties {
                for (key, child) in object {
                    if let Some(child_schema) = properties.get(key) {
                        validate_schema_value(
                            child,
                            child_schema,
                            &format!("{path}.{key}"),
                            depth + 1,
                        )?;
                    }
                }
            }
        }
        Some("string") => {
            let text = value
                .as_str()
                .ok_or_else(|| anyhow!("connector input {path} must be a string"))?;
            let length = text.chars().count() as u64;
            if let Some(minimum) = schema.get("minLength").and_then(Value::as_u64) {
                anyhow::ensure!(length >= minimum, "connector input {path} is too short");
            }
            if let Some(maximum) = schema.get("maxLength").and_then(Value::as_u64) {
                anyhow::ensure!(length <= maximum, "connector input {path} is too long");
            }
        }
        Some("integer") => {
            let number = value
                .as_i64()
                .ok_or_else(|| anyhow!("connector input {path} must be an integer"))?;
            if let Some(minimum) = schema.get("minimum").and_then(Value::as_i64) {
                anyhow::ensure!(
                    number >= minimum,
                    "connector input {path} is below the minimum"
                );
            }
            if let Some(maximum) = schema.get("maximum").and_then(Value::as_i64) {
                anyhow::ensure!(
                    number <= maximum,
                    "connector input {path} exceeds the maximum"
                );
            }
        }
        Some("boolean") => {
            anyhow::ensure!(
                value.is_boolean(),
                "connector input {path} must be a boolean"
            );
        }
        Some("array") => {
            let values = value
                .as_array()
                .ok_or_else(|| anyhow!("connector input {path} must be an array"))?;
            if let Some(maximum) = schema.get("maxItems").and_then(Value::as_u64) {
                anyhow::ensure!(
                    values.len() as u64 <= maximum,
                    "connector input {path} has too many items"
                );
            }
            if let Some(item_schema) = schema.get("items") {
                for (index, child) in values.iter().enumerate() {
                    validate_schema_value(
                        child,
                        item_schema,
                        &format!("{path}[{index}]"),
                        depth + 1,
                    )?;
                }
            }
        }
        Some(other) => anyhow::bail!("unsupported connector schema type '{other}'"),
        None => {}
    }
    Ok(())
}

fn validate_idempotency_key(key: Option<&str>) -> Result<()> {
    let Some(key) = key else { return Ok(()) };
    anyhow::ensure!(
        (8..=256).contains(&key.len()),
        "idempotency key must be 8 to 256 bytes"
    );
    anyhow::ensure!(
        key.bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':')),
        "idempotency key contains unsupported characters"
    );
    Ok(())
}

fn validate_identifier(value: &str, field: &str) -> Result<()> {
    anyhow::ensure!(
        (1..=256).contains(&value.len()),
        "'{field}' has an invalid length"
    );
    anyhow::ensure!(
        value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_')),
        "'{field}' contains unsupported characters"
    );
    Ok(())
}

fn required_identifier<'a>(value: &'a Value, key: &str) -> Result<&'a str> {
    let identifier = required_string(value, key)?;
    validate_identifier(identifier, key)?;
    Ok(identifier)
}

fn required_header<'a>(value: &'a Value, key: &str) -> Result<&'a str> {
    let header = required_string(value, key)?;
    validate_header_value(header, key)?;
    Ok(header)
}

fn optional_header<'a>(value: &'a Value, key: &str) -> Result<Option<&'a str>> {
    let header = value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty());
    if let Some(header) = header {
        validate_header_value(header, key)?;
    }
    Ok(header)
}

fn validate_header_value(value: &str, field: &str) -> Result<()> {
    anyhow::ensure!(
        !contains_line_break(value),
        "'{field}' contains a line break"
    );
    anyhow::ensure!(!value.contains('\0'), "'{field}' contains a null byte");
    Ok(())
}

fn build_plain_text_email(
    to: &str,
    cc: Option<&str>,
    bcc: Option<&str>,
    subject: &str,
    body: &str,
    in_reply_to: Option<&str>,
    references: Option<&str>,
) -> Result<String> {
    for (name, value) in [
        ("to", Some(to)),
        ("cc", cc),
        ("bcc", bcc),
        ("subject", Some(subject)),
        ("in_reply_to", in_reply_to),
        ("references", references),
    ] {
        if let Some(value) = value {
            validate_header_value(value, name)?;
        }
    }
    anyhow::ensure!(body.len() <= 200_000, "email body exceeds 200000 bytes");
    let mut message = String::new();
    message.push_str("To: ");
    message.push_str(to);
    message.push_str("\r\n");
    if let Some(cc) = cc {
        message.push_str("Cc: ");
        message.push_str(cc);
        message.push_str("\r\n");
    }
    if let Some(bcc) = bcc {
        message.push_str("Bcc: ");
        message.push_str(bcc);
        message.push_str("\r\n");
    }
    message.push_str("Subject: ");
    message.push_str(subject);
    message.push_str("\r\n");
    if let Some(in_reply_to) = in_reply_to {
        message.push_str("In-Reply-To: ");
        message.push_str(in_reply_to);
        message.push_str("\r\n");
    }
    if let Some(references) = references {
        message.push_str("References: ");
        message.push_str(references);
        message.push_str("\r\n");
    }
    message.push_str("MIME-Version: 1.0\r\n");
    message.push_str("Content-Type: text/plain; charset=UTF-8\r\n");
    message.push_str("Content-Transfer-Encoding: 8bit\r\n\r\n");
    message.push_str(
        &body
            .replace("\r\n", "\n")
            .replace('\r', "\n")
            .replace('\n', "\r\n"),
    );
    Ok(message)
}

fn gmail_header<'a>(message: &'a Value, name: &str) -> Option<&'a str> {
    message
        .pointer("/payload/headers")
        .and_then(Value::as_array)?
        .iter()
        .find(|header| {
            header
                .get("name")
                .and_then(Value::as_str)
                .is_some_and(|value| value.eq_ignore_ascii_case(name))
        })
        .and_then(|header| header.get("value"))
        .and_then(Value::as_str)
}

fn token_expires_soon(config: &StoredConnectorConfig) -> bool {
    config
        .token_expires_at_epoch_millis
        .is_some_and(|expiry| expiry <= now_millis().saturating_add(90_000))
}

fn oauth_verifier_key(provider: ConnectorProvider, state: &str) -> String {
    format!("oauth_pkce:{}:{state}", provider.as_str())
}

fn parse_token_bundle(value: &Value) -> Result<TokenBundle> {
    if value.get("access_token").is_none() {
        if let Some(error) = value.get("error").and_then(Value::as_str) {
            anyhow::bail!(
                "OAuth token endpoint returned {error}: {}",
                value
                    .get("error_description")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
            );
        }
    }
    serde_json::from_value(value.clone()).context("parse OAuth token response")
}

fn parse_json_response(response: &http::HttpResponse, operation: &str) -> Result<Value> {
    let value = if response.body.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice::<Value>(&response.body).with_context(|| {
            format!(
                "{operation} returned non-JSON data (HTTP {})",
                response.status
            )
        })?
    };
    anyhow::ensure!(
        (200..300).contains(&response.status),
        "{operation} failed with HTTP {}: {}",
        response.status,
        sanitized_error(&value)
    );
    anyhow::ensure!(
        !response.truncated,
        "{operation} response exceeded the configured size limit"
    );
    Ok(value)
}

fn sanitized_error(value: &Value) -> String {
    let text = value
        .get("error_description")
        .and_then(Value::as_str)
        .or_else(|| value.get("message").and_then(Value::as_str))
        .or_else(|| value.pointer("/error/message").and_then(Value::as_str))
        .unwrap_or("provider rejected the request");
    text.chars().take(512).collect()
}

fn github_repository_content_path(path: &str) -> Result<String> {
    let path = path.trim().trim_matches('/');
    anyhow::ensure!(
        !path.is_empty(),
        "GitHub repository content path must not be empty"
    );
    anyhow::ensure!(
        path.len() <= 4_096,
        "GitHub repository content path exceeds 4096 bytes"
    );
    anyhow::ensure!(
        !path.contains("..")
            && !path.contains('\\')
            && !contains_line_break(path)
            && !path.contains('\0'),
        "GitHub repository content path is unsafe"
    );
    Ok(path
        .split('/')
        .map(crypto::percent_encode)
        .collect::<Vec<_>>()
        .join("/"))
}

fn validate_github_api_path(path: &str) -> Result<String> {
    let path = path.trim();
    anyhow::ensure!(path.starts_with('/'), "GitHub API path must start with '/'");
    anyhow::ensure!(path.len() <= 4_096, "GitHub API path exceeds 4096 bytes");
    anyhow::ensure!(
        !path.contains("//") && !path.contains("..") && !path.contains('\\'),
        "GitHub API path contains an unsafe segment"
    );
    anyhow::ensure!(
        !contains_line_break(path) && !path.contains('\0'),
        "GitHub API path contains invalid characters"
    );
    anyhow::ensure!(
        !path.starts_with("/user/installations/") || !path.contains("access_tokens"),
        "GitHub App installation token minting is not exposed through the generic connector action"
    );
    Ok(path.to_owned())
}

fn append_github_query(mut path: String, query: Option<&Value>) -> Result<String> {
    let Some(query) = query else { return Ok(path) };
    let object = query
        .as_object()
        .ok_or_else(|| anyhow!("GitHub query must be an object"))?;
    let mut entries = object.iter().collect::<Vec<_>>();
    entries.sort_by(|left, right| left.0.cmp(right.0));
    for (index, (key, value)) in entries.into_iter().enumerate() {
        anyhow::ensure!(
            key.bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-')),
            "GitHub query key contains unsupported characters"
        );
        let value = match value {
            Value::String(value) => value.clone(),
            Value::Bool(value) => value.to_string(),
            Value::Number(value) => value.to_string(),
            Value::Null => continue,
            _ => anyhow::bail!("GitHub query values must be strings, numbers, booleans, or null"),
        };
        path.push(if index == 0 && !path.contains('?') {
            '?'
        } else {
            '&'
        });
        path.push_str(&crypto::percent_encode(key));
        path.push('=');
        path.push_str(&crypto::percent_encode(&value));
    }
    Ok(path)
}

fn looks_like_graphql_mutation(document: &str) -> bool {
    let without_comments = document
        .lines()
        .map(|line| line.split('#').next().unwrap_or_default())
        .collect::<Vec<_>>()
        .join(" ");
    without_comments.trim_start().starts_with("mutation")
}

fn required_string<'a>(value: &'a Value, key: &str) -> Result<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .ok_or_else(|| anyhow!("connector input requires non-empty '{key}'"))
}

fn required_https_url<'a>(value: &'a Value, key: &str) -> Result<&'a str> {
    let url = required_string(value, key)?;
    anyhow::ensure!(url.starts_with("https://"), "'{key}' must use HTTPS");
    anyhow::ensure!(
        !contains_line_break(url),
        "'{key}' contains invalid characters"
    );
    Ok(url)
}

fn validate_redirect_uri(uri: &str) -> Result<()> {
    anyhow::ensure!(
        uri.starts_with("http://127.0.0.1:")
            || uri.starts_with("http://localhost:")
            || uri.starts_with("https://"),
        "redirect_uri must be HTTPS or an explicit loopback URI"
    );
    anyhow::ensure!(
        !contains_line_break(uri) && !uri.contains('#'),
        "redirect_uri contains invalid characters"
    );
    Ok(())
}

fn contains_line_break(value: &str) -> bool {
    value.contains('\r') || value.contains('\n')
}

fn env_token_available(provider: ConnectorProvider) -> bool {
    let key = secret_key(provider, "access_token")
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_uppercase()
            } else {
                '_'
            }
        })
        .collect::<String>();
    std::env::var_os(format!("EVERYTHING_SECRET_{key}")).is_some()
}

fn validate_provider_url(spec: &providers::ProviderSpec, url: &str) -> Result<()> {
    anyhow::ensure!(
        url.starts_with("https://"),
        "provider endpoint must use HTTPS"
    );
    anyhow::ensure!(
        !contains_line_break(url) && !url.contains('\0'),
        "provider endpoint contains invalid characters"
    );
    let authority = url
        .strip_prefix("https://")
        .and_then(|rest| rest.split(['/', '?', '#']).next())
        .ok_or_else(|| anyhow!("provider endpoint is invalid"))?;
    anyhow::ensure!(
        !authority.is_empty(),
        "provider endpoint authority is empty"
    );
    anyhow::ensure!(
        !authority.contains('@'),
        "provider endpoint must not contain user information"
    );
    let host = authority
        .split(':')
        .next()
        .unwrap_or_default()
        .trim_end_matches('.')
        .to_ascii_lowercase();
    anyhow::ensure!(
        spec.allowed_hosts
            .iter()
            .any(|allowed| allowed.eq_ignore_ascii_case(&host)),
        "provider endpoint host '{host}' is not allowlisted for {}",
        spec.display_name
    );
    Ok(())
}

fn parse_scopes(value: &str) -> Vec<String> {
    deduplicate(
        value
            .split([',', ' '])
            .map(str::trim)
            .filter(|scope| !scope.is_empty())
            .map(str::to_owned)
            .collect(),
    )
}

fn deduplicate(values: Vec<String>) -> Vec<String> {
    values
        .into_iter()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn secret_key(provider: ConnectorProvider, name: &str) -> String {
    format!("{}:{name}", provider.as_str())
}

fn classify_error(error: &anyhow::Error) -> String {
    let text = error.to_string().to_ascii_lowercase();
    if text.contains("scope") {
        "scope_missing".to_owned()
    } else if text.contains("token") || text.contains("oauth") {
        "authorization_failed".to_owned()
    } else if text.contains("http 429") || text.contains("rate") {
        "rate_limited".to_owned()
    } else if text.contains("timeout") {
        "network_timeout".to_owned()
    } else {
        "connector_failed".to_owned()
    }
}

fn redact_url(url: &str) -> String {
    if let Some(position) = url.find("access_token=") {
        let prefix = &url[..position + "access_token=".len()];
        return format!("{prefix}<redacted>");
    }
    url.to_owned()
}

fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

#[cfg(test)]
mod tests {
    use super::{
        append_github_query, build_plain_text_email, deduplicate, github_repository_content_path,
        looks_like_graphql_mutation, parse_scopes, validate_action_input, validate_github_api_path,
        validate_idempotency_key, validate_provider_url, validate_redirect_uri,
    };
    use crate::providers::provider_spec;
    use everything_domain::ConnectorProvider;

    #[test]
    fn scopes_are_normalized() {
        assert_eq!(parse_scopes("b,a a"), vec!["a", "b"]);
        assert_eq!(deduplicate(vec!["x".into(), "x".into()]), vec!["x"]);
    }

    #[test]
    fn redirect_uri_rejects_unsafe_schemes() {
        assert!(validate_redirect_uri("http://127.0.0.1:43821/callback").is_ok());
        assert!(validate_redirect_uri("file:///tmp/token").is_err());
    }

    #[test]
    fn connector_input_schema_rejects_unknown_and_invalid_fields() {
        let schema = serde_json::json!({
            "type":"object",
            "required":["count"],
            "properties":{"count":{"type":"integer","minimum":1,"maximum":3}},
            "additionalProperties":false
        });
        assert!(validate_action_input(&serde_json::json!({"count":2}), &schema).is_ok());
        assert!(validate_action_input(&serde_json::json!({"count":0}), &schema).is_err());
        assert!(
            validate_action_input(&serde_json::json!({"count":2,"extra":true}), &schema).is_err()
        );
    }

    #[test]
    fn email_builder_blocks_header_injection_and_uses_crlf() {
        assert!(
            build_plain_text_email(
                "person@example.com",
                None,
                None,
                "Hello",
                "first\nsecond",
                None,
                None,
            )
            .expect("email")
            .contains("first\r\nsecond")
        );
        assert!(
            build_plain_text_email(
                "person@example.com\r\nBcc: attacker@example.com",
                None,
                None,
                "Hello",
                "body",
                None,
                None,
            )
            .is_err()
        );
    }

    #[test]
    fn idempotency_keys_are_bounded_and_safe() {
        assert!(validate_idempotency_key(Some("desktop:gmail:send:123")).is_ok());
        assert!(validate_idempotency_key(Some("short")).is_err());
        assert!(validate_idempotency_key(Some("bad key")).is_err());
    }

    #[test]
    fn github_paths_and_queries_are_bounded_and_canonical() {
        assert_eq!(
            github_repository_content_path("docs/hello world.md").expect("content path"),
            "docs/hello%20world.md"
        );
        assert!(github_repository_content_path("../secret").is_err());
        assert!(validate_github_api_path("/repos/example/project/issues").is_ok());
        assert!(validate_github_api_path("https://evil.invalid/").is_err());
        assert!(validate_github_api_path("/user/installations/1/access_tokens").is_err());
        assert_eq!(
            append_github_query(
                "/user/repos".to_owned(),
                Some(&serde_json::json!({"per_page": 50, "affiliation": "owner"})),
            )
            .expect("query"),
            "/user/repos?affiliation=owner&per_page=50"
        );
    }

    #[test]
    fn github_graphql_mutations_are_detected_after_comments() {
        assert!(!looks_like_graphql_mutation(
            "query Viewer { viewer { login } }"
        ));
        assert!(looks_like_graphql_mutation(
            "# operation\n mutation Update { __typename }"
        ));
    }

    #[test]
    fn provider_urls_use_exact_allowlisted_hosts_without_credentials() {
        let spotify = provider_spec(ConnectorProvider::Spotify);
        assert!(validate_provider_url(&spotify, "https://api.spotify.com/v1/me").is_ok());
        assert!(validate_provider_url(&spotify, "https://API.SPOTIFY.COM/v1/me").is_ok());
        assert!(
            validate_provider_url(&spotify, "https://api.spotify.com.evil.invalid/v1/me").is_err()
        );
        assert!(
            validate_provider_url(&spotify, "https://api.spotify.com@evil.invalid/v1/me").is_err()
        );
        assert!(validate_provider_url(&spotify, "https://user@api.spotify.com/v1/me").is_err());
        assert!(validate_provider_url(&spotify, "http://api.spotify.com/v1/me").is_err());
    }
}
