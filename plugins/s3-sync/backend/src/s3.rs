//! Minimal S3-compatible client: SigV4-signed REST calls over reqwest.
//! Covers exactly what migration needs: list buckets/objects, head, streaming
//! get, single put and multipart upload. No SDK weight, full control over
//! path-style addressing and self-signed TLS.

use std::path::Path;
use std::time::Duration;

use aws_sigv4::http_request::{sign, SignableBody, SignableRequest, SigningSettings};
use aws_sigv4::sign::v4;
use chrono::{DateTime, Utc};
use futures_util::StreamExt;
use percent_encoding::{utf8_percent_encode, AsciiSet, NON_ALPHANUMERIC};
use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;

use crate::model::ObjectMeta;

/// Keep '/' unescaped so object keys stay readable in URLs; everything else
/// non-alphanumeric is escaped (S3 requires RFC 3986 for key paths).
const KEY_SEGMENT: &AsciiSet = &NON_ALPHANUMERIC.remove(b'/').remove(b'-').remove(b'_').remove(b'.').remove(b'~');
const QUERY_VALUE: &AsciiSet = &NON_ALPHANUMERIC.remove(b'-').remove(b'_').remove(b'.').remove(b'~');

#[derive(Debug, Clone)]
pub struct S3Config {
    pub endpoint: String,
    pub region: String,
    pub access_key: String,
    pub secret_key: String,
    pub path_style: bool,
    pub insecure_tls: bool,
}

#[derive(Debug)]
pub struct S3Error {
    pub status: Option<u16>,
    pub code: String,
    pub message: String,
}

impl S3Error {
    pub fn transport(message: impl Into<String>) -> Self {
        Self { status: None, code: "Transport".into(), message: message.into() }
    }

    /// 5xx, throttling and transport failures are worth retrying; a 403/404
    /// will not fix itself.
    pub fn retryable(&self) -> bool {
        match self.status {
            None => true,
            Some(429) => true,
            Some(status) => status >= 500,
        }
    }
}

impl std::fmt::Display for S3Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.code.is_empty() {
            write!(f, "{}", self.message)
        } else {
            write!(f, "{}: {}", self.code, self.message)
        }
    }
}

impl std::error::Error for S3Error {}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObjectListing {
    pub objects: Vec<ObjectMeta>,
    pub next_token: Option<String>,
}

#[derive(Clone)]
pub struct S3Client {
    config: S3Config,
    http: reqwest::Client,
}

impl S3Client {
    pub fn new(config: S3Config) -> Result<Self, S3Error> {
        let endpoint = config.endpoint.trim().trim_end_matches('/').to_string();
        if !(endpoint.starts_with("http://") || endpoint.starts_with("https://")) {
            return Err(S3Error::transport("Endpoint must start with http:// or https://"));
        }
        let mut builder = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(15))
            .pool_idle_timeout(Duration::from_secs(60));
        if config.insecure_tls {
            builder = builder.danger_accept_invalid_certs(true);
        }
        let http = builder.build().map_err(|e| S3Error::transport(e.to_string()))?;
        Ok(Self { config: S3Config { endpoint, ..config }, http })
    }

    fn object_url(&self, bucket: &str, key: &str) -> String {
        let encoded_key = utf8_percent_encode(key, KEY_SEGMENT);
        if self.config.path_style {
            format!("{}/{bucket}/{encoded_key}", self.config.endpoint)
        } else {
            let (scheme, rest) = self.config.endpoint.split_once("://").expect("validated endpoint");
            format!("{scheme}://{bucket}.{rest}/{encoded_key}")
        }
    }

    fn bucket_url(&self, bucket: &str) -> String {
        if self.config.path_style {
            format!("{}/{bucket}", self.config.endpoint)
        } else {
            let (scheme, rest) = self.config.endpoint.split_once("://").expect("validated endpoint");
            format!("{scheme}://{bucket}.{rest}")
        }
    }

    /// Sign a request with SigV4 and send it. `payload_hash` is the hex SHA-256
    /// of the body (empty-string hash for bodyless requests).
    async fn send(
        &self,
        mut request: http::Request<Vec<u8>>,
        payload_hash: &str,
    ) -> Result<reqwest::Response, S3Error> {
        let identity = aws_smithy_runtime_api::client::identity::Identity::new(
            aws_credential_types::Credentials::new(&self.config.access_key, &self.config.secret_key, None, None, "s3-sync"),
            None,
        );
        let mut settings = SigningSettings::default();
        settings.payload_checksum_kind = aws_sigv4::http_request::PayloadChecksumKind::XAmzSha256;
        // S3 keys are literal: never normalize the path, and treat it as
        // already percent-encoded (we encode keys ourselves).
        settings.uri_path_normalization_mode = aws_sigv4::http_request::UriPathNormalizationMode::Disabled;
        settings.percent_encoding_mode = aws_sigv4::http_request::PercentEncodingMode::Single;
        let params = v4::SigningParams::builder()
            .identity(&identity)
            .region(&self.config.region)
            .name("s3")
            .time(std::time::SystemTime::now())
            .settings(settings)
            .build()
            .map_err(|e| S3Error::transport(e.to_string()))?;
        let signable = SignableRequest::new(
            request.method().as_str(),
            request.uri().to_string(),
            request.headers().iter().map(|(k, v)| (k.as_str(), v.to_str().unwrap_or(""))),
            SignableBody::Precomputed(payload_hash.to_string()),
        )
        .map_err(|e| S3Error::transport(e.to_string()))?;
        let (instructions, _signature) = sign(signable, &params.into())
            .map_err(|e| S3Error::transport(e.to_string()))?
            .into_parts();
        instructions.apply_to_request_http1x(&mut request);

        let (parts, body) = request.into_parts();
        let mut builder = self.http
            .request(parts.method.clone(), parts.uri.to_string())
            .body(body);
        for (name, value) in parts.headers.iter() {
            builder = builder.header(name, value);
        }
        builder.send().await.map_err(|e| S3Error::transport(e.to_string()))
    }

    async fn error_from_response(response: reqwest::Response) -> S3Error {
        let status = response.status().as_u16();
        let body = response.text().await.unwrap_or_default();
        let (code, message) = parse_s3_error(&body).unwrap_or_else(|| ("Http".to_string(), body.clone()));
        S3Error { status: Some(status), code, message: truncate(&message, 500) }
    }

    pub async fn list_buckets(&self) -> Result<Vec<String>, S3Error> {
        let request = http::Request::builder()
            .method("GET")
            .uri(format!("{}/", self.config.endpoint))
            .header("host", host_header(&self.config.endpoint))
            .body(Vec::new())
            .map_err(|e| S3Error::transport(e.to_string()))?;
        let response = self.send(request, EMPTY_SHA256).await?;
        if !response.status().is_success() {
            return Err(Self::error_from_response(response).await);
        }
        let body = response.text().await.map_err(|e| S3Error::transport(e.to_string()))?;
        let doc = roxmltree::Document::parse(&body).map_err(|e| S3Error::transport(e.to_string()))?;
        let mut names = Vec::new();
        for bucket in doc.descendants().filter(|n| n.has_tag_name("Bucket")) {
            if let Some(name) = bucket.children().find(|n| n.has_tag_name("Name")).and_then(|n| n.text()) {
                names.push(name.to_string());
            }
        }
        Ok(names)
    }

    /// Create a bucket (used by integration tests; idempotent on MinIO when
    /// the bucket already exists in the same region).
    #[cfg(test)]
    pub async fn create_bucket(&self, bucket: &str) -> Result<(), S3Error> {
        let request = http::Request::builder()
            .method("PUT")
            .uri(self.bucket_url(bucket))
            .header("host", host_header(&self.bucket_url(bucket)))
            .body(Vec::new())
            .map_err(|e| S3Error::transport(e.to_string()))?;
        let response = self.send(request, EMPTY_SHA256).await?;
        if !response.status().is_success() {
            let error = Self::error_from_response(response).await;
            if error.code == "BucketAlreadyOwnedByYou" || error.code == "BucketAlreadyExists" {
                return Ok(());
            }
            return Err(error);
        }
        Ok(())
    }

    /// One page of ListObjectsV2. Keys are requested URL-encoded so unusual
    /// characters survive the XML round trip.
    pub async fn list_objects(
        &self,
        bucket: &str,
        prefix: &str,
        token: Option<&str>,
        max_keys: u32,
    ) -> Result<ObjectListing, S3Error> {
        let mut uri = format!(
            "{}?list-type=2&encoding-type=url&max-keys={max_keys}&prefix={}",
            self.bucket_url(bucket),
            utf8_percent_encode(prefix, QUERY_VALUE)
        );
        if let Some(token) = token {
            uri.push_str(&format!("&continuation-token={}", utf8_percent_encode(token, QUERY_VALUE)));
        }
        let request = http::Request::builder()
            .method("GET")
            .uri(uri)
            .header("host", host_header(&self.bucket_url(bucket)))
            .body(Vec::new())
            .map_err(|e| S3Error::transport(e.to_string()))?;
        let response = self.send(request, EMPTY_SHA256).await?;
        if !response.status().is_success() {
            return Err(Self::error_from_response(response).await);
        }
        let body = response.text().await.map_err(|e| S3Error::transport(e.to_string()))?;
        parse_listing(&body)
    }

    pub async fn head_object(&self, bucket: &str, key: &str) -> Result<Option<ObjectMeta>, S3Error> {
        let request = http::Request::builder()
            .method("HEAD")
            .uri(self.object_url(bucket, key))
            .header("host", host_header(&self.object_url(bucket, key)))
            .body(Vec::new())
            .map_err(|e| S3Error::transport(e.to_string()))?;
        let response = self.send(request, EMPTY_SHA256).await?;
        if response.status().as_u16() == 404 {
            return Ok(None);
        }
        if !response.status().is_success() {
            return Err(Self::error_from_response(response).await);
        }
        let headers = response.headers().clone();
        let size = headers.get("content-length").and_then(|v| v.to_str().ok()).and_then(|v| v.parse().ok()).unwrap_or(0);
        let etag = headers.get("etag").and_then(|v| v.to_str().ok()).unwrap_or("").trim_matches('"').to_string();
        let last_modified = headers.get("last-modified")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| DateTime::parse_from_rfc2822(v).ok())
            .map(|t| t.with_timezone(&Utc))
            .unwrap_or_else(Utc::now);
        let mut user_meta = std::collections::HashMap::new();
        for (name, value) in headers.iter() {
            if let Some(suffix) = name.as_str().strip_prefix("x-amz-meta-") {
                if let Ok(value) = value.to_str() {
                    user_meta.insert(suffix.to_string(), value.to_string());
                }
            }
        }
        Ok(Some(ObjectMeta { key: key.to_string(), size, etag, last_modified, user_meta }))
    }

    /// Stream an object into a local file without buffering it in memory.
    /// Returns the byte count and the source Content-Type so migration can
    /// preserve the MIME type on the target.
    pub async fn get_object_to_file(&self, bucket: &str, key: &str, path: &Path) -> Result<(u64, String), S3Error> {
        let request = http::Request::builder()
            .method("GET")
            .uri(self.object_url(bucket, key))
            .header("host", host_header(&self.object_url(bucket, key)))
            .body(Vec::new())
            .map_err(|e| S3Error::transport(e.to_string()))?;
        let response = self.send(request, EMPTY_SHA256).await?;
        if !response.status().is_success() {
            return Err(Self::error_from_response(response).await);
        }
        let content_type = response.headers().get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("application/octet-stream")
            .to_string();
        let mut file = tokio::fs::File::create(path).await.map_err(io_error)?;
        let mut written = 0u64;
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| S3Error::transport(e.to_string()))?;
            file.write_all(&chunk).await.map_err(io_error)?;
            written += chunk.len() as u64;
        }
        file.flush().await.map_err(io_error)?;
        Ok((written, content_type))
    }

    /// Single-shot PUT for small objects. Caller holds a memory-budget permit.
    pub async fn put_object(
        &self,
        bucket: &str,
        key: &str,
        body: Vec<u8>,
        content_type: &str,
        user_meta: &std::collections::HashMap<String, String>,
    ) -> Result<(), S3Error> {
        let hash = sha256_hex(&body);
        let mut builder = http::Request::builder()
            .method("PUT")
            .uri(self.object_url(bucket, key))
            .header("host", host_header(&self.object_url(bucket, key)))
            .header("content-type", content_type)
            .header("content-length", body.len().to_string());
        for (name, value) in user_meta {
            builder = builder.header(format!("x-amz-meta-{name}"), value.as_str());
        }
        let request = builder.body(body).map_err(|e| S3Error::transport(e.to_string()))?;
        let response = self.send(request, &hash).await?;
        if !response.status().is_success() {
            return Err(Self::error_from_response(response).await);
        }
        Ok(())
    }

    pub async fn create_multipart(&self, bucket: &str, key: &str, content_type: &str, user_meta: &std::collections::HashMap<String, String>) -> Result<String, S3Error> {
        let mut builder = http::Request::builder()
            .method("POST")
            .uri(format!("{}?uploads", self.object_url(bucket, key)))
            .header("host", host_header(&self.object_url(bucket, key)))
            .header("content-type", content_type);
        for (name, value) in user_meta {
            builder = builder.header(format!("x-amz-meta-{name}"), value.as_str());
        }
        let request = builder.body(Vec::new()).map_err(|e| S3Error::transport(e.to_string()))?;
        let response = self.send(request, EMPTY_SHA256).await?;
        if !response.status().is_success() {
            return Err(Self::error_from_response(response).await);
        }
        let body = response.text().await.map_err(|e| S3Error::transport(e.to_string()))?;
        let doc = roxmltree::Document::parse(&body).map_err(|e| S3Error::transport(e.to_string()))?;
        doc.descendants()
            .find(|n| n.has_tag_name("UploadId"))
            .and_then(|n| n.text())
            .map(str::to_string)
            .ok_or_else(|| S3Error::transport("CreateMultipartUpload returned no UploadId"))
    }

    pub async fn upload_part(&self, bucket: &str, key: &str, upload_id: &str, part_number: u32, body: Vec<u8>) -> Result<String, S3Error> {
        let hash = sha256_hex(&body);
        let uri = format!(
            "{}?partNumber={part_number}&uploadId={}",
            self.object_url(bucket, key),
            utf8_percent_encode(upload_id, QUERY_VALUE)
        );
        let request = http::Request::builder()
            .method("PUT")
            .uri(uri.clone())
            .header("host", host_header(&uri))
            .header("content-length", body.len().to_string())
            .body(body)
            .map_err(|e| S3Error::transport(e.to_string()))?;
        let response = self.send(request, &hash).await?;
        if !response.status().is_success() {
            return Err(Self::error_from_response(response).await);
        }
        response.headers().get("etag")
            .and_then(|v| v.to_str().ok())
            .map(|v| v.trim_matches('"').to_string())
            .ok_or_else(|| S3Error::transport("UploadPart returned no ETag"))
    }

    pub async fn complete_multipart(&self, bucket: &str, key: &str, upload_id: &str, parts: &[(u32, String)]) -> Result<(), S3Error> {
        let mut xml = String::from("<CompleteMultipartUpload>");
        for (number, etag) in parts {
            xml.push_str(&format!("<Part><PartNumber>{number}</PartNumber><ETag>\"{etag}\"</ETag></Part>"));
        }
        xml.push_str("</CompleteMultipartUpload>");
        let body = xml.into_bytes();
        let hash = sha256_hex(&body);
        let uri = format!("{}?uploadId={}", self.object_url(bucket, key), utf8_percent_encode(upload_id, QUERY_VALUE));
        let request = http::Request::builder()
            .method("POST")
            .uri(uri.clone())
            .header("host", host_header(&uri))
            .header("content-type", "application/xml")
            .header("content-length", body.len().to_string())
            .body(body)
            .map_err(|e| S3Error::transport(e.to_string()))?;
        let response = self.send(request, &hash).await?;
        if !response.status().is_success() {
            return Err(Self::error_from_response(response).await);
        }
        // S3 can report an error inside a 200 CompleteMultipartUpload body.
        let text = response.text().await.unwrap_or_default();
        if text.contains("<Error>") {
            let (code, message) = parse_s3_error(&text).unwrap_or_else(|| ("CompleteMultipart".into(), text));
            return Err(S3Error { status: Some(200), code, message });
        }
        Ok(())
    }

    pub async fn abort_multipart(&self, bucket: &str, key: &str, upload_id: &str) {
        let uri = format!("{}?uploadId={}", self.object_url(bucket, key), utf8_percent_encode(upload_id, QUERY_VALUE));
        let Ok(request) = http::Request::builder()
            .method("DELETE")
            .uri(uri.clone())
            .header("host", host_header(&uri))
            .body(Vec::new()) else { return };
        // Best effort: a leaked multipart is garbage-collected by lifecycle
        // rules on most services; never fail the run over cleanup.
        let _ = self.send(request, EMPTY_SHA256).await;
    }
}

pub const EMPTY_SHA256: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

pub fn sha256_hex(body: &[u8]) -> String {
    use sha2::Digest;
    let mut hasher = sha2::Sha256::new();
    hasher.update(body);
    hex::encode(hasher.finalize())
}

fn host_header(url: &str) -> String {
    url.split_once("://").map(|(_, rest)| rest.split('/').next().unwrap_or(rest)).unwrap_or(url).to_string()
}

fn io_error(error: std::io::Error) -> S3Error {
    S3Error::transport(error.to_string())
}

fn truncate(value: &str, max: usize) -> String {
    if value.len() <= max { value.to_string() } else { format!("{}…", &value[..max]) }
}

fn parse_s3_error(body: &str) -> Option<(String, String)> {
    let doc = roxmltree::Document::parse(body).ok()?;
    let error = doc.descendants().find(|n| n.has_tag_name("Error"))?;
    let code = error.children().find(|n| n.has_tag_name("Code")).and_then(|n| n.text()).unwrap_or("").to_string();
    let message = error.children().find(|n| n.has_tag_name("Message")).and_then(|n| n.text()).unwrap_or("").to_string();
    Some((code, message))
}

fn parse_listing(body: &str) -> Result<ObjectListing, S3Error> {
    let doc = roxmltree::Document::parse(body).map_err(|e| S3Error::transport(e.to_string()))?;
    let mut objects = Vec::new();
    for contents in doc.descendants().filter(|n| n.has_tag_name("Contents")) {
        let text = |tag: &str| contents.children().find(|n| n.has_tag_name(tag)).and_then(|n| n.text()).unwrap_or("").to_string();
        let key = percent_decode(&text("Key"));
        // Directory placeholders carry no data; migration copies real objects.
        if key.ends_with('/') && text("Size") == "0" {
            continue;
        }
        let size = text("Size").parse().unwrap_or(0);
        let etag = text("ETag").trim_matches('"').to_string();
        let last_modified = DateTime::parse_from_rfc3339(&text("LastModified"))
            .map(|t| t.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now());
        objects.push(ObjectMeta { key, size, etag, last_modified, user_meta: Default::default() });
    }
    let truncated = doc.descendants().find(|n| n.has_tag_name("IsTruncated")).and_then(|n| n.text()) == Some("true");
    let next_token = if truncated {
        doc.descendants()
            .find(|n| n.has_tag_name("NextContinuationToken"))
            .and_then(|n| n.text())
            .map(percent_decode)
    } else {
        None
    };
    Ok(ObjectListing { objects, next_token })
}

fn percent_decode(value: &str) -> String {
    percent_encoding::percent_decode_str(value).decode_utf8_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_listing_with_encoded_keys_and_token() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<ListBucketResult>
  <IsTruncated>true</IsTruncated>
  <NextContinuationToken>abc%2Fdef</NextContinuationToken>
  <Contents>
    <Key>docs%2Fa%20b.txt</Key>
    <Size>12</Size>
    <ETag>"d41d8cd98f00b204e9800998ecf8427e"</ETag>
    <LastModified>2026-01-02T03:04:05.000Z</LastModified>
  </Contents>
  <Contents>
    <Key>empty-dir/</Key>
    <Size>0</Size>
    <ETag>"d41d8cd98f00b204e9800998ecf8427e"</ETag>
    <LastModified>2026-01-02T03:04:05.000Z</LastModified>
  </Contents>
</ListBucketResult>"#;
        let listing = parse_listing(xml).unwrap();
        assert_eq!(listing.objects.len(), 1);
        assert_eq!(listing.objects[0].key, "docs/a b.txt");
        assert_eq!(listing.objects[0].size, 12);
        assert_eq!(listing.next_token.as_deref(), Some("abc/def"));
    }

    #[test]
    fn last_page_has_no_token() {
        let xml = r#"<ListBucketResult><IsTruncated>false</IsTruncated></ListBucketResult>"#;
        assert!(parse_listing(xml).unwrap().next_token.is_none());
    }

    #[test]
    fn parses_error_body() {
        let xml = r#"<Error><Code>AccessDenied</Code><Message>nope</Message></Error>"#;
        let (code, message) = parse_s3_error(xml).unwrap();
        assert_eq!(code, "AccessDenied");
        assert_eq!(message, "nope");
    }

    #[test]
    fn retryable_classification() {
        assert!(S3Error::transport("connection reset").retryable());
        assert!(S3Error { status: Some(503), code: "SlowDown".into(), message: String::new() }.retryable());
        assert!(S3Error { status: Some(429), code: "SlowDown".into(), message: String::new() }.retryable());
        assert!(!S3Error { status: Some(403), code: "AccessDenied".into(), message: String::new() }.retryable());
        assert!(!S3Error { status: Some(404), code: "NoSuchKey".into(), message: String::new() }.retryable());
    }

    #[test]
    fn path_style_and_virtual_urls() {
        let config = S3Config {
            endpoint: "http://127.0.0.1:9000".into(),
            region: "us-east-1".into(),
            access_key: "ak".into(),
            secret_key: "sk".into(),
            path_style: true,
            insecure_tls: false,
        };
        let client = S3Client::new(config.clone()).unwrap();
        assert_eq!(client.object_url("b", "a b/c.txt"), "http://127.0.0.1:9000/b/a%20b/c.txt");
        let virtual_client = S3Client::new(S3Config { path_style: false, ..config }).unwrap();
        assert_eq!(virtual_client.object_url("b", "k"), "http://b.127.0.0.1:9000/k");
    }
}