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
            catch (error) { postMessage([error.name, error.message.includes('not implemented')]); }
        `;
        const workerUrl = URL.createObjectURL(new Blob([source], {type:'text/javascript'}));
        const worker = new Worker(workerUrl);
        worker.onmessage = event => { __importResults.push(event.data); worker.terminate(); };
    "#).unwrap();
    js.run_event_loop_bounded(100).await.unwrap();
    assert_eq!(js.evaluate("__importResults").unwrap(),
        serde_json::json!([["NetworkError", true]]));
}
