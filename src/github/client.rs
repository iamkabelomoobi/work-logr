use std::time::Duration;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum GitHubError {
    #[error("HTTP request failed: {0}")]
    RequestFailed(String),
    #[error("JSON parsing failed: {0}")]
    JsonError(String),
    #[error("API error")]
    RequestError(#[from] reqwest::Error),
}

/// Maximum number of attempts (initial try + retries) for a single request.
const MAX_ATTEMPTS: u32 = 5;
/// Base delay for the exponential backoff schedule.
const BASE_BACKOFF_MS: u64 = 250;
/// Upper bound for a single backoff sleep.
const MAX_BACKOFF: Duration = Duration::from_secs(5);

pub struct GitHubClient {
    pub client: reqwest::Client,
    pub token: String,
    pub owner: String,
    pub repo: String,
}

impl GitHubClient {
    pub fn new(token: String, owner: String, repo: String) -> Self {
        // GitHub's edge occasionally terminates HTTP/2 streams mid-body
        // (RST_STREAM CANCEL). Pinning HTTP/1.1 avoids that failure class;
        // retries in `get_with_retry` cover the drops that still happen.
        let client = reqwest::Client::builder()
            .http1_only()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .build()
            .expect("failed to build HTTP client");
        GitHubClient {
            client,
            token,
            owner,
            repo,
        }
    }

    pub fn auth_header(&self) -> String {
        format!("token {}", self.token)
    }

    /// Perform an authenticated GET, retrying transient connection failures
    /// (unexpected EOF / connection reset / timeout) and retryable statuses
    /// (5xx, 429) with exponential backoff. Returns the final status and body.
    pub async fn get_with_retry(
        &self,
        url: &str,
        query: &[(&str, &str)],
    ) -> Result<(reqwest::StatusCode, String), GitHubError> {
        let mut attempt: u32 = 0;
        loop {
            attempt += 1;
            match self.try_get(url, query).await {
                Ok((status, text)) => {
                    let retryable_status = status.is_server_error()
                        || status == reqwest::StatusCode::TOO_MANY_REQUESTS;
                    if retryable_status && attempt < MAX_ATTEMPTS {
                        tokio::time::sleep(backoff_delay(attempt)).await;
                        continue;
                    }
                    return Ok((status, text));
                }
                Err(err) => {
                    if attempt >= MAX_ATTEMPTS || !is_transient(&err) {
                        return Err(GitHubError::RequestError(err));
                    }
                    tokio::time::sleep(backoff_delay(attempt)).await;
                }
            }
        }
    }

    async fn try_get(
        &self,
        url: &str,
        query: &[(&str, &str)],
    ) -> Result<(reqwest::StatusCode, String), reqwest::Error> {
        let response = self
            .client
            .get(url)
            .header("User-Agent", "work-logr")
            .header("Authorization", self.auth_header())
            .header("Accept", "application/vnd.github.v3+json")
            .query(query)
            .send()
            .await?;

        let status = response.status();
        let text = response.text().await?;
        Ok((status, text))
    }
}

/// Classify a reqwest error as a transient network failure worth retrying.
/// Covers the "peer closed connection without close_notify" / unexpected-EOF
/// body-read failure that GitHub's edge produces intermittently.
fn is_transient(err: &reqwest::Error) -> bool {
    err.is_timeout() || err.is_connect() || err.is_request() || err.is_body() || err.is_decode()
}

/// Exponential backoff: 250ms, 500ms, 1s, 2s, ... capped at MAX_BACKOFF.
/// `attempt` is the 1-based number of the attempt that just failed.
fn backoff_delay(attempt: u32) -> Duration {
    let shift = attempt.saturating_sub(1).min(20);
    let millis = BASE_BACKOFF_MS.saturating_mul(1u64 << shift);
    Duration::from_millis(millis).min(MAX_BACKOFF)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    #[test]
    fn backoff_grows_exponentially_and_caps() {
        assert_eq!(backoff_delay(1), Duration::from_millis(250));
        assert_eq!(backoff_delay(2), Duration::from_millis(500));
        assert_eq!(backoff_delay(3), Duration::from_millis(1000));
        assert_eq!(backoff_delay(4), Duration::from_millis(2000));
        assert_eq!(backoff_delay(3), Duration::from_millis(1000));
        assert_eq!(backoff_delay(50), MAX_BACKOFF);
    }

    // Reproduces the production failure: the first connection is dropped mid-
    // request (like GitHub's mid-body reset), the second serves a valid body.
    // Without retry this returns an error; with retry it recovers.
    #[tokio::test]
    async fn retries_after_a_dropped_connection_then_succeeds() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let server = tokio::spawn(async move {
            // First connection: read the request, then drop without responding.
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 1024];
            let _ = socket.read(&mut buf).await;
            drop(socket);

            // Second connection: return a valid JSON body.
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 1024];
            let _ = socket.read(&mut buf).await;
            let body = "[]";
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            socket.write_all(response.as_bytes()).await.unwrap();
            socket.flush().await.unwrap();
        });

        let client = GitHubClient::new("token".into(), "owner".into(), "repo".into());
        let url = format!("http://{}/repos/owner/repo/issues", addr);
        let (status, text) = client
            .get_with_retry(&url, &[("state", "all")])
            .await
            .expect("retry should recover from the dropped connection");

        assert_eq!(status, reqwest::StatusCode::OK);
        assert_eq!(text, "[]");
        server.await.unwrap();
    }
}
