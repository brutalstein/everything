use anyhow::{Context, Result};
use serde_json::Value;
use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct HttpRequest {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub form: Vec<(String, String)>,
    pub json_body: Option<Value>,
}

#[derive(Debug, Clone)]
pub struct HttpResponse {
    pub status: u16,
    pub body: Vec<u8>,
    pub truncated: bool,
}

#[derive(Debug, Clone)]
pub struct CurlHttpClient {
    timeout_millis: u64,
    max_response_bytes: usize,
}

impl CurlHttpClient {
    pub fn new(timeout_millis: u64, max_response_bytes: u64) -> Self {
        Self {
            timeout_millis: timeout_millis.clamp(1_000, 10 * 60 * 1000),
            max_response_bytes: usize::try_from(max_response_bytes)
                .unwrap_or(4_000_000)
                .clamp(1_024, 100_000_000),
        }
    }

    pub fn execute(&self, request: &HttpRequest) -> Result<HttpResponse> {
        validate_request(request)?;
        let marker = "\n__EVERYTHING_HTTP_STATUS__:";
        let mut config = String::new();
        config.push_str("silent\nshow-error\n");
        config.push_str("user-agent = \"Everything/0.3 (local-first connector runtime)\"\n");
        config.push_str(&format!(
            "max-time = {}\n",
            self.timeout_millis.div_ceil(1_000)
        ));
        config.push_str(&format!(
            "connect-timeout = {}\n",
            (self.timeout_millis / 3).max(1_000).div_ceil(1_000)
        ));
        config.push_str(&format!("request = {}\n", quote_config(&request.method)));
        config.push_str(&format!("url = {}\n", quote_config(&request.url)));
        config.push_str(&format!(
            "write-out = {}\n",
            quote_config(&format!("{marker}%{{http_code}}"))
        ));
        for (name, value) in &request.headers {
            config.push_str(&format!(
                "header = {}\n",
                quote_config(&format!("{name}: {value}"))
            ));
        }
        for (name, value) in &request.form {
            config.push_str(&format!(
                "data-urlencode = {}\n",
                quote_config(&format!("{name}={value}"))
            ));
        }
        if let Some(body) = &request.json_body {
            if !request
                .headers
                .iter()
                .any(|(name, _)| name.eq_ignore_ascii_case("content-type"))
            {
                config.push_str("header = \"Content-Type: application/json; charset=UTF-8\"\n");
            }
            config.push_str(&format!(
                "data = {}\n",
                quote_config(&serde_json::to_string(body)?)
            ));
        }

        let mut child = Command::new("curl")
            // -q must be the first curl option so user-level curlrc files cannot
            // inject proxies, cookies, headers, or alternate destinations.
            .args([
                "-q",
                "--proto",
                "=https,http",
                "--proto-redir",
                "=https",
                "--config",
                "-",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("start curl HTTP adapter")?;
        child
            .stdin
            .as_mut()
            .context("open curl stdin")?
            .write_all(config.as_bytes())?;
        drop(child.stdin.take());

        let stdout = child.stdout.take().context("capture curl stdout")?;
        let stderr = child.stderr.take().context("capture curl stderr")?;
        let output_limit = self.max_response_bytes.saturating_add(128);
        let stdout_thread = std::thread::spawn(move || read_limited(stdout, output_limit));
        let stderr_thread = std::thread::spawn(move || read_limited(stderr, 256 * 1024));
        let started = Instant::now();
        let timeout = Duration::from_millis(self.timeout_millis.saturating_add(2_000));
        let status = loop {
            if let Some(status) = child.try_wait()? {
                break status;
            }
            if started.elapsed() >= timeout {
                let _ = child.kill();
                let _ = child.wait();
                anyhow::bail!("connector HTTP request timed out");
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        let (stdout, truncated) = stdout_thread.join().unwrap_or_else(|_| (Vec::new(), true));
        let (stderr, _) = stderr_thread.join().unwrap_or_else(|_| (Vec::new(), true));
        anyhow::ensure!(
            status.success(),
            "connector HTTP request failed: {}",
            String::from_utf8_lossy(&stderr).trim()
        );
        let payload = String::from_utf8_lossy(&stdout);
        let position = payload
            .rfind(marker)
            .context("curl response did not include HTTP status")?;
        let body = stdout[..position].to_vec();
        let status_text = payload[position + marker.len()..].trim();
        let http_status = status_text.parse::<u16>().context("parse HTTP status")?;
        Ok(HttpResponse {
            status: http_status,
            body: body.into_iter().take(self.max_response_bytes).collect(),
            truncated: truncated || position > self.max_response_bytes,
        })
    }
}

fn validate_request(request: &HttpRequest) -> Result<()> {
    anyhow::ensure!(
        matches!(
            request.method.as_str(),
            "GET" | "POST" | "PUT" | "PATCH" | "DELETE"
        ),
        "unsupported HTTP method"
    );
    anyhow::ensure!(
        request.url.starts_with("https://") || request.url.starts_with("http://127.0.0.1:"),
        "connector URL must use HTTPS or loopback HTTP"
    );
    for (name, value) in &request.headers {
        anyhow::ensure!(
            !contains_line_break(name) && !contains_line_break(value),
            "HTTP header contains a line break"
        );
    }
    for (name, value) in &request.form {
        anyhow::ensure!(
            !contains_line_break(name) && !value.contains('\0'),
            "HTTP form field is invalid"
        );
    }
    Ok(())
}

fn contains_line_break(value: &str) -> bool {
    value.contains('\r') || value.contains('\n')
}

fn quote_config(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for character in value.chars() {
        match character {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\r' => out.push_str("\\r"),
            '\n' => out.push_str("\\n"),
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

fn read_limited(mut reader: impl Read, limit: usize) -> (Vec<u8>, bool) {
    let mut kept = Vec::with_capacity(limit.min(64 * 1024));
    let mut buffer = [0u8; 8192];
    let mut truncated = false;
    loop {
        let count = match reader.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(count) => count,
        };
        let remaining = limit.saturating_sub(kept.len());
        if remaining > 0 {
            kept.extend_from_slice(&buffer[..count.min(remaining)]);
        }
        if count > remaining {
            truncated = true;
        }
    }
    (kept, truncated)
}

#[cfg(test)]
mod tests {
    use super::{HttpRequest, quote_config, validate_request};

    fn request(url: &str) -> HttpRequest {
        HttpRequest {
            method: "GET".to_owned(),
            url: url.to_owned(),
            headers: Vec::new(),
            form: Vec::new(),
            json_body: None,
        }
    }

    #[test]
    fn remote_plain_http_is_rejected_but_explicit_loopback_is_allowed() {
        assert!(validate_request(&request("http://example.com/data")).is_err());
        assert!(validate_request(&request("http://127.0.0.1:43821/callback")).is_ok());
        assert!(validate_request(&request("https://api.spotify.com/v1/me")).is_ok());
    }

    #[test]
    fn headers_and_form_names_reject_configuration_injection() {
        let mut unsafe_header = request("https://api.spotify.com/v1/me");
        unsafe_header.headers.push((
            "Authorization\nurl=https://evil.invalid".to_owned(),
            "x".to_owned(),
        ));
        assert!(validate_request(&unsafe_header).is_err());

        let mut unsafe_form = request("https://oauth2.googleapis.com/token");
        unsafe_form.form.push((
            "grant_type\nurl=https://evil.invalid".to_owned(),
            "x".to_owned(),
        ));
        assert!(validate_request(&unsafe_form).is_err());
    }

    #[test]
    fn curl_config_values_are_escaped() {
        assert_eq!(quote_config("a\\\"b\nc"), "\"a\\\\\\\"b\\nc\"");
    }
}
