//! The mihomo controller API (ClashBar's `MihomoAPIService` endpoints),
//! over the engine's network stack.

use nsh_core::platform::HttpRequest;
use nsh_core::prelude::fetch;
use serde::de::DeserializeOwned;
use serde_json::Value;

/// Where the API is and how to authenticate to it.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct ApiTarget {
    /// `host:port`, or a full `http(s)://` base.
    pub controller: String,
    pub secret: Option<String>,
}

impl ApiTarget {
    pub fn base_url(&self) -> String {
        let c = self.controller.trim_end_matches('/');
        if c.starts_with("http://") || c.starts_with("https://") {
            c.to_owned()
        } else {
            format!("http://{c}")
        }
    }

    /// The `ws://` URL of a stream path, the secret as its `token`.
    pub fn ws_url(&self, path: &str, query: &[(&str, String)]) -> String {
        let base = self.base_url();
        let base = base
            .strip_prefix("https://")
            .map(|r| format!("wss://{r}"))
            .or_else(|| base.strip_prefix("http://").map(|r| format!("ws://{r}")))
            .unwrap_or(base);
        let mut params: Vec<(String, String)> =
            query.iter().map(|(k, v)| ((*k).to_owned(), v.clone())).collect();
        if let Some(secret) = self.secret.as_ref().filter(|s| !s.is_empty()) {
            params.push(("token".into(), secret.clone()));
        }
        format!("{base}{path}{}", encode_query(&params))
    }
}

pub fn encode_query(params: &[(String, String)]) -> String {
    if params.is_empty() {
        return String::new();
    }
    let joined: Vec<String> = params.iter().map(|(k, v)| format!("{}={}", escape(k), escape(v))).collect();
    format!("?{}", joined.join("&"))
}

/// Percent-encodes everything but unreserved characters (a path segment
/// or a query value).
pub fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[derive(Debug)]
pub enum ApiError {
    Network(String),
    Status(u16, String),
    Decode(String),
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Network(m) => f.write_str(m),
            Self::Status(code, body) if body.is_empty() => write!(f, "HTTP {code}"),
            Self::Status(code, body) => write!(f, "HTTP {code}: {body}"),
            Self::Decode(m) => write!(f, "decode: {m}"),
        }
    }
}

/// A request to the API.
pub enum Endpoint {
    Version,
    GetConfigs,
    PatchConfigs(Value),
    PutConfigs { force: bool, path: String, payload: Option<String> },
    Proxies,
    SwitchProxy { name: String, target: String },
    ProxyDelay { name: String, url: String, timeout: u32 },
    GroupDelay { name: String, url: String, timeout: u32 },
    ProxyProviders,
    UpdateProxyProvider(String),
    ProxyProviderHealthcheck { name: String, url: String, timeout: u32 },
    Rules,
    RuleProviders,
    UpdateRuleProvider(String),
    Connections,
    CloseAllConnections,
    CloseConnection(String),
    FlushFakeIpCache,
    FlushDnsCache,
    UpgradeCore,
    UpgradeGeo,
}

impl Endpoint {
    fn method(&self) -> &'static str {
        match self {
            Self::PatchConfigs(_) => "PATCH",
            Self::PutConfigs { .. } | Self::SwitchProxy { .. } | Self::UpdateProxyProvider(_) | Self::UpdateRuleProvider(_) => "PUT",
            Self::CloseAllConnections | Self::CloseConnection(_) => "DELETE",
            Self::FlushFakeIpCache | Self::FlushDnsCache | Self::UpgradeCore | Self::UpgradeGeo => "POST",
            _ => "GET",
        }
    }

    fn path(&self) -> String {
        match self {
            Self::Version => "/version".into(),
            Self::GetConfigs | Self::PatchConfigs(_) | Self::PutConfigs { .. } => "/configs".into(),
            Self::Proxies => "/proxies".into(),
            Self::SwitchProxy { name, .. } => format!("/proxies/{}", escape(name)),
            Self::ProxyDelay { name, .. } => format!("/proxies/{}/delay", escape(name)),
            Self::GroupDelay { name, .. } => format!("/group/{}/delay", escape(name)),
            Self::ProxyProviders => "/providers/proxies".into(),
            Self::UpdateProxyProvider(name) => format!("/providers/proxies/{}", escape(name)),
            Self::ProxyProviderHealthcheck { name, .. } => format!("/providers/proxies/{}/healthcheck", escape(name)),
            Self::Rules => "/rules".into(),
            Self::RuleProviders => "/providers/rules".into(),
            Self::UpdateRuleProvider(name) => format!("/providers/rules/{}", escape(name)),
            Self::Connections | Self::CloseAllConnections => "/connections".into(),
            Self::CloseConnection(id) => format!("/connections/{}", escape(id)),
            Self::FlushFakeIpCache => "/cache/fakeip/flush".into(),
            Self::FlushDnsCache => "/cache/dns/flush".into(),
            Self::UpgradeCore => "/upgrade".into(),
            Self::UpgradeGeo => "/upgrade/geo".into(),
        }
    }

    fn query(&self) -> Vec<(String, String)> {
        match self {
            Self::PutConfigs { force, .. } => vec![("force".into(), force.to_string())],
            Self::ProxyDelay { url, timeout, .. }
            | Self::GroupDelay { url, timeout, .. }
            | Self::ProxyProviderHealthcheck { url, timeout, .. } => {
                vec![("url".into(), url.clone()), ("timeout".into(), timeout.to_string())]
            }
            _ => Vec::new(),
        }
    }

    fn body(&self) -> Option<Value> {
        match self {
            Self::PatchConfigs(body) => Some(body.clone()),
            Self::PutConfigs { path, payload, .. } => {
                Some(serde_json::json!({ "path": path, "payload": payload.clone().unwrap_or_default() }))
            }
            Self::SwitchProxy { target, .. } => Some(serde_json::json!({ "name": target })),
            Self::UpgradeCore | Self::UpgradeGeo => Some(serde_json::json!({})),
            _ => None,
        }
    }
}

/// Sends `endpoint` and returns the response body.
pub async fn request_raw(target: &ApiTarget, endpoint: &Endpoint) -> Result<Vec<u8>, ApiError> {
    let url = format!("{}{}{}", target.base_url(), endpoint.path(), encode_query(&endpoint.query()));
    let mut request = HttpRequest::new(endpoint.method(), url);
    if let Some(secret) = target.secret.as_ref().filter(|s| !s.is_empty()) {
        request = request.header("Authorization", format!("Bearer {secret}"));
    }
    if let Some(body) = endpoint.body() {
        request = request
            .header("Content-Type", "application/json")
            .body(serde_json::to_vec(&body).unwrap_or_default());
    }
    let response = fetch(request).await.map_err(ApiError::Network)?;
    if !response.ok() {
        return Err(ApiError::Status(response.status(), response.text()));
    }
    Ok(response.body)
}

pub async fn request<T: DeserializeOwned>(target: &ApiTarget, endpoint: &Endpoint) -> Result<T, ApiError> {
    let body = request_raw(target, endpoint).await?;
    serde_json::from_slice(&body).map_err(|e| ApiError::Decode(e.to_string()))
}

pub async fn request_no_response(target: &ApiTarget, endpoint: &Endpoint) -> Result<(), ApiError> {
    request_raw(target, endpoint).await.map(|_| ())
}
