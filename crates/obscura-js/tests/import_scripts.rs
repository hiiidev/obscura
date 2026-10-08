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
            "HTTP/1.1 200 OK\\r\\nContent-Type: application/javascript\\r\\nContent-Length: {}\\r\\nConnection: close\\r\\n\\r\\n{}",
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
