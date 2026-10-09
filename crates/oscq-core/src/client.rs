//! HTTP client for OSCQuery servers: tree, `HOST_INFO` and per attribute queries.

use std::time::Duration;

use reqwest::StatusCode;
use serde_json::Value;
use url::Url;

use crate::document::{Diagnostic, DocumentError, ParseReport, parse_document, parse_json};
use crate::host_info::HostInfo;

/// Response bodies above this size are refused.
pub const DEFAULT_MAX_BODY: usize = 64 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct ClientConfig {
    pub connect_timeout: Duration,
    /// Whole request, body included.
    pub request_timeout: Duration,
    pub max_body: usize,
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            connect_timeout: Duration::from_secs(3),
            request_timeout: Duration::from_secs(10),
            max_body: DEFAULT_MAX_BODY,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("not an http URL with a host and port: {0}")]
    InvalidBaseUrl(String),
    #[error("OSC path {0:?} cannot be put in a URL")]
    InvalidPath(String),
    #[error("could not connect to {url}: {source}")]
    Connect { url: String, source: reqwest::Error },
    #[error("no answer from {url} within the timeout")]
    Timeout { url: String },
    #[error("{url} answered HTTP {status}")]
    Status { url: String, status: u16 },
    #[error("{url} answered 204 with no document")]
    NoDocument { url: String },
    #[error("response from {url} is larger than {limit} bytes")]
    BodyTooLarge { url: String, limit: usize },
    #[error("request to {url} failed: {source}")]
    Transport { url: String, source: reqwest::Error },
    #[error("invalid response from {url}: {source}")]
    Document { url: String, source: DocumentError },
    #[error("could not build the HTTP client: {0}")]
    Build(reqwest::Error),
}

/// A fetched address space document: the body exactly as received, and its parsed model.
#[derive(Debug, Clone)]
pub struct Document {
    pub raw: String,
    pub report: ParseReport,
}

/// The reply to a single attribute query, `/path?ATTR`.
#[derive(Debug, Clone, PartialEq)]
pub enum AttributeReply {
    /// 200 with the attribute: the value stored under the attribute key.
    Value(Value),
    /// 200 with an empty object: the attribute is unsupported or absent for that node.
    Unsupported,
    /// 204: valid request, no content, for example `VALUE` on a node that is not readable.
    NoContent,
}

#[derive(Debug, Clone)]
pub struct OscQueryClient {
    http: reqwest::Client,
    base: Url,
    host: String,
    port: u16,
    config: ClientConfig,
}

impl OscQueryClient {
    pub fn new(base: Url) -> Result<Self, ClientError> {
        Self::with_config(base, ClientConfig::default())
    }

    /// `base` is the server's HTTP address, `http://host:port`. Any path on it is ignored.
    pub fn with_config(base: Url, config: ClientConfig) -> Result<Self, ClientError> {
        if base.scheme() != "http" {
            return Err(ClientError::InvalidBaseUrl(base.to_string()));
        }
        let host = match base.host() {
            Some(url::Host::Ipv6(a)) => a.to_string(),
            Some(h) => h.to_string(),
            None => return Err(ClientError::InvalidBaseUrl(base.to_string())),
        };
        let port = base
            .port_or_known_default()
            .ok_or_else(|| ClientError::InvalidBaseUrl(base.to_string()))?;
        // OSCQuery servers are on the LAN: a system proxy would only misroute requests.
        let http = reqwest::Client::builder()
            .no_proxy()
            .connect_timeout(config.connect_timeout)
            .timeout(config.request_timeout)
            .build()
            .map_err(ClientError::Build)?;
        Ok(Self {
            http,
            base,
            host,
            port,
            config,
        })
    }

    pub fn base(&self) -> &Url {
        &self.base
    }

    /// HTTP host, without brackets for IPv6.
    pub fn http_host(&self) -> &str {
        &self.host
    }

    pub fn http_port(&self) -> u16 {
        self.port
    }

    /// The URL for an OSC path and an optional attribute query.
    pub fn url_for(&self, path: &str, attribute: Option<&str>) -> Result<Url, ClientError> {
        let mut url = self.base.clone();
        url.set_fragment(None);
        url.set_path("/");
        if let Some(rest) = path.strip_prefix('/') {
            if !rest.is_empty() {
                let mut segments = url
                    .path_segments_mut()
                    .map_err(|_| ClientError::InvalidPath(path.to_owned()))?;
                segments.clear();
                segments.extend(rest.split('/'));
            }
        } else {
            return Err(ClientError::InvalidPath(path.to_owned()));
        }
        url.set_query(attribute);
        Ok(url)
    }

    /// GET on a path: the node and all its descendants.
    pub async fn fetch_tree(&self, path: &str) -> Result<Document, ClientError> {
        let url = self.url_for(path, None)?;
        let (status, body) = self.get(&url).await?;
        if status == StatusCode::NO_CONTENT {
            return Err(ClientError::NoDocument {
                url: url.to_string(),
            });
        }
        let report = parse_document(&body).map_err(|source| ClientError::Document {
            url: url.to_string(),
            source,
        })?;
        Ok(Document {
            raw: String::from_utf8_lossy(&body).into_owned(),
            report,
        })
    }

    /// `HOST_INFO` query on the root.
    pub async fn fetch_host_info(&self) -> Result<(HostInfo, Vec<Diagnostic>), ClientError> {
        let url = self.url_for("/", Some("HOST_INFO"))?;
        let (status, body) = self.get(&url).await?;
        if status == StatusCode::NO_CONTENT {
            return Err(ClientError::NoDocument {
                url: url.to_string(),
            });
        }
        HostInfo::parse(&body).map_err(|source| ClientError::Document {
            url: url.to_string(),
            source,
        })
    }

    /// Single attribute query, `/path?ATTR`. 200 and 204 are both success.
    pub async fn fetch_attribute(
        &self,
        path: &str,
        attribute: &str,
    ) -> Result<AttributeReply, ClientError> {
        let url = self.url_for(path, Some(attribute))?;
        let (status, body) = self.get(&url).await?;
        if status == StatusCode::NO_CONTENT || body.iter().all(u8::is_ascii_whitespace) {
            return Ok(AttributeReply::NoContent);
        }
        let doc_err = |source| ClientError::Document {
            url: url.to_string(),
            source,
        };
        let Value::Object(mut obj) = parse_json(&body).map_err(doc_err)? else {
            return Err(doc_err(DocumentError::NotAnObject));
        };
        // A server that ignores the query and returns other keys is treated like an empty object.
        Ok(match obj.remove(attribute) {
            Some(v) => AttributeReply::Value(v),
            None => AttributeReply::Unsupported,
        })
    }

    /// GET with status and size checks. Returns the status, 200 or 204, and the body.
    async fn get(&self, url: &Url) -> Result<(StatusCode, Vec<u8>), ClientError> {
        let u = url.to_string();
        let classify = |e: reqwest::Error| {
            if e.is_timeout() {
                ClientError::Timeout { url: u.clone() }
            } else if e.is_connect() {
                ClientError::Connect {
                    url: u.clone(),
                    source: e,
                }
            } else {
                ClientError::Transport {
                    url: u.clone(),
                    source: e,
                }
            }
        };
        let mut resp = self.http.get(url.clone()).send().await.map_err(classify)?;
        let status = resp.status();
        if status != StatusCode::OK && status != StatusCode::NO_CONTENT {
            return Err(ClientError::Status {
                url: u,
                status: status.as_u16(),
            });
        }
        let limit = self.config.max_body;
        let too_large = || ClientError::BodyTooLarge {
            url: u.clone(),
            limit,
        };
        if resp.content_length().is_some_and(|n| n > limit as u64) {
            return Err(too_large());
        }
        let mut body = Vec::new();
        while let Some(chunk) = resp.chunk().await.map_err(classify)? {
            if body.len() + chunk.len() > limit {
                return Err(too_large());
            }
            body.extend_from_slice(&chunk);
        }
        Ok((status, body))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls() {
        let c = OscQueryClient::new(Url::parse("http://10.0.0.2:5678/ignored#x").unwrap()).unwrap();
        assert_eq!(c.http_host(), "10.0.0.2");
        assert_eq!(c.http_port(), 5678);
        assert_eq!(
            c.url_for("/", Some("HOST_INFO")).unwrap().as_str(),
            "http://10.0.0.2:5678/?HOST_INFO"
        );
        assert_eq!(
            c.url_for("/layer1/opacity", Some("VALUE"))
                .unwrap()
                .as_str(),
            "http://10.0.0.2:5678/layer1/opacity?VALUE"
        );
        assert_eq!(
            c.url_for("/a b/c#d", None).unwrap().as_str(),
            "http://10.0.0.2:5678/a%20b/c%23d"
        );
        assert!(c.url_for("relative", None).is_err());
    }

    #[test]
    fn ipv6_host() {
        let c = OscQueryClient::new(Url::parse("http://[::1]:8080").unwrap()).unwrap();
        assert_eq!(c.http_host(), "::1");
        assert_eq!(c.http_port(), 8080);
    }

    #[test]
    fn rejects_non_http() {
        assert!(OscQueryClient::new(Url::parse("ws://h:1").unwrap()).is_err());
    }
}
