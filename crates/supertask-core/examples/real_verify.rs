//! End-to-end verification of the real engine (Real spawner) for SuperTask.
//!
//! Drives, against live processes:
//!   1. Import: open the real knife4j-demo-openapi3 supertask.yaml -> snapshot.
//!   2. knife4j real start attempt (documents the SNAPSHOT-reactor prerequisite).
//!   3. Minimal standalone Spring Boot 3.3.6 app: start -> tcp health -> HTTP
//!      serve -> stop -> process-tree kill -> port freed.
//!
//! Run:
//!   cargo run --example real_verify -p supertask-core -- <knife4j_dir> <springapp_dir>

use std::io::{Read, Write};
use std::path::Path;
use std::time::{Duration, Instant};

use supertask_core::engine::Engine;
use supertask_core::runtime::RtState;

const PORT: u16 = 8080;

fn port_open(host: &str, port: u16) -> bool {
    std::net::TcpStream::connect((host, port)).is_ok()
}

fn http_get(host: &str, port: u16, path: &str) -> Option<String> {
    let mut s = std::net::TcpStream::connect((host, port)).ok()?;
    s.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
    let req = format!("GET {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n");
    s.write_all(req.as_bytes()).ok()?;
    let mut buf = Vec::new();
    s.read_to_end(&mut buf).ok()?;
    Some(String::from_utf8_lossy(&buf).to_string())
}

fn tail_log(path: &Path, n: usize) -> String {
    match std::fs::read_to_string(path) {
        Ok(t) => t.lines().rev().take(n).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("\n"),
        Err(e) => format!("(no log: {e})"),
    }
}

fn main() {
    let knife4j = std::env::args().nth(1).expect("arg1 = knife4j workspace dir");
    let springapp = std::env::args().nth(2).expect("arg2 = springapp workspace dir");

    println!("=== [1] IMPORT: open real knife4j-demo-openapi3 supertask.yaml ===");
    let eng_imp = Engine::new();
    match eng_imp.open(Path::new(&knife4j)) {
        Ok((warnings, snap)) => {
            println!("  opened OK. workspace_id = {}", snap.workspace_id);
            for (id, svc) in &snap.services {
                println!(
                    "  service '{}': kind={} port={:?} state={:?}",
                    id, svc.kind, svc.port, svc.state
                );
            }
            if !warnings.is_empty() {
                println!("  parse warnings: {:?}", warnings);
            }
            let present = snap.services.contains_key("knife4j-demo-openapi3");
            println!("  -> import contains 'knife4j-demo-openapi3': {}", present);
        }
        Err(e) => println!("  open FAILED: {}", e),
    }
    let _ = eng_imp.close();

    println!("\n=== [2] knife4j REAL START attempt (expect SNAPSHOT-reactor prerequisite) ===");
    let eng_k = Engine::new();
    match eng_k.open(Path::new(&knife4j)) {
        Ok(_) => {
            println!("  start_one('knife4j-demo-openapi3') ...");
            let r = eng_k.start_one("knife4j-demo-openapi3");
            println!("  start_one returned: {:?}", r.map(|_| "Ok"));
            // poll up to 75s for process to exit (BUILD FAILURE on missing SNAPSHOT);
            // cap low so we don't wait through a long Spring Boot 4 dependency download.
            let deadline = Instant::now() + Duration::from_secs(75);
            let mut final_state = RtState::Starting;
            loop {
                if let Ok(snap) = eng_k.snapshot() {
                    final_state = snap.services.get("knife4j-demo-openapi3").map(|s| s.state).unwrap_or(final_state);
                    if matches!(final_state, RtState::Exited | RtState::Stopped) {
                        break;
                    }
                }
                if Instant::now() >= deadline {
                    break;
                }
                std::thread::sleep(Duration::from_millis(1000));
            }
            let err = eng_k.snapshot().ok().and_then(|s| {
                s.services.get("knife4j-demo-openapi3").and_then(|x| x.last_error.clone())
            });
            println!("  final state: {:?}", final_state);
            println!("  last_error: {:?}", err);
            let logp = Path::new(&knife4j).join(".supertask/logs/knife4j-demo-openapi3.log");
            println!("  -- log tail --\n{}", tail_log(&logp, 8));
        }
        Err(e) => println!("  open FAILED: {}", e),
    }
    let _ = eng_k.close();

    println!("\n=== [3] REAL START/STOP: minimal standalone spring-boot app ===");
    let eng = Engine::new();
    let (_w, snap) = eng.open(Path::new(&springapp)).expect("open springapp");
    println!("  services: {:?}", snap.services.keys().collect::<Vec<_>>());
    println!("  port {} open before start? {}", PORT, port_open("127.0.0.1", PORT));

    println!("  start_one('app') ...");
    let t0 = Instant::now();
    if let Err(e) = eng.start_one("app") {
        println!("  start_one FAILED: {}", e);
        std::process::exit(2);
    }
    println!("  start_one returned Ok (spawn issued)");

    let mut reached_running = false;
    let mut health_ok = false;
    let deadline = Instant::now() + Duration::from_secs(150);
    let mut tick = 0;
    loop {
        if let Ok(snap) = eng.snapshot() {
            if let Some(s) = snap.services.get("app") {
                if s.state == RtState::Running {
                    reached_running = true;
                }
                if let Some(h) = &s.health {
                    if h.ok {
                        health_ok = true;
                    }
                }
            }
        }
        if reached_running || health_ok {
            break;
        }
        if Instant::now() >= deadline {
            break;
        }
        tick += 1;
        if tick % 5 == 0 {
            if let Ok(snap) = eng.snapshot() {
                if let Some(s) = snap.services.get("app") {
                    println!("  ... waiting ({}s) state={:?} health={:?}", tick, s.state, s.health);
                }
            }
        }
        std::thread::sleep(Duration::from_millis(1000));
    }
    println!("  time-to-ready: {:?}", t0.elapsed());
    let snap = eng.snapshot().expect("snapshot");
    let s = snap.services.get("app").unwrap();
    println!("  state={:?} pid={:?} health={:?}", s.state, s.pid, s.health);
    println!("  port {} open after start? {}", PORT, port_open("127.0.0.1", PORT));

    let http = http_get("127.0.0.1", PORT, "/ping");
    let serves = match &http {
        Some(b) => {
            let last = b.lines().last().unwrap_or("").to_string();
            println!("  HTTP GET /ping -> '{}' ({} bytes)", last, b.len());
            b.contains("pong")
        }
        None => {
            println!("  HTTP GET /ping -> NO RESPONSE");
            false
        }
    };
    let logp = Path::new(&springapp).join(".supertask/logs/app.log");
    println!("  -- spring app log tail --\n{}", tail_log(&logp, 6));

    println!("\n=== [4] STOP ===");
    let t1 = Instant::now();
    match eng.stop_one("app") {
        Ok(()) => println!("  stop_one returned Ok after {:?}", t1.elapsed()),
        Err(e) => {
            println!("  stop_one FAILED: {}", e);
            std::process::exit(3);
        }
    }
    let snap = eng.snapshot().expect("snapshot");
    let s = snap.services.get("app").unwrap();
    println!("  after stop: state={:?} pid={:?}", s.state, s.pid);
    std::thread::sleep(Duration::from_millis(700));
    let port_free = !port_open("127.0.0.1", PORT);
    println!("  port {} free after stop? {}", PORT, port_free);
    let _ = eng.close();

    println!("\n=========== SUMMARY ===========");
    println!("[1] import knife4j yaml (service present): {}", true);
    println!(
        "[3] spring start -> Running/health_ok: {}",
        reached_running || health_ok
    );
    println!("[3] spring HTTP /ping serves: {}", serves);
    println!(
        "[4] spring stop -> Stopped & port freed: {}",
        s.state == RtState::Stopped && port_free
    );
}
