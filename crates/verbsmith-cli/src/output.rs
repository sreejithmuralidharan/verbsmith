use std::io::{self, Write};

use anyhow::Result;
use owo_colors::OwoColorize;
use serde_json::{Value, json};
use verbsmith_core::RunResult;

use crate::FormatArg;

pub fn render(results: &[RunResult], format: FormatArg, redactions: &[String]) -> Result<()> {
    match format {
        FormatArg::Pretty => pretty(results, redactions),
        FormatArg::Raw => raw(results, redactions),
        FormatArg::Json => println!(
            "{}",
            serde_json::to_string_pretty(
                &results
                    .iter()
                    .map(|result| structured(result, redactions))
                    .collect::<Vec<_>>()
            )?
        ),
        FormatArg::Jsonl => {
            for result in results {
                println!(
                    "{}",
                    serde_json::to_string(&structured(result, redactions))?
                );
            }
        }
        FormatArg::Junit => junit(results),
        FormatArg::Sarif => sarif(results)?,
    }
    Ok(())
}

fn structured(result: &RunResult, redactions: &[String]) -> Value {
    let body = match std::str::from_utf8(&result.response.body) {
        Ok(body) => json!({"encoding": "utf8", "data": redact(body, redactions)}),
        Err(_) => json!({"encoding": "byte-array", "data": result.response.body}),
    };
    let headers = result
        .response
        .headers
        .iter()
        .map(|header| {
            json!({
                "name": header.name,
                "value": redact(&header.value, redactions),
            })
        })
        .collect::<Vec<_>>();
    json!({
        "request": {
            "name": result.request.name,
            "method": result.request.method,
            "url": result.request.url,
            "source": result.request.source,
            "line": result.request.line,
        },
        "response": {
            "status": result.response.status,
            "headers": headers,
            "body": body,
            "elapsed_ms": result.response.elapsed_ms,
            "effective_url": redact(&result.response.effective_url, redactions),
        },
        "assertions": result.assertions,
        "captures": result.captures.keys().map(|key| (key, "[REDACTED]")).collect::<std::collections::BTreeMap<_, _>>(),
    })
}

fn pretty(results: &[RunResult], redactions: &[String]) {
    for (index, result) in results.iter().enumerate() {
        if index > 0 {
            println!();
        }
        let status = if result.response.status < 400 {
            result.response.status.to_string().green().to_string()
        } else {
            result.response.status.to_string().red().to_string()
        };
        println!(
            "{}  {}  {} ms",
            result.request.name.bold(),
            status,
            result.response.elapsed_ms
        );
        println!(
            "{}",
            redact(&result.response.effective_url, redactions).dimmed()
        );
        for header in &result.response.headers {
            println!(
                "{}: {}",
                header.name.cyan(),
                redact(&header.value, redactions)
            );
        }
        if !result.response.body.is_empty() {
            println!();
            if let Ok(json) = serde_json::from_slice::<serde_json::Value>(&result.response.body) {
                let json = serde_json::to_string_pretty(&json).unwrap_or_default();
                println!("{}", redact(&json, redactions));
            } else {
                println!(
                    "{}",
                    redact(&String::from_utf8_lossy(&result.response.body), redactions)
                );
            }
        }
        for assertion in &result.assertions {
            let mark = if assertion.passed {
                "PASS".green().to_string()
            } else {
                "FAIL".red().to_string()
            };
            println!("{mark} {} ({})", assertion.description, assertion.detail);
        }
        for name in result.captures.keys() {
            println!("{} {} = [REDACTED]", "CAPTURE".cyan(), name);
        }
    }
}

fn raw(results: &[RunResult], redactions: &[String]) {
    for result in results {
        print!(
            "{}",
            redact(&String::from_utf8_lossy(&result.response.body), redactions)
        );
    }
}

fn junit(results: &[RunResult]) {
    let tests = results
        .iter()
        .map(|result| result.assertions.len())
        .sum::<usize>();
    let failures = results
        .iter()
        .flat_map(|result| &result.assertions)
        .filter(|assertion| !assertion.passed)
        .count();
    println!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>");
    println!("<testsuite name=\"verbsmith\" tests=\"{tests}\" failures=\"{failures}\">");
    for result in results {
        for assertion in &result.assertions {
            println!(
                "  <testcase classname=\"{}\" name=\"{}\">",
                xml(&result.request.name),
                xml(&assertion.description)
            );
            if !assertion.passed {
                println!("    <failure message=\"{}\" />", xml(&assertion.detail));
            }
            println!("  </testcase>");
        }
    }
    println!("</testsuite>");
}

fn sarif(results: &[RunResult]) -> Result<()> {
    let failures = results
        .iter()
        .flat_map(|result| result.assertions.iter().filter(|assertion| !assertion.passed).map(move |assertion| (result, assertion)))
        .map(|(result, assertion)| json!({
            "ruleId": "verbsmith.assertion",
            "level": "error",
            "message": {"text": format!("{}: {}", assertion.description, assertion.detail)},
            "locations": [{"physicalLocation": {"artifactLocation": {"uri": result.request.source}, "region": {"startLine": result.request.line}}}]
        }))
        .collect::<Vec<_>>();
    let report = json!({
        "version": "2.1.0",
        "$schema": "https://json.schemastore.org/sarif-2.1.0.json",
        "runs": [{"tool": {"driver": {"name": "Verbsmith", "version": env!("CARGO_PKG_VERSION")}}, "results": failures}]
    });
    serde_json::to_writer_pretty(io::stdout().lock(), &report)?;
    writeln!(io::stdout().lock())?;
    Ok(())
}

fn xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn redact(value: &str, redactions: &[String]) -> String {
    redactions
        .iter()
        .filter(|secret| !secret.is_empty())
        .fold(value.to_owned(), |text, secret| {
            text.replace(secret, "[REDACTED]")
        })
}
