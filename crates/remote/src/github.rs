use serde::Deserialize;

use ferry_core::ports::{PublishedRef, RemoteError, RemoteFetch, RemoteLocator, SnippetPublisher};

use crate::http::{HttpClient, HttpError};

const USER_AGENT: &str = "ferry";

#[derive(Debug, Clone)]
pub struct GithubEndpoints {
    pub api: String,
    pub device_code: String,
    pub token: String,
}

impl Default for GithubEndpoints {
    fn default() -> Self {
        Self {
            api: "https://api.github.com".into(),
            device_code: "https://github.com/login/device/code".into(),
            token: "https://github.com/login/oauth/access_token".into(),
        }
    }
}

impl GithubEndpoints {
    pub fn with_base(base: &str) -> Self {
        Self {
            api: base.trim_end_matches('/').to_string(),
            device_code: format!("{}/login/device/code", base.trim_end_matches('/')),
            token: format!("{}/login/oauth/access_token", base.trim_end_matches('/')),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum GithubError {
    #[error(transparent)]
    Http(#[from] HttpError),
    #[error("unexpected response shape: {0}")]
    Shape(String),
    #[error("device authorization is still pending")]
    AuthorizationPending,
    #[error("polling too fast; slow down")]
    SlowDown,
    #[error("device authorization failed: {0}")]
    AuthFailed(String),
}

#[derive(Debug, Clone, Deserialize)]
pub struct DeviceCode {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub expires_in: u64,
    pub interval: u64,
}

pub fn begin_device_flow(
    http: &HttpClient,
    endpoints: &GithubEndpoints,
    client_id: &str,
    scope: &str,
) -> Result<DeviceCode, GithubError> {
    let body = http.post_form(
        &endpoints.device_code,
        &[("accept", "application/json"), ("user-agent", USER_AGENT)],
        &[("client_id", client_id), ("scope", scope)],
    )?;
    serde_json::from_str(&body).map_err(|e| GithubError::Shape(e.to_string()))
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: Option<String>,
    error: Option<String>,
}

pub fn poll_device_flow(
    http: &HttpClient,
    endpoints: &GithubEndpoints,
    client_id: &str,
    device_code: &str,
) -> Result<String, GithubError> {
    let body = http.post_form(
        &endpoints.token,
        &[("accept", "application/json"), ("user-agent", USER_AGENT)],
        &[
            ("client_id", client_id),
            ("device_code", device_code),
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
        ],
    )?;
    let parsed: TokenResponse = serde_json::from_str(&body).map_err(|e| GithubError::Shape(e.to_string()))?;
    if let Some(token) = parsed.access_token {
        return Ok(token);
    }
    match parsed.error.as_deref() {
        Some("authorization_pending") => Err(GithubError::AuthorizationPending),
        Some("slow_down") => Err(GithubError::SlowDown),
        Some(other) => Err(GithubError::AuthFailed(other.to_string())),
        None => Err(GithubError::Shape("no token and no error".into())),
    }
}

#[derive(Debug, Deserialize)]
pub struct GithubLogin {
    pub login: String,
}

pub fn verify_token(
    http: &HttpClient,
    endpoints: &GithubEndpoints,
    token: &str,
) -> Result<GithubLogin, GithubError> {
    let owned = auth_headers(token);
    let body = http.get(&format!("{}/user", endpoints.api), &header_refs(&owned))?;
    serde_json::from_str(&body).map_err(|e| GithubError::Shape(e.to_string()))
}

fn auth_headers(token: &str) -> Vec<(&'static str, String)> {
    vec![
        ("authorization", format!("Bearer {token}")),
        ("accept", "application/vnd.github+json".to_string()),
        ("user-agent", USER_AGENT.to_string()),
        ("x-github-api-version", "2022-11-28".to_string()),
    ]
}

fn header_refs<'a>(owned: &'a [(&'static str, String)]) -> Vec<(&'static str, &'a str)> {
    owned.iter().map(|(k, v)| (*k, v.as_str())).collect()
}

pub struct GithubClient {
    http: HttpClient,
    endpoints: GithubEndpoints,
    token: Option<String>,
}

impl GithubClient {
    pub fn new(token: Option<String>) -> Self {
        Self {
            http: HttpClient::new(),
            endpoints: GithubEndpoints::default(),
            token,
        }
    }

    pub fn with_endpoints(http: HttpClient, endpoints: GithubEndpoints, token: Option<String>) -> Self {
        Self { http, endpoints, token }
    }
}

impl RemoteFetch for GithubClient {
    fn fetch(&self, locator: &RemoteLocator) -> Result<Vec<u8>, RemoteError> {
        let reference = locator.reference.as_deref().unwrap_or("HEAD");
        let mut parts = locator.path.splitn(3, '/');
        let owner = parts.next().unwrap_or_default();
        let repo = parts.next().unwrap_or_default();
        let path = parts.next().unwrap_or_default();
        if owner.is_empty() || repo.is_empty() || path.is_empty() {
            return Err(RemoteError(
                "locator path must be <owner>/<repo>/<path-in-repo>".into(),
            ));
        }
        let url = format!(
            "{}/repos/{owner}/{repo}/contents/{path}?ref={reference}",
            self.endpoints.api
        );
        let owned = self.token.as_deref().map(auth_headers).unwrap_or_else(|| {
            vec![
                ("user-agent", USER_AGENT.to_string()),
                ("accept", "application/vnd.github.raw+json".to_string()),
            ]
        });
        let mut headers = header_refs(&owned);
        headers.push(("accept", "application/vnd.github.raw"));
        let body = self.http.get(&url, &headers).map_err(|e| RemoteError(e.to_string()))?;
        Ok(body.into_bytes())
    }
}

#[derive(serde::Serialize)]
struct GistFile<'a> {
    content: &'a str,
}

#[derive(serde::Serialize)]
struct CreateGist<'a> {
    description: &'a str,
    public: bool,
    files: std::collections::BTreeMap<String, GistFile<'a>>,
}

#[derive(Deserialize)]
struct GistResponse {
    id: String,
    html_url: String,
}

impl SnippetPublisher for GithubClient {
    fn publish_private(&self, name: &str, bytes: &[u8]) -> Result<PublishedRef, RemoteError> {
        let token = self
            .token
            .as_deref()
            .ok_or_else(|| RemoteError("publishing a gist requires an authenticated GitHub connection".into()))?;
        let content = base64_encode(bytes);
        let mut files = std::collections::BTreeMap::new();
        files.insert(name.to_string(), GistFile { content: &content });
        let payload = CreateGist {
            description: "Ferry sealed item — encrypted, single recipient",
            public: false,
            files,
        };
        let json = serde_json::to_string(&payload).map_err(|e| RemoteError(e.to_string()))?;
        let owned = auth_headers(token);
        let headers = header_refs(&owned);
        let body = self
            .http
            .post_json(&format!("{}/gists", self.endpoints.api), &headers, &json)
            .map_err(|e| RemoteError(e.to_string()))?;
        let resp: GistResponse = serde_json::from_str(&body).map_err(|e| RemoteError(e.to_string()))?;
        Ok(PublishedRef {
            url: resp.html_url,
            id: resp.id,
        })
    }
}

fn base64_encode(bytes: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;
    use std::sync::mpsc;

    struct FakeGithub {
        base: String,
        handle: Option<std::thread::JoinHandle<()>>,
        shutdown: mpsc::Sender<()>,
    }

    impl FakeGithub {
        fn start(mut responder: impl FnMut(&str, &str) -> (u16, String) + Send + 'static) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            listener.set_nonblocking(false).unwrap();
            let addr = listener.local_addr().unwrap();
            let base = format!("http://{addr}");
            let (tx, rx) = mpsc::channel::<()>();
            let handle = std::thread::spawn(move || {
                listener.set_nonblocking(true).unwrap();
                loop {
                    if rx.try_recv().is_ok() {
                        break;
                    }
                    match listener.accept() {
                        Ok((mut stream, _)) => {
                            stream.set_nonblocking(false).unwrap();
                            let mut reader = BufReader::new(stream.try_clone().unwrap());
                            let mut request_line = String::new();
                            reader.read_line(&mut request_line).unwrap();
                            let mut method_path = request_line.split_whitespace();
                            let method = method_path.next().unwrap_or("").to_string();
                            let path = method_path.next().unwrap_or("").to_string();
                            let mut content_length = 0usize;
                            loop {
                                let mut line = String::new();
                                reader.read_line(&mut line).unwrap();
                                if line == "\r\n" || line.is_empty() {
                                    break;
                                }
                                if let Some(v) = line.to_lowercase().strip_prefix("content-length:") {
                                    content_length = v.trim().parse().unwrap_or(0);
                                }
                            }
                            let mut body = vec![0u8; content_length];
                            if content_length > 0 {
                                reader.read_exact(&mut body).unwrap();
                            }
                            let (status, payload) = responder(&format!("{method} {path}"), &String::from_utf8_lossy(&body));
                            let response = format!(
                                "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
                                payload.len()
                            );
                            stream.write_all(response.as_bytes()).unwrap();
                            stream.flush().unwrap();
                        }
                        Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            std::thread::sleep(std::time::Duration::from_millis(10));
                        }
                        Err(e) => panic!("accept failed: {e}"),
                    }
                }
            });
            Self {
                base,
                handle: Some(handle),
                shutdown: tx,
            }
        }
    }

    impl Drop for FakeGithub {
        fn drop(&mut self) {
            let _ = self.shutdown.send(());
            if let Some(h) = self.handle.take() {
                let _ = h.join();
            }
        }
    }

    #[test]
    fn device_flow_walks_pending_then_slow_down_then_issues_a_token() {
        let poll_count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let pc = poll_count.clone();
        let fake = FakeGithub::start(move |req, _body| {
            if req.starts_with("POST /login/device/code") {
                return (
                    200,
                    r#"{"device_code":"DC1","user_code":"WDJB-MJHT","verification_uri":"https://github.com/login/device","expires_in":900,"interval":1}"#.to_string(),
                );
            }
            if req.starts_with("POST /login/oauth/access_token") {
                let n = pc.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                return match n {
                    0 => (200, r#"{"error":"authorization_pending"}"#.to_string()),
                    1 => (200, r#"{"error":"slow_down"}"#.to_string()),
                    _ => (200, r#"{"access_token":"gho_realtoken"}"#.to_string()),
                };
            }
            (404, "{}".to_string())
        });

        let http = HttpClient::new();
        let endpoints = GithubEndpoints::with_base(&fake.base);

        let device = begin_device_flow(&http, &endpoints, "client-id", "gist").unwrap();
        assert_eq!(device.user_code, "WDJB-MJHT");
        assert_eq!(device.device_code, "DC1");

        assert!(matches!(
            poll_device_flow(&http, &endpoints, "client-id", &device.device_code),
            Err(GithubError::AuthorizationPending)
        ));
        assert!(matches!(
            poll_device_flow(&http, &endpoints, "client-id", &device.device_code),
            Err(GithubError::SlowDown)
        ));
        assert_eq!(
            poll_device_flow(&http, &endpoints, "client-id", &device.device_code).unwrap(),
            "gho_realtoken"
        );
        assert_eq!(poll_count.load(std::sync::atomic::Ordering::SeqCst), 3);
    }

    #[test]
    fn verify_token_reads_the_login_and_a_denied_token_is_an_error() {
        let fake = FakeGithub::start(|req, _| {
            if req.starts_with("GET /user") {
                (200, r#"{"login":"octocat"}"#.to_string())
            } else {
                (401, r#"{"message":"Bad credentials"}"#.to_string())
            }
        });
        let http = HttpClient::new();
        let endpoints = GithubEndpoints::with_base(&fake.base);
        assert_eq!(verify_token(&http, &endpoints, "gho_ok").unwrap().login, "octocat");

        let fake2 = FakeGithub::start(|_, _| (401, r#"{"message":"Bad credentials"}"#.to_string()));
        let endpoints2 = GithubEndpoints::with_base(&fake2.base);
        assert!(verify_token(&http, &endpoints2, "gho_bad").is_err());
    }

    #[test]
    fn fetch_pulls_raw_repo_contents_and_publish_creates_a_private_gist() {
        let fake = FakeGithub::start(|req, body| {
            if req.starts_with("GET /repos/acme/team/contents/roster.json") {
                (200, "signed-roster-bytes".to_string())
            } else if req.starts_with("POST /gists") {
                assert!(body.contains("\"public\":false"), "gist must be private");
                (201, r#"{"id":"abc123","html_url":"https://gist.github.com/octocat/abc123"}"#.to_string())
            } else {
                (404, "{}".to_string())
            }
        });
        let endpoints = GithubEndpoints::with_base(&fake.base);
        let client = GithubClient::with_endpoints(HttpClient::new(), endpoints, Some("gho_ok".into()));

        let bytes = client
            .fetch(&RemoteLocator {
                provider: "github".into(),
                host: None,
                path: "acme/team/roster.json".into(),
                reference: None,
            })
            .unwrap();
        assert_eq!(bytes, b"signed-roster-bytes");

        let published = client.publish_private("ferry-x.sealed", b"ciphertext").unwrap();
        assert_eq!(published.id, "abc123");
        assert!(published.url.contains("gist.github.com"));
    }

    #[test]
    #[ignore = "hits the real GitHub API; set GITHUB_TEST_TOKEN and run with --ignored"]
    fn contract_real_github_verifies_a_token_and_publishes_then_deletes_a_gist() {
        let token = std::env::var("GITHUB_TEST_TOKEN")
            .expect("set GITHUB_TEST_TOKEN to a PAT with the `gist` scope");
        let http = HttpClient::new();
        let endpoints = GithubEndpoints::default();

        let login = verify_token(&http, &endpoints, &token).expect("token should be accepted");
        assert!(!login.login.is_empty());

        let client = GithubClient::with_endpoints(HttpClient::new(), GithubEndpoints::default(), Some(token.clone()));
        let published = client
            .publish_private("ferry-contract-test.txt", b"ferry contract test payload")
            .expect("gist creation should succeed");
        assert!(published.url.contains("gist.github.com"));

        let owned = auth_headers(&token);
        http.get(&format!("{}/gists/{}", endpoints.api, published.id), &header_refs(&owned))
            .expect("the gist should be readable back");

        // cleanup: DELETE the throwaway gist
        let del = ureq::Agent::config_builder()
            .timeout_global(Some(std::time::Duration::from_secs(30)))
            .build();
        let agent: ureq::Agent = del.into();
        let mut req = agent.delete(&format!("{}/gists/{}", endpoints.api, published.id));
        for (k, v) in &owned {
            req = req.header(*k, v.as_str());
        }
        let _ = req.call();
    }

    #[test]
    fn publish_without_a_token_is_refused_before_any_request() {
        let client = GithubClient::new(None);
        assert!(client.publish_private("x", b"y").is_err());
    }

    #[test]
    fn a_locator_missing_a_path_segment_is_rejected_before_any_request() {
        let client = GithubClient::new(None);
        let locator = RemoteLocator {
            provider: "github".into(),
            host: None,
            path: "owner/repo".into(),
            reference: None,
        };
        assert!(client.fetch(&locator).is_err());
    }
}
