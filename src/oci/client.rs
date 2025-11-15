use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use bytes::Bytes;
use futures::stream::StreamExt;
use reqwest::header::CONTENT_LENGTH;
use reqwest::header::{ACCEPT, CONTENT_TYPE, HeaderMap, HeaderValue, LOCATION, WWW_AUTHENTICATE};
use reqwest::{Method, StatusCode, Url};
use serde_json::Value;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::Mutex;

use crate::oci::auth::RegistryAuth;
use crate::oci::errors::OciError;
use crate::oci::manifest::{OciDescriptor, OciImageManifest};
use crate::oci::reference::Reference;

const MANIFEST_MEDIA_TYPE: &str = "application/vnd.docker.distribution.manifest.v2+json";
const OCTET_STREAM: &str = "application/octet-stream";

#[derive(Clone, Default)]
pub struct ClientConfig {
    pub user_agent: Option<String>,
}

fn env_flag(name: &str) -> bool {
    std::env::var(name)
        .map(|value| {
            let value = value.trim();
            !(value.is_empty()
                || value == "0"
                || value.eq_ignore_ascii_case("false")
                || value.eq_ignore_ascii_case("off"))
        })
        .unwrap_or(false)
}

fn oci_debug_enabled() -> bool {
    static FLAG: OnceLock<bool> = OnceLock::new();
    *FLAG.get_or_init(|| env_flag("OCI_DEBUG"))
}

fn upload_debug_enabled() -> bool {
    static FLAG: OnceLock<bool> = OnceLock::new();
    *FLAG.get_or_init(|| env_flag("OCI_DEBUG_UPLOAD") || oci_debug_enabled())
}

fn log_debug<F>(message: F)
where
    F: FnOnce() -> String,
{
    if oci_debug_enabled() {
        println!("[OCI] {}", message());
    }
}

#[derive(Clone)]
pub struct Client {
    http: reqwest::Client,
    _config: ClientConfig,
    tokens: Arc<Mutex<HashMap<String, String>>>,
}

impl Client {
    pub fn new(config: ClientConfig) -> Self {
        let builder = reqwest::Client::builder().user_agent(
            config
                .user_agent
                .clone()
                .unwrap_or_else(|| "docker-image-pusher/0.0".to_string()),
        );
        let http = builder.build().expect("Failed to build client");
        Self {
            http,
            _config: config,
            tokens: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn base_url(reference: &Reference) -> Result<Url, OciError> {
        let url = format!("https://{}/v2/{}", reference.registry, reference.repository);
        Url::parse(&url).map_err(|e| OciError::Reference(format!("Invalid URL: {}", e)))
    }

    fn pull_scope(reference: &Reference) -> String {
        format!("repository:{}:pull", reference.repository)
    }

    fn push_scope(reference: &Reference) -> String {
        format!("repository:{}:push,pull", reference.repository)
    }

    async fn get_cached_token(&self, scope: &str) -> Option<String> {
        let guard = self.tokens.lock().await;
        guard.get(scope).cloned()
    }

    async fn store_token(&self, scope: &str, token: &str) {
        let mut guard = self.tokens.lock().await;
        guard.insert(scope.to_string(), token.to_string());
    }

    fn parse_bearer_challenge(value: &str) -> Result<(String, HashMap<String, String>), OciError> {
        let mut parts = value.splitn(2, ' ');
        let scheme = parts.next().unwrap_or("").trim().to_owned();
        let remainder = parts.next().unwrap_or("").trim();
        let mut params = HashMap::new();
        for segment in remainder.split(',') {
            let segment = segment.trim();
            if segment.is_empty() {
                continue;
            }
            if let Some((k, v)) = segment.split_once('=') {
                let key = k.trim().to_ascii_lowercase();
                let value = v.trim().trim_matches('"').to_string();
                params.insert(key, value);
            }
        }
        Ok((scheme.to_ascii_lowercase(), params))
    }

    async fn fetch_bearer_token(
        &self,
        challenge: &str,
        scope: &str,
        auth: &RegistryAuth,
    ) -> Result<String, OciError> {
        let (scheme, params) = Self::parse_bearer_challenge(challenge)?;
        if scheme != "bearer" {
            return Err(OciError::Auth(format!(
                "Unsupported auth scheme: {}",
                scheme
            )));
        }

        let realm = params
            .get("realm")
            .ok_or_else(|| OciError::Auth("Missing realm in auth challenge".to_string()))?;
        let mut url = Url::parse(realm)
            .map_err(|e| OciError::Auth(format!("Invalid auth realm URL: {}", e)))?;
        {
            let mut pairs = url.query_pairs_mut();
            if let Some(service) = params.get("service") {
                pairs.append_pair("service", service);
            }
            let scope_value = if scope.is_empty() {
                params.get("scope").cloned()
            } else {
                Some(scope.to_string())
            };
            if let Some(value) = scope_value {
                pairs.append_pair("scope", &value);
            }
        }

        let mut req = self.http.get(url);
        req = auth.apply(req);
        let resp = req.send().await?;
        if !resp.status().is_success() {
            return Err(OciError::Auth(format!(
                "Failed to obtain bearer token: {}",
                resp.status()
            )));
        }
        let payload: Value = resp
            .json()
            .await
            .map_err(|e| OciError::Auth(e.to_string()))?;
        let token = payload
            .get("token")
            .or_else(|| payload.get("access_token"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| OciError::Auth("Bearer token missing in response".to_string()))?;
        Ok(token.to_string())
    }

    async fn request(
        &self,
        method: Method,
        url: Url,
        auth: &RegistryAuth,
        headers: Option<HeaderMap>,
        body: Option<Bytes>,
        scope: Option<String>,
    ) -> Result<reqwest::Response, OciError> {
        let headers_clone = headers.clone();
        let body_clone = body.clone();
        let scope_clone = scope.clone();

        let mut req = self.http.request(method.clone(), url.clone());
        if let Some(ref hdrs) = headers_clone {
            req = req.headers(hdrs.clone());
        }

        if let Some(ref scope_value) = scope_clone {
            if let Some(token) = self.get_cached_token(scope_value).await {
                req = req.bearer_auth(token);
            } else {
                req = auth.apply(req);
            }
        } else {
            req = auth.apply(req);
        }

        if let Some(ref body_value) = body_clone {
            req = req.body(body_value.clone());
        }
        let body_len = body_clone.as_ref().map(|b| b.len()).unwrap_or(0);
        log_debug(|| {
            let scope_label = scope_clone.as_deref().unwrap_or("-");
            format!(
                "➡️  {} {} scope={} bytes={}",
                method, url, scope_label, body_len
            )
        });
        let resp = req.send().await?;
        log_debug(|| format!("⬅️  {} {}", resp.status(), resp.url()));
        if resp.status() != StatusCode::UNAUTHORIZED {
            return Ok(resp);
        }

        let scope_value = match scope {
            Some(value) => value,
            None => return Ok(resp),
        };

        let challenge = resp
            .headers()
            .get(WWW_AUTHENTICATE)
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| OciError::Auth("Registry returned 401 without challenge".to_string()))?
            .to_string();

        log_debug(|| format!("🔐 Fetching bearer token for scope {}", scope_value));
        let token = self
            .fetch_bearer_token(&challenge, &scope_value, auth)
            .await?;
        self.store_token(&scope_value, &token).await;
        log_debug(|| "🔐 Bearer token obtained".to_string());

        let mut retry_req = self.http.request(method, url);
        if let Some(hdrs) = headers_clone {
            retry_req = retry_req.headers(hdrs);
        }
        if let Some(body_value) = body_clone {
            retry_req = retry_req.body(body_value);
        }
        log_debug(|| "↻ Retrying request with bearer token".to_string());
        let resp = retry_req.bearer_auth(token).send().await?;
        log_debug(|| format!("⬅️  {} {}", resp.status(), resp.url()));
        Ok(resp)
    }

    fn manifest_accept_header() -> HeaderValue {
        HeaderValue::from_static(MANIFEST_MEDIA_TYPE)
    }

    fn resolve_upload_url(base: &Url, location: Option<&HeaderValue>) -> Result<Url, OciError> {
        let location = location
            .ok_or_else(|| OciError::InvalidResponse("Missing upload location".to_string()))?
            .to_str()
            .map_err(|_| OciError::InvalidResponse("Invalid upload location header".to_string()))?;
        if location.starts_with("http://") || location.starts_with("https://") {
            Url::parse(location)
                .map_err(|e| OciError::InvalidResponse(format!("Invalid upload URL: {}", e)))
        } else {
            base.join(location).map_err(|e| {
                OciError::InvalidResponse(format!("Failed to resolve upload URL: {}", e))
            })
        }
    }

    async fn begin_upload(
        &self,
        reference: &Reference,
        auth: &RegistryAuth,
    ) -> Result<Url, OciError> {
        let mut url = Self::base_url(reference)?;
        url.path_segments_mut()
            .map_err(|_| OciError::Reference("Invalid repository path".to_string()))?
            .push("blobs")
            .push("uploads")
            .push("");
        let scope = Self::push_scope(reference);
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_LENGTH, HeaderValue::from_static("0"));
        let resp = self
            .request(
                Method::POST,
                url.clone(),
                auth,
                Some(headers),
                None,
                Some(scope),
            )
            .await?;
        if resp.status() != StatusCode::ACCEPTED {
            return Err(OciError::Status {
                status: resp.status().as_u16(),
                message: resp.text().await.unwrap_or_default(),
            });
        }
        Self::resolve_upload_url(resp.url(), resp.headers().get(LOCATION))
    }

    pub async fn pull_image_manifest(
        &self,
        reference: &Reference,
        auth: &RegistryAuth,
    ) -> Result<(OciImageManifest, Option<String>), OciError> {
        let tag = reference
            .tag
            .as_ref()
            .ok_or_else(|| OciError::Reference("Cannot pull manifest without tag".to_string()))?;
        let mut url = Self::base_url(reference)?;
        url.path_segments_mut()
            .map_err(|_| OciError::Reference("Invalid repository path".to_string()))?
            .push("manifests")
            .push(tag);

        let mut headers = HeaderMap::new();
        headers.insert(ACCEPT, Self::manifest_accept_header());

        let scope = Self::pull_scope(reference);
        let resp = self
            .request(Method::GET, url, auth, Some(headers), None, Some(scope))
            .await?;
        if !resp.status().is_success() {
            return Err(OciError::Status {
                status: resp.status().as_u16(),
                message: resp.text().await.unwrap_or_default(),
            });
        }
        let digest = resp
            .headers()
            .get("Docker-Content-Digest")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());
        let manifest = resp.json::<OciImageManifest>().await?;
        Ok((manifest, digest))
    }

    pub async fn pull_blob<W: AsyncWrite + Unpin + Send>(
        &self,
        reference: &Reference,
        descriptor: &OciDescriptor,
        auth: &RegistryAuth,
        writer: &mut W,
    ) -> Result<(), OciError> {
        let mut url = Self::base_url(reference)?;
        url.path_segments_mut()
            .map_err(|_| OciError::Reference("Invalid repository path".to_string()))?
            .push("blobs")
            .push(&descriptor.digest);

        let scope = Self::pull_scope(reference);
        let resp = self
            .request(Method::GET, url, auth, None, None, Some(scope))
            .await?;
        if !resp.status().is_success() {
            return Err(OciError::Status {
                status: resp.status().as_u16(),
                message: resp.text().await.unwrap_or_default(),
            });
        }

        let mut stream = resp.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let data = chunk?;
            writer.write_all(&data).await?;
        }
        Ok(())
    }

    pub async fn blob_exists(
        &self,
        reference: &Reference,
        digest: &str,
        auth: &RegistryAuth,
    ) -> Result<bool, OciError> {
        let mut url = Self::base_url(reference)?;
        url.path_segments_mut()
            .map_err(|_| OciError::Reference("Invalid repository path".to_string()))?
            .push("blobs")
            .push(digest);
        let scope = Self::pull_scope(reference);
        let resp = self
            .request(Method::HEAD, url, auth, None, None, Some(scope))
            .await?;
        match resp.status() {
            StatusCode::OK => Ok(true),
            StatusCode::NOT_FOUND => Ok(false),
            status => Err(OciError::Status {
                status: status.as_u16(),
                message: resp.text().await.unwrap_or_default(),
            }),
        }
    }

    pub async fn push_blob(
        &self,
        reference: &Reference,
        auth: &RegistryAuth,
        data: &[u8],
        digest: &str,
    ) -> Result<(), OciError> {
        let mut upload_url = self.begin_upload(reference, auth).await?;
        let scope = Self::push_scope(reference);

        if upload_debug_enabled() {
            println!("   🔗 Upload session started at {}", upload_url);
        }

        if !data.is_empty() {
            let mut headers = HeaderMap::new();
            headers.insert(CONTENT_TYPE, HeaderValue::from_static(OCTET_STREAM));
            headers.insert(
                CONTENT_LENGTH,
                HeaderValue::from_str(&data.len().to_string()).map_err(|_| {
                    OciError::InvalidResponse("Failed to encode content length".to_string())
                })?,
            );
            let resp = self
                .request(
                    Method::PATCH,
                    upload_url.clone(),
                    auth,
                    Some(headers),
                    Some(Bytes::copy_from_slice(data)),
                    Some(scope.clone()),
                )
                .await?;
            if resp.status() != StatusCode::ACCEPTED {
                return Err(OciError::Status {
                    status: resp.status().as_u16(),
                    message: resp.text().await.unwrap_or_default(),
                });
            }
            if let Some(next) = resp.headers().get(LOCATION) {
                if upload_debug_enabled() {
                    if let Ok(loc) = next.to_str() {
                        println!("   ↪️  Upload redirected to {}", loc);
                    }
                }
                upload_url = Self::resolve_upload_url(resp.url(), Some(next))?;
            }
        }

        upload_url.query_pairs_mut().append_pair("digest", digest);
        let mut final_headers = HeaderMap::new();
        final_headers.insert(CONTENT_LENGTH, HeaderValue::from_static("0"));

        if upload_debug_enabled() {
            println!("   ✅ Finalizing upload via {}", upload_url);
        }
        let resp = self
            .request(
                Method::PUT,
                upload_url,
                auth,
                Some(final_headers),
                None,
                Some(scope),
            )
            .await?;
        if !resp.status().is_success() {
            return Err(OciError::Status {
                status: resp.status().as_u16(),
                message: resp.text().await.unwrap_or_default(),
            });
        }
        Ok(())
    }

    pub async fn push_blob_stream<R: AsyncRead + Unpin + Send>(
        &self,
        reference: &Reference,
        auth: &RegistryAuth,
        reader: &mut R,
        digest: &str,
        chunk_size: usize,
    ) -> Result<(), OciError> {
        let mut upload_url = self.begin_upload(reference, auth).await?;
        let scope = Self::push_scope(reference);
        let mut buffer = vec![0u8; chunk_size];

        if upload_debug_enabled() {
            println!("   🔗 Upload session started at {}", upload_url);
        }

        loop {
            let read = reader.read(&mut buffer).await?;
            if read == 0 {
                break;
            }
            let mut headers = HeaderMap::new();
            headers.insert(CONTENT_TYPE, HeaderValue::from_static(OCTET_STREAM));
            headers.insert(
                CONTENT_LENGTH,
                HeaderValue::from_str(&read.to_string()).map_err(|_| {
                    OciError::InvalidResponse("Failed to encode chunk length".to_string())
                })?,
            );
            let chunk = Bytes::copy_from_slice(&buffer[..read]);
            let resp = self
                .request(
                    Method::PATCH,
                    upload_url.clone(),
                    auth,
                    Some(headers),
                    Some(chunk),
                    Some(scope.clone()),
                )
                .await?;
            if resp.status() != StatusCode::ACCEPTED {
                return Err(OciError::Status {
                    status: resp.status().as_u16(),
                    message: resp.text().await.unwrap_or_default(),
                });
            }
            if let Some(next) = resp.headers().get(LOCATION) {
                if upload_debug_enabled() {
                    if let Ok(loc) = next.to_str() {
                        println!("   ↪️  Upload redirected to {}", loc);
                    }
                }
                upload_url = Self::resolve_upload_url(resp.url(), Some(next))?;
            }
        }

        upload_url.query_pairs_mut().append_pair("digest", digest);
        let mut final_headers = HeaderMap::new();
        final_headers.insert(CONTENT_LENGTH, HeaderValue::from_static("0"));

        if upload_debug_enabled() {
            println!("   ✅ Finalizing upload via {}", upload_url);
        }
        let resp = self
            .request(
                Method::PUT,
                upload_url,
                auth,
                Some(final_headers),
                None,
                Some(scope),
            )
            .await?;
        if !resp.status().is_success() {
            return Err(OciError::Status {
                status: resp.status().as_u16(),
                message: resp.text().await.unwrap_or_default(),
            });
        }
        Ok(())
    }

    pub async fn push_manifest(
        &self,
        reference: &Reference,
        manifest: &OciImageManifest,
        auth: &RegistryAuth,
    ) -> Result<String, OciError> {
        let tag = reference
            .tag
            .as_ref()
            .ok_or_else(|| OciError::Reference("Cannot push manifest without tag".to_string()))?;
        let mut url = Self::base_url(reference)?;
        url.path_segments_mut()
            .map_err(|_| OciError::Reference("Invalid repository path".to_string()))?
            .push("manifests")
            .push(tag);
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static(MANIFEST_MEDIA_TYPE));
        let body = serde_json::to_vec(manifest)
            .map_err(|e| OciError::InvalidResponse(format!("Failed to encode manifest: {}", e)))?;
        let scope = Self::push_scope(reference);
        let resp = self
            .request(
                Method::PUT,
                url,
                auth,
                Some(headers),
                Some(Bytes::from(body)),
                Some(scope),
            )
            .await?;
        if !resp.status().is_success() {
            return Err(OciError::Status {
                status: resp.status().as_u16(),
                message: resp.text().await.unwrap_or_default(),
            });
        }
        let location = resp
            .headers()
            .get(LOCATION)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string())
            .unwrap_or_else(|| reference.to_string());
        Ok(location)
    }
}
