use obscura_js::runtime::ObscuraJsRuntime;

#[tokio::test(flavor = "current_thread")]
async fn worker_imports_blob_and_data_scripts_synchronously_in_order() {
    let mut js = ObscuraJsRuntime::new();
    js.run_page_init();
    js.execute_script("importScripts-in-order", r#"
        globalThis.__importResults = [];
        const blobA = URL.createObjectURL(new Blob([
            'self.answer = 40; self.events = ["first"];'
        ], {type:'text/javascript'}));
        const blobB = URL.createObjectURL(new Blob([
            'self.answer += 2; self.events.push("second");'
        ], {type:'text/javascript'}));
        const source = 'importScripts(' + JSON.stringify(blobA) + ','
          + JSON.stringify(blobB) + ');'
          + 'importScripts("data:text/javascript,self.events.push(%22third%22)");'
          + 'postMessage([self.answer, self.events]);';
        const workerUrl = URL.createObjectURL(new Blob([source], {type:'text/javascript'}));
        const worker = new Worker(workerUrl);
        worker.onmessage = event => { __importResults.push(event.data); worker.terminate(); };
    "#).unwrap();
    js.run_event_loop_bounded(100).await.unwrap();
    assert_eq!(js.evaluate("__importResults").unwrap(),
        serde_json::json!([[42, ["first", "second", "third"]]]));
}

#[tokio::test(flavor = "current_thread")]
async fn worker_imports_reject_network_url_instead_of_async_execution() {
    let mut js = ObscuraJsRuntime::new();
    js.run_page_init();
    js.execute_script("importScripts-network-rejected", r#"
        globalThis.__importResults = [];
        const source = `
            try { importScripts('https://example.com/lib.js'); }
            catch (error) { postMessage([error.name, error.message.includes('page HTTP client')]); }
        `;
        const workerUrl = URL.createObjectURL(new Blob([source], {type:'text/javascript'}));
        const worker = new Worker(workerUrl);
        worker.onmessage = event => { __importResults.push(event.data); worker.terminate(); };
    "#).unwrap();
    js.run_event_loop_bounded(100).await.unwrap();
    assert_eq!(js.evaluate("__importResults").unwrap(),
        serde_json::json!([["NetworkError", true]]));
}


#[tokio::test(flavor = "current_thread")]
async fn worker_imports_network_script_before_next_statement() {
    use std::io::{Read, Write};
    use std::sync::Arc;
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0u8; 4096];
        let n = stream.read(&mut request).unwrap();
        assert!(String::from_utf8_lossy(&request[..n]).starts_with("GET /lib.js "));
        let body = "self.imported = 42; self.order = ['imported'];";
        write!(stream,
            "HTTP/1.1 200 OK\r\nContent-Type: application/javascript\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(), body).unwrap();
    });
    let mut js = ObscuraJsRuntime::new();
    let base = format!("http://{address}/worker.js");
    js.set_url(&format!("http://{address}/index.html"));
    js.set_http_client(Arc::new(obscura_net::ObscuraHttpClient::with_full_options(
        Arc::new(obscura_net::CookieJar::new()), None, true,
    )));
    js.run_page_init();
    let script = format!(
        "importScripts({:?}); self.order.push('after'); postMessage([self.imported,self.order]);",
        format!("http://{address}/lib.js")
    );
    js.execute_script("worker-network-import", &format!(r#"
        globalThis.__networkImportReplies = [];
        const blobUrl = URL.createObjectURL(new Blob([{}], {{type:'text/javascript'}}));
        const worker = new Worker(blobUrl);
        worker.onmessage = event => {{ __networkImportReplies.push(event.data); worker.terminate(); }};
    "#, serde_json::to_string(&script).unwrap())).unwrap();
    js.run_event_loop_bounded(200).await.unwrap();
    assert_eq!(js.evaluate("__networkImportReplies").unwrap(),
        serde_json::json!([[42, ["imported", "after"]]]));
    server.join().unwrap();
}


#[tokio::test(flavor = "current_thread")]
async fn network_imports_use_each_context_authenticated_proxy() {
    use std::io::{Read, Write};
    use std::sync::Arc;
    for (user, pass, value) in [("alice", "alpha", 11), ("bob", "beta", 22)] {
        let proxy = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let proxy_addr = proxy.local_addr().unwrap();
        let expected = format!(
            "Proxy-Authorization: Basic {}",
            base64::Engine::encode(
                &base64::engine::general_purpose::STANDARD,
                format!("{user}:{pass}"),
            )
        ).to_ascii_lowercase();
        let proxy_thread = std::thread::spawn(move || {
            let (mut stream, _) = proxy.accept().unwrap();
            let mut bytes = [0u8; 8192];
            let n = stream.read(&mut bytes).unwrap();
            let request = String::from_utf8_lossy(&bytes[..n]).to_ascii_lowercase();
            assert!(request.starts_with("get http://example.invalid/lib.js "));
            assert!(request.contains(&expected), "missing the expected proxy authorization");
            let body = format!("self.proxyIdentity = {value};");
            write!(stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/javascript\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(), body,
            ).unwrap();
        });
        let client = Arc::new(obscura_net::ObscuraHttpClient::with_full_options(
            Arc::new(obscura_net::CookieJar::new()),
            Some(&format!("http://{proxy_addr}")),
            true,
        ));
        client.set_proxy_credentials(user.into(), pass.into()).unwrap();
        let mut js = ObscuraJsRuntime::new();
        js.set_url("https://example.com/parent.html");
        js.set_http_client(client);
        js.run_page_init();
        js.execute_script("worker-proxy", r#"
            globalThis.__proxyReplies = [];
            const src = 'importScripts("http://example.invalid/lib.js"); postMessage(self.proxyIdentity);';
            const u = URL.createObjectURL(new Blob([src], {type:'text/javascript'}));
            const w = new Worker(u);
            w.onmessage = event => { __proxyReplies.push(event.data); w.terminate(); };
        "#).unwrap();
        js.run_event_loop_bounded(250).await.unwrap();
        assert_eq!(js.evaluate("__proxyReplies").unwrap(),
            serde_json::json!([value]));
        proxy_thread.join().unwrap();
    }
}


/// A proxy refusal must surface an error, never retry the unresolved host
/// directly. The .invalid TLD makes a direct fallback observable as failure.
#[tokio::test(flavor = "current_thread")]
async fn network_import_rejects_proxy_407_without_direct_fallback() {
    use std::io::{Read, Write};
    use std::sync::Arc;
    let proxy = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = proxy.local_addr().unwrap();
    let proxy_thread = std::thread::spawn(move || {
        let (mut stream, _) = proxy.accept().unwrap();
        let mut request = [0u8; 4096];
        let n = stream.read(&mut request).unwrap();
        assert!(String::from_utf8_lossy(&request[..n])
            .starts_with("GET http://example.invalid/refused.js "));
        stream.write_all(
            b"HTTP/1.1 407 Proxy Authentication Required\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        ).unwrap();
    });
    let client = Arc::new(obscura_net::ObscuraHttpClient::with_full_options(
        Arc::new(obscura_net::CookieJar::new()),
        Some(&format!("http://{address}")),
        true,
    ));
    let mut js = ObscuraJsRuntime::new();
    js.set_url("https://example.com/parent.html");
    js.set_http_client(client);
    js.run_page_init();
    js.execute_script("worker-proxy-refused", r#"
        globalThis.__proxyFailures = [];
        const code = 'importScripts("http://example.invalid/refused.js"); postMessage("unexpected");';
        const workerUrl = URL.createObjectURL(new Blob([code], {type: 'text/javascript'}));
        const worker = new Worker(workerUrl);
        worker.onerror = error => { __proxyFailures.push(error.name); worker.terminate(); };
        worker.onmessage = event => { __proxyFailures.push(event.data); worker.terminate(); };
    "#).unwrap();
    js.run_event_loop_bounded(250).await.unwrap();
    assert_eq!(js.evaluate("__proxyFailures").unwrap(),
        serde_json::json!(["NetworkError"]));
    proxy_thread.join().unwrap();
}


#[tokio::test(flavor = "current_thread")]
async fn dedicated_worker_has_no_document_or_window_global() {
    let mut js = ObscuraJsRuntime::new();
    js.run_page_init();
    js.execute_script("dedicated-worker-global-scope", r#"
        globalThis.__dedicatedWorkerResults = [];
        const source = [
            'postMessage({',
            '  documentType: typeof document,',
            '  windowType: typeof window,',
            '  selfDocumentType: typeof self.document,',
            '  globalDocumentType: typeof globalThis.document,',
            '  importScriptsType: typeof importScripts,',
            '  selfType: typeof self,',
            '  locationType: typeof location,',
            '  globalThisIsSelf: globalThis === self',
            '});'
        ].join('\n');
        const workerURL = URL.createObjectURL(new Blob([source], {type:'text/javascript'}));
        const worker = new Worker(workerURL);
        worker.onmessage = event => {
            __dedicatedWorkerResults.push(event.data);
            worker.terminate();
        };
    "#).unwrap();
    js.run_event_loop_bounded(100).await.unwrap();
    assert_eq!(js.evaluate("__dedicatedWorkerResults").unwrap(),
        serde_json::json!([{
            "documentType": "undefined",
            "windowType": "undefined",
            "selfDocumentType": "undefined",
            "globalDocumentType": "undefined",
            "importScriptsType": "function",
            "selfType": "object",
            "locationType": "object",
            "globalThisIsSelf": true
        }]));
    assert_eq!(js.evaluate("typeof document").unwrap(), serde_json::json!("object"),
        "the parent page must retain its DOM");
}

#[tokio::test(flavor = "current_thread")]
async fn imported_worker_script_and_message_handler_do_not_inherit_window_globals() {
    let mut js = ObscuraJsRuntime::new();
    js.run_page_init();
    js.execute_script("dedicated-worker-import-global-scope", r#"
        globalThis.__workerScopeResults = [];
        const imported = URL.createObjectURL(new Blob([
            'self.importDoc = typeof document;',
            'self.importWindow = typeof globalThis.window;',
        ], {type: 'text/javascript'}));
        const source = 'importScripts(' + JSON.stringify(imported) + ');'
            + 'onmessage = function() { postMessage(['
            + 'typeof document, typeof window, self.importDoc,'
            + 'self.importWindow, globalThis === self]); };';
        const u = URL.createObjectURL(new Blob([source], {type:'text/javascript'}));
        const w = new Worker(u);
        w.onmessage = e => { __workerScopeResults.push(e.data); w.terminate(); };
        w.postMessage('probe');
    "#).unwrap();
    js.run_event_loop_bounded(100).await.unwrap();
    assert_eq!(js.evaluate("__workerScopeResults").unwrap(),
        serde_json::json!([["undefined", "undefined", "undefined", "undefined", true]]));
}
