use std::{path::Path, time::Duration};

use crate::{Assertion, Capture, Error, Header, HttpRequest, Result};

pub fn parse_document(path: &Path, input: &str) -> Result<Vec<HttpRequest>> {
    let mut requests = Vec::new();
    let mut section = Vec::new();
    let mut start_line = 1;

    for (index, line) in input.lines().enumerate() {
        if line.trim_start().starts_with("###") {
            if section.iter().any(|line: &&str| !line.trim().is_empty()) {
                while section
                    .last()
                    .is_some_and(|line: &&str| line.trim().is_empty())
                {
                    section.pop();
                }
                requests.push(parse_section(path, start_line, &section)?);
            }
            section.clear();
            start_line = index + 2;
        } else {
            section.push(line);
        }
    }

    if section.iter().any(|line| !line.trim().is_empty()) {
        while section
            .last()
            .is_some_and(|line: &&str| line.trim().is_empty())
        {
            section.pop();
        }
        requests.push(parse_section(path, start_line, &section)?);
    }

    if requests.is_empty() {
        return Err(Error::Parse {
            path: path.to_path_buf(),
            message: "document contains no requests".into(),
        });
    }

    Ok(requests)
}

fn parse_section(path: &Path, start_line: usize, lines: &[&str]) -> Result<HttpRequest> {
    let mut name = None;
    let mut timeout = None;
    let mut tags = Vec::new();
    let mut depends_on = Vec::new();
    let mut assertions = Vec::new();
    let mut captures = Vec::new();
    let mut disabled = false;
    let mut request_line = None;

    for (offset, raw) in lines.iter().enumerate() {
        let line = raw.trim();
        if let Some(value) = directive(line, "name") {
            name = Some(value.to_owned());
        } else if let Some(value) = directive(line, "timeout") {
            timeout = Some(parse_duration(path, start_line + offset, value)?);
        } else if let Some(value) = directive(line, "tag") {
            tags.extend(
                value
                    .split(',')
                    .map(str::trim)
                    .filter(|v| !v.is_empty())
                    .map(str::to_owned),
            );
        } else if let Some(value) = directive(line, "depends-on") {
            depends_on.extend(
                value
                    .split(',')
                    .map(str::trim)
                    .filter(|v| !v.is_empty())
                    .map(str::to_owned),
            );
        } else if let Some(value) = directive(line, "assert") {
            assertions.push(parse_assertion(path, start_line + offset, value)?);
        } else if let Some(value) = directive(line, "capture") {
            captures.push(parse_capture(path, start_line + offset, value)?);
        } else if line == "# @disabled" || line == "// @disabled" {
            disabled = true;
        } else if !line.is_empty() && !line.starts_with('#') && !line.starts_with("//") {
            request_line = Some(offset);
            break;
        }
    }

    let request_index = request_line.ok_or_else(|| Error::Parse {
        path: path.to_path_buf(),
        message: format!("line {start_line}: missing METHOD URL request line"),
    })?;
    let parts: Vec<_> = lines[request_index].split_whitespace().collect();
    if parts.len() < 2 {
        return Err(Error::Parse {
            path: path.to_path_buf(),
            message: format!("line {}: expected METHOD URL", start_line + request_index),
        });
    }
    let method = parts[0].to_ascii_uppercase();
    let url = parts[1].to_owned();
    let mut headers = Vec::new();
    let mut body_start = None;

    for (offset, raw) in lines.iter().enumerate().skip(request_index + 1) {
        if raw.trim().is_empty() {
            body_start = Some(offset + 1);
            break;
        }
        let Some((header_name, value)) = raw.split_once(':') else {
            return Err(Error::Parse {
                path: path.to_path_buf(),
                message: format!("line {}: invalid header", start_line + offset),
            });
        };
        headers.push(Header {
            name: header_name.trim().into(),
            value: value.trim().into(),
        });
    }

    let body = body_start
        .map(|index| lines[index..].join("\n"))
        .unwrap_or_default();
    let fallback_name = format!("{}-{}", method.to_ascii_lowercase(), requestsafe_name(&url));

    Ok(HttpRequest {
        name: name.unwrap_or(fallback_name),
        method,
        url,
        headers,
        body,
        source: path.to_path_buf(),
        line: start_line + request_index,
        timeout,
        tags,
        depends_on,
        assertions,
        captures,
        disabled,
    })
}

fn directive<'a>(line: &'a str, name: &str) -> Option<&'a str> {
    let line = line
        .strip_prefix('#')
        .or_else(|| line.strip_prefix("//"))?
        .trim();
    line.strip_prefix(&format!("@{name}"))
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn parse_duration(path: &Path, line: usize, value: &str) -> Result<Duration> {
    let duration = if let Some(value) = value.strip_suffix("ms") {
        value.parse::<u64>().ok().map(Duration::from_millis)
    } else if let Some(value) = value.strip_suffix('s') {
        value.parse::<u64>().ok().map(Duration::from_secs)
    } else {
        value.parse::<u64>().ok().map(Duration::from_secs)
    };
    duration.ok_or_else(|| Error::Parse {
        path: path.to_path_buf(),
        message: format!("line {line}: invalid timeout `{value}`"),
    })
}

fn parse_assertion(path: &Path, line: usize, value: &str) -> Result<Assertion> {
    if let Some(value) = value.strip_prefix("status == ") {
        return value
            .parse()
            .map(Assertion::StatusEquals)
            .map_err(|_| parse_error(path, line, "invalid status assertion"));
    }
    if let Some(value) = value.strip_prefix("body contains ") {
        return Ok(Assertion::BodyContains(unquote(value)));
    }
    if let Some(value) = value.strip_prefix("header ")
        && let Some((name, expected)) = value.split_once(" contains ")
    {
        return Ok(Assertion::HeaderContains {
            name: name.trim().into(),
            value: unquote(expected),
        });
    }
    if let Some(value) = value.strip_prefix("json ")
        && let Some((json_path, expected)) = value.split_once(" == ")
    {
        let expected = serde_json::from_str(expected)
            .unwrap_or_else(|_| serde_json::Value::String(unquote(expected)));
        return Ok(Assertion::JsonEquals {
            path: json_path.trim().into(),
            expected,
        });
    }
    Err(parse_error(path, line, "unsupported assertion"))
}

fn parse_capture(path: &Path, line: usize, value: &str) -> Result<Capture> {
    let Some((name, expression)) = value.split_once('=') else {
        return Err(parse_error(
            path,
            line,
            "expected capture-name = expression",
        ));
    };
    let name = name.trim().to_owned();
    let expression = expression.trim();
    if let Some(path) = call_argument(expression, "jsonpath") {
        return Ok(Capture::JsonPath {
            name,
            path: unquote(path),
        });
    }
    if let Some(header) = call_argument(expression, "header") {
        return Ok(Capture::Header {
            name,
            header: unquote(header),
        });
    }
    Err(parse_error(path, line, "unsupported capture expression"))
}

fn call_argument<'a>(value: &'a str, function: &str) -> Option<&'a str> {
    value
        .strip_prefix(function)?
        .strip_prefix('(')?
        .strip_suffix(')')
        .map(str::trim)
}

fn unquote(value: &str) -> String {
    value
        .trim()
        .trim_matches(|character| character == '"' || character == '\'')
        .into()
}

fn parse_error(path: &Path, line: usize, message: &str) -> Error {
    Error::Parse {
        path: path.to_path_buf(),
        message: format!("line {line}: {message}"),
    }
}

fn requestsafe_name(url: &str) -> String {
    url.chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|part| !part.is_empty())
        .take(4)
        .collect::<Vec<_>>()
        .join("-")
}

pub fn format_document(requests: &[HttpRequest]) -> String {
    let mut output = String::new();
    for (index, request) in requests.iter().enumerate() {
        if index > 0 {
            output.push_str("\n###\n\n");
        }
        output.push_str(&format!("# @name {}\n", request.name));
        if request.disabled {
            output.push_str("# @disabled\n");
        }
        if let Some(timeout) = request.timeout {
            output.push_str(&format!("# @timeout {}ms\n", timeout.as_millis()));
        }
        if !request.tags.is_empty() {
            output.push_str(&format!("# @tag {}\n", request.tags.join(", ")));
        }
        if !request.depends_on.is_empty() {
            output.push_str(&format!(
                "# @depends-on {}\n",
                request.depends_on.join(", ")
            ));
        }
        for assertion in &request.assertions {
            output.push_str("# @assert ");
            match assertion {
                Assertion::StatusEquals(status) => output.push_str(&format!("status == {status}")),
                Assertion::HeaderContains { name, value } => {
                    output.push_str(&format!("header {name} contains {value:?}"));
                }
                Assertion::BodyContains(value) => {
                    output.push_str(&format!("body contains {value:?}"));
                }
                Assertion::JsonEquals { path, expected } => {
                    output.push_str(&format!("json {path} == {expected}"));
                }
            }
            output.push('\n');
        }
        for capture in &request.captures {
            match capture {
                Capture::JsonPath { name, path } => {
                    output.push_str(&format!("# @capture {name} = jsonpath({path:?})\n"));
                }
                Capture::Header { name, header } => {
                    output.push_str(&format!("# @capture {name} = header({header:?})\n"));
                }
            }
        }
        output.push_str(&format!("{} {}\n", request.method, request.url));
        for header in &request.headers {
            output.push_str(&format!("{}: {}\n", header.name, header.value));
        }
        if !request.body.is_empty() {
            output.push('\n');
            output.push_str(&request.body);
            if !output.ends_with('\n') {
                output.push('\n');
            }
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_directives_and_multiple_requests() {
        let input = r#"# @name login
# @timeout 2s
# @assert status == 200
# @capture token = jsonpath("$.token")
POST {{base_url}}/login
Content-Type: application/json

{"name":"Ada"}

###

# @name health
# @depends-on login
GET {{base_url}}/health
"#;
        let requests = parse_document(Path::new("example.http"), input).unwrap();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0].name, "login");
        assert_eq!(requests[1].depends_on, ["login"]);
        assert_eq!(requests[0].body, r#"{"name":"Ada"}"#);
    }

    #[test]
    fn formatting_round_trips() {
        let input = "# @name ping\n# @assert status == 204\nGET https://example.com\n";
        let requests = parse_document(Path::new("ping.http"), input).unwrap();
        let formatted = format_document(&requests);
        assert_eq!(
            parse_document(Path::new("ping.http"), &formatted).unwrap(),
            requests
        );
    }
}
