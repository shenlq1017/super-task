//! 测试专用 node 桩工作区（§13.2 stub 纪律：进程内创建文件，spawn 的只有
//! npm/node 桩本身；测试结束前先停引擎/清场，再删目录）。
//!
//! 两种桩：`listen: true` 起一个监听端口的 http 服务（健康可达）；
//! `listen: false` 只 sleep（永不健康，用于超时/失败路径）。
//! 环境 无 node/npm 时调用方应跳过（CI 三平台与开发机均预装 node）。

#![cfg(test)]

pub mod node_stub {
    use std::path::PathBuf;
    use std::process::Command;

    pub fn node_available() -> bool {
        Command::new("node")
            .arg("--version")
            .output()
            .is_ok_and(|o| o.status.success())
    }

    pub struct StubWs {
        pub root: PathBuf,
        pub port: u16,
    }

    pub fn write_ws(name: &str, port: u16, listen: bool) -> StubWs {
        let root = std::env::temp_dir().join(format!("st-stub-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let web = root.join("web");
        std::fs::create_dir_all(&web).unwrap();
        std::fs::write(
            root.join("supertask.yaml"),
            format!(
                "version: 1\nname: stub\nservices:\n  web:\n    kind: node\n    dir: web\n    script: start\n    port: {port}\n    health:\n      type: tcp\n"
            ),
        )
        .unwrap();
        std::fs::write(
            web.join("package.json"),
            r#"{ "name": "stub", "scripts": { "start": "node server.js" } }"#,
        )
        .unwrap();
        let server = if listen {
            format!("require('http').createServer((q,s)=>s.end('ok')).listen({port});\n")
        } else {
            // 永不监听：只保活进程，让 tcp 健康检查失败
            "setInterval(()=>{}, 1000);\n".to_string()
        };
        std::fs::write(web.join("server.js"), server).unwrap();
        StubWs { root, port }
    }

    /// 清理桩工作区（调用前必须已 stop_all/close，无存活桩进程）。
    pub fn cleanup(ws: &StubWs) {
        let _ = std::fs::remove_dir_all(&ws.root);
    }
}
