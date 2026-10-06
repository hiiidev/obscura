use std::collections::HashMap;

use serde_json::{json, Value};

use crate::dispatch::CdpContext;
use crate::types::CdpEvent;

pub struct PausedRequest {
    pub request_id: String,
    pub url: String,
    pub method: String,
    pub headers: HashMap<String, String>,
    pub resource_type: String,
    pub resolver: tokio::sync::oneshot::Sender<FetchResolution>,
}

pub enum FetchResolution {
    Continue {
        url: Option<String>,
        method: Option<String>,
        headers: Option<HashMap<String, String>>,
        post_data: Option<String>,
    },
    Fulfill {
        status: u16,
        headers: Vec<(String, String)>,
        body: String,
    },
    Fail {
        reason: String,
    },
}

pub struct FetchInterceptState {
    pub enabled: bool,
    pub patterns: Vec<String>,
    pub paused: HashMap<String, PausedRequest>,
    /// Synthetic proxy-auth challenges issued before the first request so
    /// Playwright's standard Fetch.authRequired/continueWithAuth flow can
    /// supply BrowserContext proxy credentials.
    pub proxy_auth_requests: HashMap<String, String>,
    request_counter: u64,
}

impl FetchInterceptState {
    pub fn new() -> Self {
        FetchInterceptState {
            enabled: false,
            patterns: Vec::new(),
            paused: HashMap::new(),
            proxy_auth_requests: HashMap::new(),
            request_counter: 0,
        }
    }

    pub fn next_request_id(&mut self) -> String {
        self.request_counter += 1;
        format!("interception-{}", self.request_counter)
    }
}

pub async fn handle(
    method: &str,
    params: &Value,
    ctx: &mut CdpContext,
    session_id: &Option<String>,
) -> Result<Value, String> {
    match method {
        "enable" => {
            let patterns = params
                .get("patterns")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|p| {
                            p.get("urlPattern")
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string())
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_else(|| vec!["*".to_string()]);
            let handle_auth_requests = params
                .get("handleAuthRequests")
                .and_then(Value::as_bool)
                .unwrap_or(false);

            // Snapshot auth routing before mutably borrowing interception state.
            let proxy_auth_target = if handle_auth_requests {
                ctx.get_session_page(session_id).and_then(|page| {
                    let proxy = page.context.proxy_url.clone()?;
                    if page.context.has_proxy_credentials() {
                        return None;
                    }
                    Some((page.id.clone(), page.frame_id.clone(), proxy))
                })
            } else {
                None
            };

            ctx.fetch_intercept.enabled = true;
            ctx.fetch_intercept.patterns = patterns.clone();
            let tx_clone = ctx.intercept_tx.clone();
            if let Some(page) = ctx.get_session_page_mut(session_id) {
                page.intercept_block_patterns = patterns.clone();
                if let Some(tx) = tx_clone {
                    page.set_intercept_tx(tx);
                }
                page.enable_intercept(true);
            }

            if let Some((page_id, frame_id, proxy)) = proxy_auth_target {
                let request_id = ctx.fetch_intercept.next_request_id();
                ctx.fetch_intercept
                    .proxy_auth_requests
                    .insert(request_id.clone(), page_id);

                let mut parsed = url::Url::parse(&proxy)
                    .map_err(|error| format!("Invalid configured proxy URL: {error}"))?;
                let _ = parsed.set_username("");
                let _ = parsed.set_password(None);
                parsed.set_path("/");
                parsed.set_query(None);
                parsed.set_fragment(None);
                let origin = parsed.origin().ascii_serialization();
                let request_url = parsed.to_string();

                ctx.pending_events.push(CdpEvent {
                    method: "Fetch.authRequired".to_string(),
                    params: json!({
                        "requestId": request_id,
                        "request": {
                            "url": request_url,
                            "method": "GET",
                            "headers": {},
                            "initialPriority": "High",
                            "referrerPolicy": "no-referrer",
                        },
                        "frameId": frame_id,
                        "resourceType": "Document",
                        "authChallenge": {
                            "source": "Proxy",
                            "origin": origin,
                            "scheme": "Basic",
                            "realm": "proxy",
                        },
                    }),
                    session_id: session_id.clone(),
                });
            }

            tracing::info!("Fetch interception enabled");
            Ok(json!({}))
        }
        "disable" => {
            ctx.fetch_intercept.enabled = false;
            ctx.fetch_intercept.patterns.clear();
            ctx.fetch_intercept.proxy_auth_requests.clear();
            if let Some(page) = ctx.get_session_page_mut(session_id) {
                page.intercept_block_patterns.clear();
                page.enable_intercept(false);
            }
            let paused: Vec<_> = ctx.fetch_intercept.paused.drain().collect();
            for (_, req) in paused {
                let _ = req.resolver.send(FetchResolution::Continue {
                    url: None,
                    method: None,
                    headers: None,
                    post_data: None,
                });
            }
            Ok(json!({}))
        }
        "continueWithAuth" => {
            let request_id = params
                .get("requestId")
                .and_then(Value::as_str)
                .ok_or("requestId required")?;
            let page_id = ctx
                .fetch_intercept
                .proxy_auth_requests
                .remove(request_id);

            if let Some(page_id) = page_id {
                let response = params
                    .get("authChallengeResponse")
                    .and_then(|value| value.get("response"))
                    .and_then(Value::as_str)
                    .unwrap_or("Default");
                if response == "ProvideCredentials" {
                    let username = params
                        .get("authChallengeResponse")
                        .and_then(|value| value.get("username"))
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    let password = params
                        .get("authChallengeResponse")
                        .and_then(|value| value.get("password"))
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();

                    let context = ctx
                        .pages
                        .iter()
                        .find(|page| page.id == page_id)
                        .map(|page| page.context.clone())
                        .ok_or("Proxy auth target page no longer exists")?;
                    context
                        .set_proxy_credentials(username, password)
                        .map_err(|error| error.to_string())?;

                    #[cfg(feature = "stealth")]
                    for page in &mut ctx.pages {
                        if std::sync::Arc::ptr_eq(&page.context, &context) {
                            page.refresh_stealth_transport();
                        }
                    }
                }
            }
            Ok(json!({}))
        }
        "continueRequest" => {
            let request_id = params
                .get("requestId")
                .and_then(|v| v.as_str())
                .ok_or("requestId required")?;

            if let Some(paused) = ctx.fetch_intercept.paused.remove(request_id) {
                let _ = paused.resolver.send(FetchResolution::Continue {
                    url: params
                        .get("url")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string()),
                    method: params
                        .get("method")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string()),
                    // Honor client header overrides (route.continue({ headers }))
                    // — parity with server.rs handle_fetch_resolution (#919).
                    headers: crate::server::parse_cdp_headers(params),
                    post_data: params
                        .get("postData")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string()),
                });
            }
            Ok(json!({}))
        }
        "fulfillRequest" => {
            let request_id = params
                .get("requestId")
                .and_then(|v| v.as_str())
                .ok_or("requestId required")?;

            let status = params
                .get("responseCode")
                .and_then(|v| v.as_u64())
                .unwrap_or(200) as u16;
            let headers: HashMap<String, String> = params
                .get("responseHeaders")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|h| {
                            let name = h.get("name")?.as_str()?.to_string();
                            let value = h.get("value")?.as_str()?.to_string();
                            Some((name, value))
                        })
                        .collect()
                })
                .unwrap_or_default();
            // The CDP fulfillRequest body is base64-encoded; decode it — parity
            // with server.rs handle_fetch_resolution (#919). (Binary-safe body
            // transport across the JS boundary remains tracked in #912.)
            let body =
                crate::server::decode_base64(params.get("body").and_then(|v| v.as_str()).unwrap_or(""));

            if let Some(paused) = ctx.fetch_intercept.paused.remove(request_id) {
                let _ = paused.resolver.send(FetchResolution::Fulfill {
                    status,
                    headers: headers.into_iter().collect(),
                    body,
                });
            }
            Ok(json!({}))
        }
        "failRequest" => {
            let request_id = params
                .get("requestId")
                .and_then(|v| v.as_str())
                .ok_or("requestId required")?;

            let reason = params
                .get("errorReason")
                .and_then(|v| v.as_str())
                .unwrap_or("Failed")
                .to_string();

            if let Some(paused) = ctx.fetch_intercept.paused.remove(request_id) {
                let _ = paused.resolver.send(FetchResolution::Fail { reason });
            }
            Ok(json!({}))
        }
        "getResponseBody" => Ok(json!({ "body": "", "base64Encoded": false })),
        "takeResponseBodyAsStream" => {
            // Hand the client a streaming handle for a large response body so it
            // can pull it in chunks via IO.read and free it with IO.close,
            // instead of receiving one giant base64 blob (issue #360). The body
            // is moved out of the page cache into the stream, so it is held once
            // and released on close. Requires the body to have been cached
            // (raise OBSCURA_NETWORK_BODY_BUFFER_BYTES for large downloads).
            let request_id = params
                .get("requestId")
                .and_then(|v| v.as_str())
                .ok_or("Fetch.takeResponseBodyAsStream requires requestId")?;

            let bytes = {
                let page = ctx.get_session_page_mut(session_id).ok_or("No page")?;
                page.take_response_body_raw(request_id)
            }
            .or_else(|| {
                ctx.pages
                    .iter_mut()
                    .find_map(|p| p.take_response_body_raw(request_id))
            })
            .ok_or_else(|| {
                format!("Fetch.takeResponseBodyAsStream: no cached body for {request_id}")
            })?;

            let handle = ctx
                .io_streams
                .insert(bytes)
                .map_err(|error| format!("Fetch.takeResponseBodyAsStream: {error}"))?;
            Ok(json!({ "stream": handle }))
        }
        _ => Err(format!("Unknown Fetch method: {}", method)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dispatch::CdpContext;
    use serde_json::json;
    use std::collections::HashMap;

    fn pause(ctx: &mut CdpContext, id: &str) -> tokio::sync::oneshot::Receiver<FetchResolution> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        ctx.fetch_intercept.paused.insert(
            id.to_string(),
            PausedRequest {
                request_id: id.to_string(),
                url: "https://example.test/".to_string(),
                method: "GET".to_string(),
                headers: HashMap::new(),
                resource_type: "Fetch".to_string(),
                resolver: tx,
            },
        );
        rx
    }

    // Parity with server.rs handle_fetch_resolution: continueRequest must
    // forward the client's header overrides (route.continue({ headers })), not
    // drop them. See #919.
    #[tokio::test]
    async fn continue_request_forwards_header_overrides() {
        let mut ctx = CdpContext::new();
        let rx = pause(&mut ctx, "req-1");
        handle(
            "continueRequest",
            &json!({ "requestId": "req-1", "headers": [{ "name": "X-Test", "value": "42" }] }),
            &mut ctx,
            &None,
        )
        .await
        .expect("continueRequest should succeed");

        match rx.await.expect("resolver should fire") {
            FetchResolution::Continue { headers, .. } => {
                let mut expected = HashMap::new();
                expected.insert("X-Test".to_string(), "42".to_string());
                assert_eq!(headers, Some(expected), "continue must forward header overrides");
            }
            _ => panic!("expected FetchResolution::Continue"),
        }
    }

    // Parity with server.rs: the fulfillRequest body is base64-encoded per CDP
    // and must be decoded, not passed through as raw base64 text. See #919/#912.
    #[tokio::test]
    async fn fulfill_request_base64_decodes_body() {
        let mut ctx = CdpContext::new();
        let rx = pause(&mut ctx, "req-2");
        handle(
            "fulfillRequest",
            &json!({ "requestId": "req-2", "responseCode": 200, "body": "SGVsbG8=" }),
            &mut ctx,
            &None,
        )
        .await
        .expect("fulfillRequest should succeed");

        match rx.await.expect("resolver should fire") {
            FetchResolution::Fulfill { body, .. } => {
                assert_eq!(body, "Hello", "fulfill body must be base64-decoded");
            }
            _ => panic!("expected FetchResolution::Fulfill"),
        }
    }
}
