use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

use curl::easy::{Easy, List};
use indexmap::IndexMap;
use regex::Regex;

use crate::{
    Assertion, AssertionResult, Capture, Error, Header, HttpRequest, HttpResponse, Result,
};

#[derive(Debug, Clone)]
pub struct ExecutionOptions {
    pub follow_redirects: bool,
    pub insecure: bool,
    pub timeout: Duration,
    pub proxy: Option<String>,
    pub variables: BTreeMap<String, String>,
    pub max_response_bytes: usize,
    pub retries: u32,
    pub retry_non_idempotent: bool,
}

impl Default for ExecutionOptions {
    fn default() -> Self {
        Self {
            follow_redirects: true,
            insecure: false,
            timeout: Duration::from_secs(30),
            proxy: None,
            variables: BTreeMap::new(),
            max_response_bytes: 64 * 1024 * 1024,
            retries: 0,
            retry_non_idempotent: false,
        }
    }
}

pub fn execute(request: &HttpRequest, options: &ExecutionOptions) -> Result<HttpResponse> {
    let may_retry = options.retry_non_idempotent || is_idempotent(&request.method);
    let retries = if may_retry { options.retries } else { 0 };
    let mut attempt = 0;
    loop {
        match execute_once(request, options) {
            Ok(response) if attempt < retries && retryable_status(response.status) => {
                std::thread::sleep(retry_delay(attempt));
            }
            Ok(response) => return Ok(response),
            Err(error @ Error::Transport(_)) if attempt < retries => {
                tracing::debug!(attempt, error = %error, "retrying request after transport failure");
                std::thread::sleep(retry_delay(attempt));
            }
            Err(error) => return Err(error),
        }
        attempt += 1;
    }
}

fn execute_once(request: &HttpRequest, options: &ExecutionOptions) -> Result<HttpResponse> {
    let url = resolve_template(&request.url, &options.variables)?;
    url::Url::parse(&url).map_err(|_| Error::InvalidUrl(url.clone()))?;

    let mut handle = Easy::new();
    handle.url(&url).map_err(transport)?;
    handle.custom_request(&request.method).map_err(transport)?;
    handle
        .follow_location(options.follow_redirects)
        .map_err(transport)?;
    handle
        .ssl_verify_peer(!options.insecure)
        .map_err(transport)?;
    handle
        .ssl_verify_host(!options.insecure)
        .map_err(transport)?;
    handle
        .timeout(request.timeout.unwrap_or(options.timeout))
        .map_err(transport)?;
    handle
        .useragent(concat!("verbsmith/", env!("CARGO_PKG_VERSION")))
        .map_err(transport)?;
    handle.accept_encoding("").map_err(transport)?;
    if let Some(proxy) = &options.proxy {
        handle.proxy(proxy).map_err(transport)?;
    }

    let mut headers = List::new();
    for header in &request.headers {
        let value = resolve_template(&header.value, &options.variables)?;
        headers
            .append(&format!("{}: {value}", header.name))
            .map_err(transport)?;
    }
    if !request.headers.is_empty() {
        handle.http_headers(headers).map_err(transport)?;
    }

    if !request.body.is_empty() {
        let body = resolve_template(&request.body, &options.variables)?;
        handle
            .post_fields_copy(body.as_bytes())
            .map_err(transport)?;
    }

    let started = Instant::now();
    let mut body = Vec::new();
    let mut response_headers = Vec::new();
    let mut response_too_large = false;
    {
        let mut transfer = handle.transfer();
        transfer
            .write_function(|data| {
                if body.len().saturating_add(data.len()) > options.max_response_bytes {
                    response_too_large = true;
                    return Ok(0);
                }
                body.extend_from_slice(data);
                Ok(data.len())
            })
            .map_err(transport)?;
        transfer
            .header_function(|line| {
                if let Ok(line) = std::str::from_utf8(line)
                    && let Some((name, value)) = line.split_once(':')
                {
                    response_headers.push(Header {
                        name: name.trim().into(),
                        value: value.trim().into(),
                    });
                }
                true
            })
            .map_err(transport)?;
        let result = transfer.perform();
        drop(transfer);
        if response_too_large {
            return Err(Error::ResponseTooLarge {
                limit: options.max_response_bytes,
            });
        }
        result.map_err(transport)?;
    }
    let status = handle.response_code().map_err(transport)?;
    let effective_url = handle
        .effective_url()
        .map_err(transport)?
        .unwrap_or(&url)
        .to_owned();

    Ok(HttpResponse {
        status,
        headers: response_headers,
        body,
        elapsed_ms: started.elapsed().as_millis(),
        effective_url,
    })
}

fn is_idempotent(method: &str) -> bool {
    matches!(
        method,
        "GET" | "HEAD" | "PUT" | "DELETE" | "OPTIONS" | "TRACE"
    )
}

fn retryable_status(status: u32) -> bool {
    status == 408 || status == 429 || (500..=599).contains(&status)
}

fn retry_delay(attempt: u32) -> Duration {
    Duration::from_millis(
        100_u64
            .saturating_mul(2_u64.saturating_pow(attempt))
            .min(2_000),
    )
}

pub fn resolve_template(input: &str, variables: &BTreeMap<String, String>) -> Result<String> {
    let pattern = Regex::new(r"\{\{\s*([A-Za-z_][A-Za-z0-9_.-]*)\s*\}\}").expect("valid regex");
    let mut unresolved = None;
    let resolved = pattern
        .replace_all(input, |captures: &regex::Captures<'_>| {
            let name = &captures[1];
            variables.get(name).cloned().unwrap_or_else(|| {
                unresolved = Some(name.to_owned());
                captures[0].to_owned()
            })
        })
        .into_owned();
    if let Some(name) = unresolved {
        Err(Error::UnresolvedVariable(name))
    } else {
        Ok(resolved)
    }
}

pub fn evaluate_assertions(request: &HttpRequest, response: &HttpResponse) -> Vec<AssertionResult> {
    request
        .assertions
        .iter()
        .map(|assertion| match assertion {
            Assertion::StatusEquals(expected) => AssertionResult {
                description: format!("status == {expected}"),
                passed: response.status == *expected,
                detail: format!("received {}", response.status),
            },
            Assertion::HeaderContains { name, value } => {
                let actual = response
                    .headers
                    .iter()
                    .filter(|header| header.name.eq_ignore_ascii_case(name))
                    .map(|header| header.value.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                AssertionResult {
                    description: format!("header {name} contains {value:?}"),
                    passed: actual.contains(value),
                    detail: format!("received {actual:?}"),
                }
            }
            Assertion::BodyContains(expected) => {
                let body = String::from_utf8_lossy(&response.body);
                AssertionResult {
                    description: format!("body contains {expected:?}"),
                    passed: body.contains(expected),
                    detail: format!("body length {} bytes", response.body.len()),
                }
            }
            Assertion::JsonEquals { path, expected } => {
                let document = serde_json::from_slice::<serde_json::Value>(&response.body).ok();
                let actual = document.as_ref().and_then(|value| json_path(value, path));
                AssertionResult {
                    description: format!("json {path} == {expected}"),
                    passed: actual == Some(expected),
                    detail: actual.map_or_else(|| "path not found".into(), ToString::to_string),
                }
            }
        })
        .collect()
}

pub fn extract_captures(
    request: &HttpRequest,
    response: &HttpResponse,
) -> IndexMap<String, String> {
    request
        .captures
        .iter()
        .filter_map(|capture| match capture {
            Capture::Header { name, header } => response
                .headers
                .iter()
                .find(|candidate| candidate.name.eq_ignore_ascii_case(header))
                .map(|value| (name.clone(), value.value.clone())),
            Capture::JsonPath { name, path } => {
                serde_json::from_slice::<serde_json::Value>(&response.body)
                    .ok()
                    .and_then(|document| json_path(&document, path).cloned())
                    .map(|value| {
                        let value = value
                            .as_str()
                            .map(str::to_owned)
                            .unwrap_or_else(|| value.to_string());
                        (name.clone(), value)
                    })
            }
        })
        .collect()
}

fn json_path<'a>(document: &'a serde_json::Value, path: &str) -> Option<&'a serde_json::Value> {
    let path = path
        .strip_prefix("$.")
        .or_else(|| path.strip_prefix('$'))
        .unwrap_or(path);
    if path.is_empty() {
        return Some(document);
    }
    path.split('.').try_fold(document, |current, segment| {
        if let Some((field, index)) = segment.split_once('[') {
            let index = index.strip_suffix(']')?.parse::<usize>().ok()?;
            current.get(field)?.get(index)
        } else {
            current.get(segment)
        }
    })
}

fn transport(error: curl::Error) -> Error {
    Error::Transport(error.to_string())
}

#[cfg(test)]
mod tests {
    use std::{
        io::{Read, Write},
        net::TcpListener,
        thread,
    };

    use super::*;

    #[test]
    fn resolves_variables_and_rejects_missing_values() {
        let variables = BTreeMap::from([("host".into(), "example.com".into())]);
        assert_eq!(
            resolve_template("https://{{ host }}/v1", &variables).unwrap(),
            "https://example.com/v1"
        );
        assert!(matches!(
            resolve_template("{{missing}}", &variables),
            Err(Error::UnresolvedVariable(_))
        ));
    }

    #[test]
    fn evaluates_nested_json_paths() {
        let value = serde_json::json!({"items": [{"id": 42}]});
        assert_eq!(
            json_path(&value, "$.items[0].id"),
            Some(&serde_json::json!(42))
        );
    }

    #[test]
    fn executes_against_a_local_http_server() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0_u8; 2048];
            let size = stream.read(&mut request).unwrap();
            assert!(String::from_utf8_lossy(&request[..size]).starts_with("GET /health HTTP/1.1"));
            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 11\r\nConnection: close\r\n\r\n{\"ok\":true}",
                )
                .unwrap();
        });
        let request = HttpRequest {
            name: "health".into(),
            method: "GET".into(),
            url: format!("http://{address}/health"),
            headers: Vec::new(),
            body: String::new(),
            source: "test.http".into(),
            line: 1,
            timeout: None,
            tags: Vec::new(),
            depends_on: Vec::new(),
            assertions: vec![Assertion::JsonEquals {
                path: "$.ok".into(),
                expected: serde_json::json!(true),
            }],
            captures: Vec::new(),
            disabled: false,
        };
        let response = execute(&request, &ExecutionOptions::default()).unwrap();
        server.join().unwrap();
        assert_eq!(response.status, 200);
        assert!(evaluate_assertions(&request, &response)[0].passed);
    }

    #[test]
    fn retries_idempotent_server_errors() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            for status in [500, 200] {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = [0_u8; 1024];
                let _bytes_read = stream.read(&mut request).unwrap();
                let response = format!(
                    "HTTP/1.1 {status} Test\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                );
                stream.write_all(response.as_bytes()).unwrap();
            }
        });
        let request = HttpRequest {
            name: "retry".into(),
            method: "GET".into(),
            url: format!("http://{address}/"),
            headers: Vec::new(),
            body: String::new(),
            source: "test.http".into(),
            line: 1,
            timeout: None,
            tags: Vec::new(),
            depends_on: Vec::new(),
            assertions: Vec::new(),
            captures: Vec::new(),
            disabled: false,
        };
        let response = execute(
            &request,
            &ExecutionOptions {
                retries: 1,
                ..ExecutionOptions::default()
            },
        )
        .unwrap();
        server.join().unwrap();
        assert_eq!(response.status, 200);
    }
}
