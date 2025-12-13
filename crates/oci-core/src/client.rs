use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};

use bytes::{Bytes, BytesMut};
use futures::stream::StreamExt;
use reqwest::header::CONTENT_LENGTH;
use reqwest::header::{ACCEPT, CONTENT_TYPE, HeaderMap, HeaderValue, LOCATION, WWW_AUTHENTICATE};
use reqwest::{Method, StatusCode, Url};
use serde_json::Value;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::Mutex;

use crate::auth::RegistryAuth;
use crate::errors::OciError;
use crate::manifest::{OciDescriptor, OciImageManifest};
use crate::progress::{ChunkTransferEvent, ProgressReporterHandle};
use crate::reference::Reference;

const MANIFEST_MEDIA_TYPE: &str = "application/vnd.docker.distribution.manifest.v2+json";
const OCTET_STREAM: &str = "application/octet-stream";
const OCI_CHUNK_MIN_LENGTH: &str = "OCI-Chunk-Min-Length";

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

fn chunk_trace_enabled() -> bool {
    static FLAG: OnceLock<bool> = OnceLock::new();
    *FLAG.get_or_init(|| env_flag("OCI_CHUNK_TRACE") || upload_debug_enabled())
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

    fn parse_chunk_hint(headers: &HeaderMap) -> Option<usize> {
        headers
            .get(OCI_CHUNK_MIN_LENGTH)
            .and_then(|value| value.to_str().ok())
            .and_then(|raw| raw.trim().parse::<u64>().ok())
            .and_then(|value| {
                if value > usize::MAX as u64 {
                    None
                } else {
                    Some(value as usize)
                }
            })
    }

    async fn begin_upload(
        &self,
        reference: &Reference,
        auth: &RegistryAuth,
    ) -> Result<(Url, Option<usize>), OciError> {
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
            let status = resp.status();
            let message = resp.text().await.unwrap_or_default();
            if Self::requires_upload_reset(status, &message) {
                return Err(OciError::UploadReset(message));
            }
            return Err(OciError::Status {
                status: status.as_u16(),
                message,
            });
        }
        let chunk_hint = Self::parse_chunk_hint(resp.headers());
        let resolved = Self::resolve_upload_url(resp.url(), resp.headers().get(LOCATION))?;
        Ok((resolved, chunk_hint))
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
        let (mut upload_url, _) = self.begin_upload(reference, auth).await?;
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
        total_size_bytes: Option<u64>,
        reporter: Option<ProgressReporterHandle>,
        progress: Option<Arc<AtomicU64>>,
    ) -> Result<(), OciError> {
        let (mut upload_url, initial_chunk_hint) = self.begin_upload(reference, auth).await?;
        let scope = Self::push_scope(reference);
        let mut effective_chunk = chunk_size.max(1);
        let mut chunk_seq: u64 = 0;
        let mut total_sent_bytes: u64 = 0;
        let mut largest_chunk: usize = 0;
        let total_size_hint = total_size_bytes.filter(|value| *value > 0);
        let chunk_trace = chunk_trace_enabled();
        if let Some(min_required) = initial_chunk_hint {
            if min_required > effective_chunk {
                println!(
                    "   📏 Registry requested {:.0} MB minimum chunks; increasing buffer",
                    min_required as f64 / (1024.0 * 1024.0)
                );
                effective_chunk = min_required;
            }
        }

        if upload_debug_enabled() {
            println!("   🔗 Upload session started at {}", upload_url);
        }

        let mut buffer = BytesMut::with_capacity(effective_chunk);

        loop {
            while buffer.len() < effective_chunk {
                let read = reader.read_buf(&mut buffer).await?;
                if read == 0 {
                    break;
                }
            }

            if buffer.is_empty() {
                break;
            }

            let chunk_bytes = buffer.split().freeze();
            let chunk_len = chunk_bytes.len();
            let (next_url, next_hint) = self
                .transmit_chunk(upload_url, auth, &scope, chunk_bytes, progress.as_ref())
                .await?;
            upload_url = next_url;
            chunk_seq += 1;
            total_sent_bytes += chunk_len as u64;
            largest_chunk = largest_chunk.max(chunk_len);
            Self::emit_chunk_event(
                digest,
                chunk_seq,
                chunk_len,
                total_sent_bytes,
                total_size_hint,
                chunk_trace,
                reporter.as_ref(),
            );

            if let Some(min_required) = next_hint {
                if min_required > effective_chunk {
                    println!(
                        "   📏 Registry increased minimum chunk to {:.0} MB; adjusting",
                        min_required as f64 / (1024.0 * 1024.0)
                    );
                    effective_chunk = min_required;
                    buffer = BytesMut::with_capacity(effective_chunk);
                }
            }
        }

        Self::log_chunk_summary(
            digest,
            chunk_seq,
            total_sent_bytes,
            largest_chunk,
            reporter.as_ref(),
            chunk_trace,
        );

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

    fn emit_chunk_trace_line(
        digest: &str,
        chunk_index: u64,
        chunk_len: usize,
        total_sent_bytes: u64,
        total_size_hint: Option<u64>,
        chunk_trace: bool,
    ) {
        if !chunk_trace {
            return;
        }
        let chunk_mb = chunk_len as f64 / (1024.0 * 1024.0);
        if let Some(total) = total_size_hint {
            let percent = (total_sent_bytes as f64 / total as f64 * 100.0).min(100.0);
            println!(
                "   🔹 {} chunk #{chunk_index}: {:.2} MB ({:.1}% cumulative)",
                digest, chunk_mb, percent
            );
        } else {
            println!(
                "   🔹 {} chunk #{chunk_index}: {:.2} MB (cumulative {} bytes)",
                digest, chunk_mb, total_sent_bytes
            );
        }
    }

    fn emit_chunk_event(
        digest: &str,
        chunk_index: u64,
        chunk_len: usize,
        total_sent_bytes: u64,
        total_size_hint: Option<u64>,
        chunk_trace: bool,
        reporter: Option<&ProgressReporterHandle>,
    ) {
        Self::emit_chunk_trace_line(
            digest,
            chunk_index,
            chunk_len,
            total_sent_bytes,
            total_size_hint,
            chunk_trace,
        );
        if let Some(handle) = reporter {
            handle.on_chunk_transferred(ChunkTransferEvent {
                digest: digest.to_string(),
                chunk_index,
                chunk_bytes: chunk_len,
                total_transferred: total_sent_bytes,
                total_bytes: total_size_hint,
            });
        }
    }

    fn log_chunk_summary(
        digest: &str,
        chunk_seq: u64,
        total_sent_bytes: u64,
        largest_chunk: usize,
        reporter: Option<&ProgressReporterHandle>,
        chunk_trace: bool,
    ) {
        if reporter.is_none() || chunk_trace {
            let largest_mb = largest_chunk as f64 / (1024.0 * 1024.0);
            println!(
                "   🧾 {}: {} chunk(s) uploaded, largest chunk {:.2} MB, total {} bytes",
                digest, chunk_seq, largest_mb, total_sent_bytes
            );
        }
    }

    fn requires_upload_reset(status: StatusCode, body: &str) -> bool {
        if status == StatusCode::NOT_FOUND || status == StatusCode::BAD_REQUEST {
            let lowered = body.to_ascii_lowercase();
            return lowered.contains("blob_upload_invalid")
                || lowered.contains("blob upload invalid")
                || lowered.contains("blob upload unknown")
                || lowered.contains("blob unknown")
                || lowered.contains("upload invalid");
        }
        false
    }

    async fn transmit_chunk(
        &self,
        upload_url: Url,
        auth: &RegistryAuth,
        scope: &str,
        chunk: Bytes,
        progress: Option<&Arc<AtomicU64>>,
    ) -> Result<(Url, Option<usize>), OciError> {
        let chunk_len = chunk.len();
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static(OCTET_STREAM));
        headers.insert(
            CONTENT_LENGTH,
            HeaderValue::from_str(&chunk_len.to_string()).map_err(|_| {
                OciError::InvalidResponse("Failed to encode chunk length".to_string())
            })?,
        );
        let chunk_bytes = Bytes::from(chunk);
        let resp = self
            .request(
                Method::PATCH,
                upload_url.clone(),
                auth,
                Some(headers),
                Some(chunk_bytes),
                Some(scope.to_string()),
            )
            .await?;
        if resp.status() != StatusCode::ACCEPTED {
            return Err(OciError::Status {
                status: resp.status().as_u16(),
                message: resp.text().await.unwrap_or_default(),
            });
        }
        let mut next_url = upload_url;
        if let Some(next) = resp.headers().get(LOCATION) {
            if upload_debug_enabled() {
                if let Ok(loc) = next.to_str() {
                    println!("   ↪️  Upload redirected to {}", loc);
                }
            }
            next_url = Self::resolve_upload_url(resp.url(), Some(next))?;
        }

        if let Some(counter) = progress {
            counter.fetch_add(chunk_len as u64, Ordering::Relaxed);
        }

        Ok((next_url, Self::parse_chunk_hint(resp.headers())))
    }

    pub async fn push_manifest(
        &self,
        reference: &Reference,
        manifest: &OciImageManifest,
        auth: &RegistryAuth,
    ) -> Result<String, OciError> {
        let body = serde_json::to_vec(manifest)
            .map_err(|e| OciError::InvalidResponse(format!("Failed to encode manifest: {}", e)))?;
        let mt = manifest.media_type.as_str();
        self.push_manifest_bytes(reference, mt, &body, auth).await
    }

    pub async fn push_manifest_bytes(
        &self,
        reference: &Reference,
        media_type: &str,
        body: &[u8],
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
        headers.insert(
            CONTENT_TYPE,
            HeaderValue::from_str(media_type)
                .unwrap_or_else(|_| HeaderValue::from_static(MANIFEST_MEDIA_TYPE)),
        );
        let scope = Self::push_scope(reference);
        let resp = self
            .request(
                Method::PUT,
                url,
                auth,
                Some(headers),
                Some(Bytes::from(body.to_vec())),
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
