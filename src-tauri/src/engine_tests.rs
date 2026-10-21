use super::*;
fn port() -> u16 {
    std::net::TcpListener::bind(("127.0.0.1", 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}
fn fixture(script: &str) -> (tempfile::TempDir, RuntimePaths) {
    let dir = tempfile::tempdir().unwrap();
    let entry = dir.path().join("main.mjs");
    std::fs::write(&entry, script).unwrap();
    let node = std::env::var_os("ASTRALINK_NODE_BINARY")
        .map(PathBuf::from)
        .unwrap_or_else(|| candidate_paths(Some(dir.path()), dir.path(), None).node);
    assert!(node.is_file(), "runtime tests require Node executable");
    let paths = RuntimePaths {
        node,
        entry,
        cwd: dir.path().into(),
    };
    (dir, paths)
}
const SERVER: &str = r#"import http from 'node:http';
const server=http.createServer(async(req,res)=>{res.setHeader('Content-Type','application/json');if(req.headers.authorization!=='Bearer '+process.env.ASTRALINK_CONTROL_TOKEN){res.writeHead(401);res.end('{}');return;}let body='';for await(const chunk of req)body+=chunk;
if(req.url==='/control/status')res.end(JSON.stringify({running:true,pid:process.pid,version:'fixture',endpoint:'http://127.0.0.1:'+process.env.ASTRALINK_PORT+'/mcp'}));
else if(req.url==='/control/profiles'){const profiles=JSON.parse(body).profiles;if(profiles.some(p=>p.hasPassword&&!p.password||p.hasTotpSecret&&!p.totpSecret)){res.writeHead(400);res.end('{}');}else res.end(JSON.stringify({count:profiles.length}));}
else if(req.url==='/control/test')res.end(JSON.stringify({status:'failed',stage:'fixture',detail:'真实控制端点已转发',elapsedMs:1}));
else if(req.url.startsWith('/control/events'))res.end(JSON.stringify({cursor:0,events:[]}));
else if(req.url==='/control/shutdown'){res.end('{}');server.close(()=>process.exit(0));}
else{res.writeHead(404);res.end('{}');}});server.listen(Number(process.env.ASTRALINK_PORT),'127.0.0.1');"#;
#[tokio::test]
async fn missing_engine_reports_not_running_and_failed_test() {
    let (_dir, paths) = fixture("process.exit(1)");
    let manager = EngineManager::new(paths, port(), "test-token".into()).unwrap();
    assert!(!manager.status().await.running);
    let status = manager.start(vec![]).await;
    assert!(!status.running);
    assert!(status.error.is_some());
    let test = manager.test("unknown").await;
    assert_eq!(test.status, "failed");
    assert_eq!(test.stage, "engine");
}
#[tokio::test]
async fn real_control_handshake_sync_test_and_shutdown() {
    let (_dir, paths) = fixture(SERVER);
    let manager = EngineManager::new(paths, port(), "test-token".into()).unwrap();
    let p = ConnectionProfile {
        id: uuid::Uuid::new_v4().to_string(),
        name: "p".into(),
        host: "example".into(),
        port: 22,
        username: "ops".into(),
        password: Some("runtime-test-secret".into()),
        has_password: true,
        totp_secret: Some("GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ".into()),
        has_totp_secret: true,
        enabled: true,
        ..Default::default()
    };
    let status = manager.start(vec![p]).await;
    assert!(status.running, "{:?}", status);
    assert_eq!(status.version.as_deref(), Some("fixture"));
    assert_eq!(manager.test("p").await.stage, "fixture");
    assert!(!manager.stop().await.running);
    assert!(!manager.status().await.running);
}
#[tokio::test]
async fn jumpserver_menu_is_not_reported_as_authentication_failure() {
    let server = SERVER.replace(
        "status:'failed',stage:'fixture',detail:'真实控制端点已转发'",
        "status:'partial',stage:'target',detail:'认证成功，尚未选择目标资产'",
    );
    let (_dir, paths) = fixture(&server);
    let manager = EngineManager::new(paths, port(), "test-token".into()).unwrap();
    assert!(manager.start(vec![]).await.running);
    let result = manager.test(&uuid::Uuid::new_v4().to_string()).await;
    assert_eq!(result.status, "partial");
    assert_eq!(result.stage, "target");
    manager.stop().await;
}
#[tokio::test]
async fn owned_engine_diagnostics_are_ingested_once_and_persist_without_raw_payloads() {
    let server = SERVER.replace("else if(req.url.startsWith('/control/events'))", "else if(req.url.startsWith('/control/logs'))res.end(JSON.stringify({cursor:1,dropped:false,entries:req.url.endsWith('=0')?[{seq:1,timestamp:Date.now(),code:'otp_auto',message:'private-secret'}]:[]}));else if(req.url.startsWith('/control/events'))");
    let (dir, paths) = fixture(&server);
    let logs = Arc::new(crate::runtime_logs::RuntimeLogs::open(
        dir.path().join("logs"),
    ));
    let manager = EngineManager::new(paths, port(), "test-token".into())
        .unwrap()
        .with_logs(logs.clone());
    assert!(manager.start(vec![]).await.running);
    tokio::time::sleep(Duration::from_millis(550)).await;
    manager.stop().await;
    let entries = logs.snapshot().entries;
    assert_eq!(entries.iter().filter(|e| e.code == "otp_auto").count(), 1);
    assert!(!serde_json::to_string(&entries)
        .unwrap()
        .contains("private-secret"));
    assert!(dir.path().join("logs/runtime.jsonl").exists());
}
#[tokio::test]
async fn rejected_profile_sync_never_reports_running_success() {
    let (_dir, paths) = fixture(SERVER);
    let manager = EngineManager::new(paths, port(), "test-token".into()).unwrap();
    let p = ConnectionProfile {
        has_password: true,
        ..Default::default()
    };
    let s = manager.start(vec![p]).await;
    assert!(!s.running);
    assert!(s.error.is_some());
    assert!(!manager.status().await.running);
}
#[tokio::test]
async fn occupied_port_is_not_adopted_as_our_engine() {
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let (_dir, paths) = fixture(SERVER);
    let manager = EngineManager::new(
        paths,
        listener.local_addr().unwrap().port(),
        "test-token".into(),
    )
    .unwrap();
    let s = manager.start(vec![]).await;
    assert!(!s.running);
    assert!(s.error.is_some());
}

#[tokio::test]
async fn actual_engine_control_roundtrip() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf();
    let paths = resolve_paths(
        Some(&root),
        Path::new("unused"),
        std::env::var_os("ASTRALINK_NODE_BINARY")
            .map(PathBuf::from)
            .as_deref(),
    )
    .unwrap();
    let p = port();
    let directory = tempfile::tempdir().unwrap();
    let logs = Arc::new(crate::runtime_logs::RuntimeLogs::open(
        directory.path().join("logs"),
    ));
    let manager = EngineManager::new(paths, p, "actual-engine-test-token".into())
        .unwrap()
        .with_logs(logs.clone());
    let status = manager.start(vec![]).await;
    assert!(status.running, "{status:?}");
    assert_eq!(status.version.as_deref(), Some("0.1.0"));
    let test = manager.test(&uuid::Uuid::new_v4().to_string()).await;
    assert_eq!(test.status, "failed");
    assert!(!manager.stop().await.running);
    assert!(logs
        .snapshot()
        .entries
        .iter()
        .any(|entry| entry.code == "engine_ready"));
    assert!(logs
        .snapshot()
        .entries
        .iter()
        .any(|entry| entry.code == "profiles_synced"));
    let output = directory.path().join("runtime-export.log");
    logs.export(&output).unwrap();
    let exported = std::fs::read_to_string(output).unwrap();
    assert!(exported.contains("MCP"));
    assert!(!exported.contains("actual-engine-test-token"));
    let reopened = crate::runtime_logs::RuntimeLogs::open(directory.path().join("logs"));
    assert!(reopened
        .snapshot()
        .entries
        .iter()
        .any(|entry| entry.code == "engine_ready"));
    assert!(std::net::TcpListener::bind(("127.0.0.1", p)).is_ok());
}
#[tokio::test]
async fn unknown_auth_response_is_not_reported_accepted() {
    let (_dir, paths) = fixture(SERVER);
    let manager = EngineManager::new(paths, port(), "test-token".into()).unwrap();
    assert!(!manager
        .auth(
            &uuid::Uuid::new_v4().to_string(),
            Some("123456".into()),
            false
        )
        .await
        .unwrap());
    assert!(!manager
        .auth(&uuid::Uuid::new_v4().to_string(), None, true)
        .await
        .unwrap());
}
#[tokio::test]
async fn otp_can_be_forwarded_while_connection_test_waits() {
    const AUTH: &str = r#"import http from 'node:http';const id='00000000-0000-4000-8000-000000000001';const profileId='00000000-0000-4000-8000-000000000002';let waiting,settled=false;
 const server=http.createServer(async(req,res)=>{res.setHeader('Content-Type','application/json');if(req.headers.authorization!=='Bearer '+process.env.ASTRALINK_CONTROL_TOKEN){res.writeHead(401);res.end('{}');return;}let body='';for await(const chunk of req)body+=chunk;
 if(req.url==='/control/status')res.end(JSON.stringify({running:true,pid:process.pid,version:'fixture',endpoint:'http://127.0.0.1:'+process.env.ASTRALINK_PORT+'/mcp'}));
 else if(req.url==='/control/profiles')res.end(JSON.stringify({count:JSON.parse(body).profiles.length}));
 else if(req.url==='/control/test')waiting=res;
 else if(req.url.startsWith('/control/events'))res.end(JSON.stringify({cursor:waiting?1:0,events:waiting&&!settled?[{requestId:id,profileId,connectionName:'test',host:'host',username:'ops',instructions:'OTP',prompts:[{prompt:'Code',echo:false}],expiresAt:Date.now()+10000}]:[]}));
 else if(req.url==='/control/auth'){const input=JSON.parse(body);const accepted=input.requestId===id&&input.code==='123456';res.end(JSON.stringify({accepted}));if(accepted){settled=true;waiting.end(JSON.stringify({status:'auth_required',stage:'authentication',detail:'控制桥已处理',elapsedMs:1}));}}
 else if(req.url==='/control/shutdown'){res.end('{}');server.close(()=>process.exit(0));}});server.listen(Number(process.env.ASTRALINK_PORT),'127.0.0.1');"#;
    let (_dir, paths) = fixture(AUTH);
    let manager = EngineManager::new(paths, port(), "test-token".into()).unwrap();
    assert!(manager.start(vec![]).await.running);
    let testing = manager.clone();
    let test =
        tokio::spawn(async move { testing.test("00000000-0000-4000-8000-000000000002").await });
    let id = "00000000-0000-4000-8000-000000000001";
    for _ in 0..40 {
        if manager.inner.lock().await.pending.contains_key(id) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(manager.inner.lock().await.pending.contains_key(id));
    assert!(manager
        .auth(id, Some("123456".into()), false)
        .await
        .unwrap());
    let result = tokio::time::timeout(Duration::from_secs(3), test)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(result.status, "auth_required");
    assert!(!manager
        .auth(id, Some("123456".into()), false)
        .await
        .unwrap());
    manager.stop().await;
}

#[tokio::test]
async fn port_change_updates_stopped_endpoint_and_rejects_running_engine() {
    let (_dir, paths) = fixture(SERVER);
    let manager = EngineManager::new(paths, port(), "test-token".into()).unwrap();
    let next = port();
    manager.set_port(next).await.unwrap();
    assert_eq!(
        manager.status().await.endpoint,
        format!("http://127.0.0.1:{next}/mcp")
    );
    assert!(manager.start(vec![]).await.running);
    assert!(manager.set_port(port()).await.is_err());
    manager.stop().await;
}

#[tokio::test]
async fn temporary_control_failure_does_not_kill_live_engine() {
    let script = SERVER.replacen(
        "const server=http.createServer",
        "let statusCalls=0; const server=http.createServer",
        1,
    ).replacen(
        "if(req.url==='/control/status')res.end(JSON.stringify({running:true",
        "if(req.url==='/control/status' && ++statusCalls===2){res.writeHead(503);res.end('{}');} else if(req.url==='/control/status')res.end(JSON.stringify({running:true",
        1,
    );
    let (_dir, paths) = fixture(&script);
    let manager = EngineManager::new(paths, port(), "test-token".into()).unwrap();
    assert!(manager.start(vec![]).await.running);
    tokio::time::sleep(Duration::from_millis(600)).await;
    assert!(
        manager.status().await.running,
        "transient probe failure must not kill the owned process"
    );
    assert_eq!(manager.test("fixture").await.stage, "fixture");
    manager.stop().await;
}

#[tokio::test]
#[ignore = "requires ASTRALINK_TEST_INSTALLED_DIR installed bundle"]
async fn installed_resource_layout_starts_real_engine() {
    let root = PathBuf::from(std::env::var_os("ASTRALINK_TEST_INSTALLED_DIR").unwrap());
    let paths = resolve_paths(None, &root, None).unwrap();
    let test_port = std::env::var("ASTRALINK_TEST_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or_else(port);
    let manager = EngineManager::new(paths, test_port, "installed-fixture-token".into()).unwrap();
    let status = manager.start(vec![]).await;
    assert!(status.running, "{status:?}");
    manager.stop().await;
}

#[test]
fn boot_errors_are_classified_without_leaking_diagnostic_text() {
    assert_eq!(
        super::boot_failure(Some(1), "Error [ERR_MODULE_NOT_FOUND]: secret"),
        "MCP 运行依赖缺失"
    );
    assert_eq!(
        super::boot_failure(Some(1), "EADDRINUSE private-value"),
        "MCP 端口已被占用"
    );
    assert_eq!(
        super::boot_failure(Some(-1073741511), "private-value"),
        "MCP 运行程序缺少 Windows 入口点"
    );
    assert_eq!(
        super::boot_failure(Some(1), "private-value"),
        "MCP 引擎启动失败（退出码 1）"
    );
}

#[test]
fn bundled_node_name_matches_build_platform() {
    let paths = candidate_paths(None, Path::new("/fixture/resources"), None);
    let binary = if cfg!(windows) { "node.exe" } else { "node" };
    assert_eq!(paths.node.file_name().unwrap(), binary);
}

#[test]
fn missing_node_has_an_actionable_install_message() {
    let dir = tempfile::tempdir().unwrap();
    let absent = dir.path().join("missing-node");
    let error = resolve_paths(Some(dir.path()), dir.path(), Some(&absent))
        .err()
        .unwrap();
    assert!(error.contains("安装 Node.js"), "{error}");
}

#[cfg(windows)]
#[test]
fn node_script_paths_strip_windows_extended_length_prefix() {
    use std::path::Path;
    let path = Path::new(r"\\?\F:\develop\astralink\engine\src\main.mjs");
    assert_eq!(
        super::node_compatible_path(path),
        PathBuf::from(r"F:\develop\astralink\engine\src\main.mjs")
    );
    let unc = Path::new(r"\\?\UNC\server\share\engine\src\main.mjs");
    assert_eq!(
        super::node_compatible_path(unc),
        PathBuf::from(r"\\server\share\engine\src\main.mjs")
    );
}

#[cfg(windows)]
#[test]
fn release_paths_use_node_compatible_normal_drive_paths() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("runtime")).unwrap();
    std::fs::write(dir.path().join("runtime/node.exe"), b"fixture").unwrap();
    let extended = std::fs::canonicalize(dir.path()).unwrap();
    let normal = super::node_compatible_path(&extended);
    let paths = super::candidate_paths(None, &extended, None);
    assert_eq!(paths.node, normal.join("runtime/node.exe"));
    assert_eq!(paths.entry, normal.join("engine/src/main.mjs"));
    assert_eq!(paths.cwd, normal.join("engine"));
}
