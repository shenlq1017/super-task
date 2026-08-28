use std::fs;
use std::path::{Path, PathBuf};

use indexmap::IndexMap;

use crate::error::{Error, ErrorCode, Result};
use crate::ipc::is_valid_id;
use crate::spec::{HealthSpec, HealthType, PackageManager, ScriptSpec, ServiceSpec, SuperTaskFile};

const SKIP_DIRS: &[&str] = &["node_modules", "target", "dist"];

/// 递归扫描的规模护栏：深度与访问目录数上限，防止超大仓库拖慢打开工作区。
const MAX_DEPTH: usize = 4;
const MAX_VISITED_DIRS: usize = 2000;
const MAX_POM_READS: usize = 500;

/// 1.3 §7.1 compose 文件候选（工作区根，不递归），按优先级降序。
pub const COMPOSE_CANDIDATES: &[&str] = &[
    "compose.yaml",
    "compose.yml",
    "docker-compose.yml",
    "docker-compose.yaml",
];

/// One Maven reactor root discovered during the scan.
struct Reactor {
    /// Relative dir of the pom with `<modules>` ("." for workspace root).
    rel: String,
    /// `<module>` entries as written (may contain `/`).
    modules: Vec<String>,
}

/// 按候选顺序探测工作区根的 compose 文件；都不存在 → None（不是错误）。
pub fn discover_compose_file(root: &Path) -> Option<String> {
    COMPOSE_CANDIDATES
        .iter()
        .find(|c| root.join(c).is_file())
        .map(|c| c.to_string())
}

/// Scan a project tree into a draft spec. Does not write disk.
pub fn scan_draft(root: &Path) -> Result<(SuperTaskFile, Vec<String>)> {
    scan_draft_with_runner(root, &crate::docker::ProcessDockerRunner)
}

/// 可注入 runner 的扫描入口（测试用 fake，不真调 docker）。
pub fn scan_draft_with_runner(
    root: &Path,
    runner: &dyn crate::docker::DockerRunner,
) -> Result<(SuperTaskFile, Vec<String>)> {
    if !root.is_dir() {
        return Err(Error::new(
            ErrorCode::CwdMissing,
            format!("目录不存在: {}", root.display()),
        ));
    }
    let mut warnings = Vec::new();
    let mut services = IndexMap::new();
    let mut port_java = 8080u16;
    let mut spring_ids = Vec::new();

    // ---- Maven：递归找所有 reactor 根（工作区根可以是普通容器目录） ----
    let mut budget = ScanBudget::default();
    let reactors = collect_reactors(root, &mut budget, &mut warnings);
    if budget.exhausted {
        warnings.push(format!(
            "工程过大，扫描在深度 {MAX_DEPTH} / {MAX_VISITED_DIRS} 个目录内截断，结果可能不全"
        ));
    }
    for reactor in &reactors {
        let reactor_dir = root.join(&reactor.rel);
        let parent_text = fs::read_to_string(reactor_dir.join("pom.xml")).unwrap_or_default();
        for module in &reactor.modules {
            let child_pom = reactor_dir.join(module.replace('/', std::path::MAIN_SEPARATOR_STR)).join("pom.xml");
            if !child_pom.is_file() {
                warnings.push(format!("跳过 {module}：无 pom.xml"));
                continue;
            }
            let child = match fs::read_to_string(&child_pom) {
                Ok(t) => t,
                Err(_) => continue,
            };
            if !is_boot_candidate(&child) {
                continue; // 纯库模块 / 无关 POM，静默跳过
            }
            let module_dir = child_pom.parent().unwrap_or(root);
            if !is_launchable_module(&child, module_dir, &mut warnings, module) {
                continue;
            }
            let artifact = project_artifact_id(&child).unwrap_or_else(|| {
                child_pom
                    .parent()
                    .and_then(|p| p.file_name())
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_else(|| module.clone())
            });
            insert_spring_with_cwd(
                &mut services,
                &mut spring_ids,
                &mut port_java,
                reactor.rel.clone(),
                module.clone(),
                &artifact,
                &child,
            );
        }

        // 单模块 reactor：无 <modules> 且自身可启动。launchable 检查静默进行，
        // 因为"未命中"在这里不算用户错误（可能只是个聚合仓库里的普通 POM）。
        if reactor.modules.is_empty() && is_boot_candidate(&parent_text) {
            let module_dir = reactor_dir.as_path();
            if has_boot_app_class(module_dir) || pom_declares_boot_plugin(&parent_text) {
                let artifact = project_artifact_id(&parent_text).unwrap_or_else(|| {
                    reactor_dir
                        .file_name()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_else(|| "app".into())
                });
                insert_spring_with_cwd(
                    &mut services,
                    &mut spring_ids,
                    &mut port_java,
                    reactor.rel.clone(),
                    ".".into(),
                    &artifact,
                    &parent_text,
                );
            } else {
                warnings.push(format!(
                    "{} 含 Spring Boot 依赖但无启动类且无 boot 插件，未生成服务",
                    display_rel(&reactor.rel)
                ));
            }
        }
    }

    scan_node_roots(root, &reactors, &mut services, &mut warnings);

    // 1.3 §7：compose 文件发现 → kind: compose 服务草稿（Docker 不可用 → 警告跳过）
    let docker_spec = scan_compose(root, runner, &mut services, &mut warnings);

    if services.is_empty() {
        return Err(Error::new(
            ErrorCode::NoYaml,
            "未扫描到 spring-boot 模块或 package.json。已支持多层 Maven 工程与嵌套 Node 项目；若服务在更深的目录（>4 层），请直接打开其父工程。",
        ));
    }

    let mut file = SuperTaskFile {
        version: 1,
        kind: Some("workspace".into()),
        name: Some(
            root.file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "workspace".into()),
        ),
        description: None,
        root: ".".into(),
        env: IndexMap::new(),
        services,
        scripts: IndexMap::new(),
        logging: None,
        secrets: None,
        profiles: None,
        toolchain: None,
        templates: None,
        git: None,
        docker: docker_spec,
        gateway: None,
        cloud: None,
        ai: None,
        network: None,
        log_retention: None,
        extra: IndexMap::new(),
    };
    // Maven 工程给一个「安装依赖」引导脚本；cwd 指向首个 reactor 根
    let first_reactor = reactors.iter().map(|r| r.rel.clone()).next();
    if let Some(rel) = first_reactor {
        let cwd = if rel == "." { None } else { Some(rel) };
        file.scripts.insert(
            "bootstrap".into(),
            ScriptSpec {
                desc: Some("安装依赖".into()),
                cmds: vec!["mvn -q -DskipTests install".into()],
                cwd,
                env: IndexMap::new(),
                timeout_secs: Some(1800),
                depends_on: vec![],
            },
        );
    }
    file.apply_defaults();
    Ok((file, warnings))
}

impl ServiceSpec {
    fn default_service() -> Self {
        Self {
            kind: String::new(),
            service: None,
            enabled: true,
            group: None,
            labels: IndexMap::new(),
            port: None,
            ports: vec![],
            env: IndexMap::new(),
            env_file: vec![],
            depends_on: vec![],
            depends_on_ex: None,
            grace_secs: None,
            health: None,
            restart: None,
            extra_args: vec![],
            cwd: None,
            launch: None,
            module: None,
            jvm_args: vec![],
            dir: None,
            package_manager: None,
            script: None,
            logging: None,
            resources: None,
            build_args: vec![],
            extra: IndexMap::new(),
        }
    }
}

fn pom_modules(pom: &str) -> Vec<String> {
    let Some(start) = pom.find("<modules>") else {
        return Vec::new();
    };
    let Some(rel_end) = pom[start..].find("</modules>") else {
        return Vec::new();
    };
    let block = &pom[start..start + rel_end];
    let mut out = Vec::new();
    let mut rest = block;
    while let Some(i) = rest.find("<module>") {
        rest = &rest[i + "<module>".len()..];
        if let Some(j) = rest.find("</module>") {
            let m = rest[..j].trim().replace('\\', "/");
            if !m.is_empty() {
                out.push(m);
            }
            rest = &rest[j + "</module>".len()..];
        } else {
            break;
        }
    }
    out
}

fn insert_spring_with_cwd(
    services: &mut IndexMap<String, ServiceSpec>,
    spring_ids: &mut Vec<String>,
    port: &mut u16,
    reactor_rel: String,
    module_rel: String,
    artifact: &str,
    pom: &str,
) {
    let id = unique_id(&sanitize_id(artifact), services);
    // -pl 在 reactor 根执行；reactor 不在工作区根时 cwd 必须指向它
    let cwd = if reactor_rel == "." { None } else { Some(reactor_rel) };
    services.insert(
        id.clone(),
        ServiceSpec {
            kind: "spring-boot".into(),
            module: Some(module_rel),
            port: Some(*port),
            health: Some(spring_health(*port, pom)),
            grace_secs: Some(45),
            launch: Some("run".into()),
            cwd,
            ..ServiceSpec::default_service()
        },
    );
    spring_ids.push(id);
    *port = port.saturating_add(1);
}

fn spring_health(port: u16, pom: &str) -> HealthSpec {
    if pom_has_actuator(pom) {
        HealthSpec {
            r#type: HealthType::Http,
            http: Some(format!("http://127.0.0.1:{port}/actuator/health")),
            interval_secs: 2,
            timeout_secs: 2,
        }
    } else {
        HealthSpec {
            r#type: HealthType::Tcp,
            http: None,
            interval_secs: 2,
            timeout_secs: 2,
        }
    }
}

fn pom_has_actuator(pom: &str) -> bool {
    pom.contains("spring-boot-starter-actuator") || pom.contains("spring-boot-actuator")
}

/// pom 是否具备成为 boot 服务的潜质（插件声明或含 spring-boot 依赖）。
/// 真正「可启动」还需 [`is_launchable_module`] 的启动类/插件二选一确认。
fn is_boot_candidate(pom: &str) -> bool {
    pom_declares_boot_plugin(pom) || pom.contains("spring-boot")
}

fn pom_declares_boot_plugin(pom: &str) -> bool {
    pom.contains("spring-boot-maven-plugin")
}

/// 可启动判定：boot 插件（用户可能有自定义 main，不强求启动类）
/// 或 存在 @SpringBootApplication 启动类。纯库模块两者皆无 → 跳过并 warning。
fn is_launchable_module(pom: &str, module_dir: &Path, warnings: &mut Vec<String>, label: &str) -> bool {
    if pom_declares_boot_plugin(pom) {
        return true;
    }
    if has_boot_app_class(module_dir) {
        return true;
    }
    warnings.push(format!(
        "{label} 含 Spring Boot 依赖但无 boot 插件也无启动类（疑似库模块），未生成服务"
    ));
    false
}

/// 在 `src/main/java` 下找内容含 @SpringBootApplication 的 .java 文件（文本包含即可）。
fn has_boot_app_class(module_dir: &Path) -> bool {
    let src = module_dir.join("src").join("main").join("java");
    if !src.is_dir() {
        return false;
    }
    let mut found = false;
    visit_java_files(&src, 0, &mut |p| {
        if found {
            return;
        }
        if let Ok(text) = fs::read_to_string(p) {
            if text.contains("@SpringBootApplication") {
                found = true;
            }
        }
    });
    found
}

fn visit_java_files(dir: &Path, depth: usize, f: &mut impl FnMut(&Path)) {
    // 启动类通常在包路径前两层之内；深度护栏防深目录遍历失控
    if depth > MAX_DEPTH + 4 {
        return;
    }
    let Ok(rd) = fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            visit_java_files(&p, depth + 1, f);
        } else if p.extension().map(|x| x == "java").unwrap_or(false) {
            f(&p);
        }
    }
}

/// BFS 收集工作区内所有 Maven reactor 根（含 `<modules>` 的 pom）。
/// 工作区根本身没 pom 也能下钻 —— 典型如后端在 server/ 子目录的单仓库。
fn collect_reactors(root: &Path, budget: &mut ScanBudget, warnings: &mut Vec<String>) -> Vec<Reactor> {
    let mut out = Vec::new();
    let mut queue: Vec<(PathBuf, String)> = vec![(root.to_path_buf(), ".".into())];

    while let Some((dir, rel)) = queue.pop() {
        budget.visited += 1;
        if budget.exceeded() {
            budget.exhausted = true;
            break;
        }
        let pom_path = dir.join("pom.xml");
        let text = match fs::read_to_string(&pom_path) {
            Ok(t) => {
                budget.pom_reads += 1;
                t
            }
            Err(_) => {
                // 无 pom 的目录（含工作区根）：继续下钻找嵌套工程
                push_children(&dir, &rel, &mut queue);
                continue;
            }
        };
        let modules = pom_modules(&text);
        out.push(Reactor { rel: rel.clone(), modules: modules.clone() });
        // 聚合模块要探；同时兄弟目录可能藏着独立工程，也入队
        push_children_filtered(&dir, &rel, &modules, &mut queue);
    }
    warnings.shrink_to_fit();
    out
}

/// 把 dir 的非跳过子目录入队。已作为 pom module 入队的名字不重复入队，
/// 但只匹配第一段（module 可能写成 `sub/child`）。
fn push_children_filtered(
    dir: &Path,
    rel: &str,
    exclude: &[String],
    queue: &mut Vec<(PathBuf, String)>,
) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if !p.is_dir() {
            continue;
        }
        let name = e.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') || SKIP_DIRS.contains(&name.as_str()) || is_pom_hinted_module(&name, exclude) {
            continue;
        }
        let child_rel = join_rel(rel, &name);
        if rel_depth(&child_rel) > MAX_DEPTH {
            continue;
        }
        queue.push((p, child_rel));
    }
}

fn push_children(dir: &Path, rel: &str, queue: &mut Vec<(PathBuf, String)>) {
    push_children_filtered(dir, rel, &[], queue);
}

/// module 条目可能是 `sub/child` 形式，exclude 匹配第一段即可避免重复下钻。
fn is_pom_hinted_module(name: &str, exclude: &[String]) -> bool {
    exclude.iter().any(|m| m.split('/').next() == Some(name))
}

fn join_rel(base: &str, seg: &str) -> String {
    if base == "." {
        seg.replace('\\', "/")
    } else {
        format!("{base}/{}", seg.replace('\\', "/"))
    }
}

fn rel_depth(rel: &str) -> usize {
    if rel == "." { 0 } else { rel.split('/').count() }
}

#[derive(Default)]
struct ScanBudget {
    visited: usize,
    pom_reads: usize,
    exhausted: bool,
}

impl ScanBudget {
    fn exceeded(&self) -> bool {
        self.visited > MAX_VISITED_DIRS || self.pom_reads > MAX_POM_READS
    }
}

fn display_rel(rel: &str) -> &str {
    rel
}

/// Skip `<parent>` so we don't pick the parent's artifactId.
fn project_artifact_id(pom: &str) -> Option<String> {
    let stripped = strip_parent_block(pom);
    first_artifact_id(&stripped)
}

fn strip_parent_block(pom: &str) -> String {
    let Some(start) = pom.find("<parent>") else {
        return pom.to_string();
    };
    let Some(rel_end) = pom[start..].find("</parent>") else {
        return pom.to_string();
    };
    let mut out = String::with_capacity(pom.len());
    out.push_str(&pom[..start]);
    out.push_str(&pom[start + rel_end + "</parent>".len()..]);
    out
}

fn first_artifact_id(pom: &str) -> Option<String> {
    let i = pom.find("<artifactId>")?;
    let rest = &pom[i + 12..];
    let j = rest.find("</artifactId>")?;
    Some(rest[..j].trim().to_string())
}

fn sanitize_id(raw: &str) -> String {
    let mut s: String = raw
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '-' })
        .collect();
    if s.is_empty() || !s.as_bytes()[0].is_ascii_alphabetic() {
        s = format!("svc-{s}");
    }
    if !is_valid_id(&s) {
        s = "svc".into();
    }
    s
}

fn unique_id(base: &str, existing: &IndexMap<String, ServiceSpec>) -> String {
    if !existing.contains_key(base) {
        return base.into();
    }
    for i in 2..99 {
        let c = format!("{base}-{i}");
        if !existing.contains_key(&c) {
            return c;
        }
    }
    format!("{base}-x")
}

fn pkg_has_dev_or_start(txt: &str) -> bool {
    pkg_has_script(txt, "dev") || pkg_has_script(txt, "start")
}

/// 递归发现可运行的 Node 服务（≤MAX_DEPTH 层）。
/// 含 `workspaces` 字段的 package.json 是 monorepo 管理文件，自身不算服务；
/// 无 dev/start script 的（如纯构建配置包）跳过并提示。
fn scan_node_roots(
    root: &Path,
    _reactors: &[Reactor],
    services: &mut IndexMap<String, ServiceSpec>,
    warnings: &mut Vec<String>,
) {
    let spring_ids: Vec<String> = services
        .iter()
        .filter(|(_, s)| s.kind == "spring-boot")
        .map(|(id, _)| id.clone())
        .collect();

    let mut port_node = 5173u16;
    // BFS 收集所有含 package.json 的目录，浅层优先保证排序稳定（根最前）
    let mut dirs: Vec<String> = Vec::new();
    collect_pkg_dirs(root, root, 0, &mut dirs);

    for rel in dirs {
        let dir_path = if rel == "." {
            root.to_path_buf()
        } else {
            root.join(rel.replace('/', std::path::MAIN_SEPARATOR_STR))
        };
        let Ok(txt) = fs::read_to_string(dir_path.join("package.json")) else {
            continue;
        };
        // monorepo 根：只负责管理 workspaces，不是可启动服务
        if txt.contains("\"workspaces\"") {
            continue;
        }
        // Java 工程里的 "spring" 依赖说明是 spring boot node 工具包误报
        if txt.contains("\"spring\"") && !pkg_has_dev_or_start(&txt) {
            continue;
        }
        let script = if pkg_has_script(&txt, "dev") {
            "dev"
        } else if pkg_has_script(&txt, "start") {
            "start"
        } else {
            warnings.push(format!("{}{} 无 dev/start script，生成后需手选", display_rel("."), rel));
            "dev"
        };
        let pm = detect_pm(root, Path::new(&rel), &txt);
        let id_src = if rel == "." {
            "web".to_string()
        } else {
            rel.rsplit('/').next().unwrap_or(&rel).to_string()
        };
        let id = unique_id(&sanitize_id(&id_src), services);
        let mut spec = ServiceSpec::default_service();
        spec.kind = "node".into();
        spec.dir = Some(rel.clone());
        spec.port = Some(port_node);
        spec.script = Some(script.into());
        spec.package_manager = Some(pm);
        spec.grace_secs = Some(15);
        spec.health = Some(HealthSpec {
            r#type: HealthType::Tcp,
            http: None,
            interval_secs: 2,
            timeout_secs: 2,
        });
        spec.depends_on = spring_ids.clone();
        services.insert(id, spec);
        port_node = port_node.saturating_add(1);
    }
}

/// 收集含 package.json 的目录相对路径（"." 开头代表根）。浅层优先。
fn collect_pkg_dirs(root: &Path, dir: &Path, depth: usize, out: &mut Vec<String>) {
    if depth > MAX_DEPTH {
        return;
    }
    if dir.join("package.json").is_file() {
        out.push(if depth == 0 { ".".into() } else {
            dir.strip_prefix(root)
                .unwrap_or(dir)
                .to_string_lossy()
                .replace('\\', "/")
        });
    }
    let Ok(rd) = fs::read_dir(dir) else { return };
    let mut children: Vec<_> = rd.flatten().map(|e| e.path()).collect();
    children.sort(); // 稳定顺序 → 稳定的服务 ID/端口分配
    for p in children {
        if !p.is_dir() {
            continue;
        }
        let name = p.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        if name.starts_with('.') || SKIP_DIRS.contains(&name.as_str()) {
            continue;
        }
        collect_pkg_dirs(root, &p, depth + 1, out);
    }
}

fn pkg_has_script(txt: &str, name: &str) -> bool {
    txt.contains(&format!("\"{name}\"")) && txt.contains("\"scripts\"")
}

// ============================================================================
// 1.3 §7 compose 导入：文件发现 → config 解析 → kind: compose 草稿
// ============================================================================

/// compose 文件发现与草稿生成。返回写入草稿的 `docker` 段（无 compose 文件
/// 或 Docker 不可用时为 None——不是错误，其余扫描照常）。
fn scan_compose(
    root: &Path,
    runner: &dyn crate::docker::DockerRunner,
    services: &mut IndexMap<String, ServiceSpec>,
    warnings: &mut Vec<String>,
) -> Option<crate::spec::DockerSpec> {
    let present: Vec<&str> = COMPOSE_CANDIDATES
        .iter()
        .copied()
        .filter(|c| root.join(c).is_file())
        .collect();
    if present.is_empty() {
        return None; // 没有 compose 文件：不产生 compose 草稿（§7.1）
    }
    let chosen = present[0];
    if present.len() > 1 {
        warnings.push(format!(
            "多个 compose 文件并存：{}；采用优先级最高的 {chosen}，并显式写入 docker.compose_file",
            present.join("、")
        ));
    }

    // 服务清单：`docker compose config --format json`（与 ComposeConfigLoader 同 argv）。
    // Docker 不可用 → 整段跳过并警告（§7.2），不让扫描在无 docker 机器上失败。
    let model = match compose_config_model(root, chosen, runner) {
        Ok(m) => m,
        Err(e) => {
            // 错误码随警告透出（DOCKER_NOT_FOUND / COMPOSE_CONFIG_FAILED / …）
            let code = serde_yaml::to_string(&e.code())
                .unwrap_or_default()
                .trim()
                .to_string();
            warnings.push(format!("跳过 compose 导入（{chosen}）：[{code}] {e}"));
            return None;
        }
    };

    // 第一遍：id 分配（compose 服务名 → 候选 id，冲突时 unique_id 兜底）
    let mut id_map: IndexMap<String, String> = IndexMap::new();
    for svc in &model.services {
        let (base, renamed) = sanitize_compose_id(&svc.name);
        if renamed {
            warnings.push(format!(
                "compose 服务 {:?} 含非法 id 字符，替换为 {base:?}",
                svc.name
            ));
        }
        let id = unique_id(&base, services);
        id_map.insert(svc.name.clone(), id);
    }
    // 第二遍：草稿字段（port = ports[0].published；depends_on 键映射；build 标记）
    for svc in &model.services {
        let id = id_map.get(&svc.name).cloned().unwrap_or_default();
        let mut depends_on = Vec::new();
        for dep in &svc.depends_on {
            match id_map.get(dep) {
                Some(dep_id) => depends_on.push(dep_id.clone()),
                None => warnings.push(format!(
                    "compose 服务 {:?} 的 depends_on {dep:?} 未在 compose 文件中定义，已丢弃",
                    svc.name
                )),
            }
        }
        let mut labels = IndexMap::new();
        if svc.has_build {
            labels.insert("supertask.docker.build".into(), "true".into());
        }
        services.insert(
            id,
            ServiceSpec {
                kind: "compose".into(),
                service: Some(svc.name.clone()),
                port: svc.port,
                depends_on,
                labels,
                ..ServiceSpec::default_service()
            },
        );
    }
    Some(crate::spec::DockerSpec {
        compose_file: Some(chosen.to_string()),
        project_name: None,
        builds: Vec::new(),
        extra: Default::default(),
    })
}

/// `docker compose --ansi never -f <file> config --format json`，扫描专用（无缓存）。
fn compose_config_model(
    root: &Path,
    compose_file: &str,
    runner: &dyn crate::docker::DockerRunner,
) -> Result<crate::docker::ComposeModel> {
    let abs = crate::sandbox::confine(root, compose_file)?;
    let args = crate::docker::compose_base_args(&abs, None);
    let mut args = args;
    args.push("config".into());
    args.push("--format".into());
    args.push("json".into());
    let out = runner
        .run(&crate::docker::DockerSpawn {
            args,
            cwd: Some(root.to_path_buf()),
            timeout: std::time::Duration::from_secs(10),
        })
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                Error::new(
                    ErrorCode::DockerNotFound,
                    "未找到 docker。请安装 Docker Desktop 并确保在 PATH 中。",
                )
            } else {
                Error::new(ErrorCode::ComposeConfigFailed, format!("docker compose config 执行失败: {e}"))
            }
        })?;
    if out.code != 0 {
        return Err(Error::new(
            ErrorCode::ComposeConfigFailed,
            format!("docker compose config 退出码 {}", out.code),
        ));
    }
    crate::docker::parse_compose_config(&out.stdout)
}

/// compose 服务名 → 合法 SuperTask id：非法字符替换 `_`；仍不合法（数字开头等）
/// 加 `svc-` 前缀。返回 (id, 是否被改写)。
fn sanitize_compose_id(raw: &str) -> (String, bool) {
    let cleaned: String = raw
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let changed = cleaned != raw;
    if is_valid_id(&cleaned) {
        return (cleaned, changed);
    }
    let prefixed = format!("svc-{cleaned}");
    if is_valid_id(&prefixed) {
        return (prefixed, true);
    }
    ("svc".into(), true)
}

fn detect_pm(root: &Path, dir: &Path, pkg_txt: &str) -> PackageManager {
    if pkg_txt.contains("\"packageManager\"") && pkg_txt.contains("pnpm") {
        return PackageManager::Pnpm;
    }
    if pkg_txt.contains("\"packageManager\"") && pkg_txt.contains("yarn") {
        return PackageManager::Yarn;
    }
    let base = if dir == Path::new(".") {
        root.to_path_buf()
    } else {
        root.join(dir)
    };
    if base.join("pnpm-lock.yaml").is_file() {
        PackageManager::Pnpm
    } else if base.join("yarn.lock").is_file() {
        PackageManager::Yarn
    } else {
        PackageManager::Npm
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scans_spring_and_node() {
        let root = std::env::temp_dir().join(format!("st-scan-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("user-service")).unwrap();
        fs::create_dir_all(root.join("lib")).unwrap();
        fs::create_dir_all(root.join("web")).unwrap();
        fs::write(
            root.join("pom.xml"),
            r#"<project><modules><module>user-service</module><module>lib</module></modules></project>"#,
        )
        .unwrap();
        fs::write(
            root.join("user-service/pom.xml"),
            r#"<project><artifactId>user-service</artifactId><build><plugins><plugin><artifactId>spring-boot-maven-plugin</artifactId></plugin></plugins></build></project>"#,
        )
        .unwrap();
        fs::write(
            root.join("lib/pom.xml"),
            r#"<project><artifactId>lib</artifactId><packaging>jar</packaging></project>"#,
        )
        .unwrap();
        fs::write(
            root.join("web/package.json"),
            r#"{"name":"web","scripts":{"dev":"vite"}}"#,
        )
        .unwrap();
        let (file, _) = scan_draft(&root).unwrap();
        assert!(file.services.contains_key("user-service"));
        assert!(!file.services.contains_key("lib"));
        let web = file.services.values().find(|s| s.kind == "node").unwrap();
        assert_eq!(web.dir.as_deref(), Some("web"));
        assert_eq!(web.depends_on, vec!["user-service"]);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn skips_parent_artifact_id() {
        let pom = r#"
<project>
  <parent>
    <groupId>com.example</groupId>
    <artifactId>knife4j</artifactId>
    <version>1</version>
  </parent>
  <artifactId>knife4j-demo-openapi3</artifactId>
  <build><plugins><plugin><artifactId>spring-boot-maven-plugin</artifactId></plugin></plugins></build>
</project>
"#;
        assert_eq!(
            project_artifact_id(pom).as_deref(),
            Some("knife4j-demo-openapi3")
        );
    }

    #[test]
    fn parent_collision_keeps_both_modules() {
        let root = std::env::temp_dir().join(format!("st-scan-parent-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("demo-a")).unwrap();
        fs::create_dir_all(root.join("demo-b")).unwrap();
        fs::write(
            root.join("pom.xml"),
            r#"<project><modules><module>demo-a</module><module>demo-b</module></modules></project>"#,
        )
        .unwrap();
        let child = r#"<project>
  <parent><artifactId>umbrella</artifactId></parent>
  <artifactId>PLACEHOLDER</artifactId>
  <build><plugins><plugin><artifactId>spring-boot-maven-plugin</artifactId></plugin></plugins></build>
</project>"#;
        fs::write(
            root.join("demo-a/pom.xml"),
            child.replace("PLACEHOLDER", "knife4j-demo-openapi3"),
        )
        .unwrap();
        fs::write(
            root.join("demo-b/pom.xml"),
            child.replace("PLACEHOLDER", "knife4j-demo-openapi2"),
        )
        .unwrap();
        let (file, _) = scan_draft(&root).unwrap();
        assert!(file.services.contains_key("knife4j-demo-openapi3"));
        assert!(file.services.contains_key("knife4j-demo-openapi2"));
        let a = file.services.get("knife4j-demo-openapi3").unwrap();
        assert_eq!(a.module.as_deref(), Some("demo-a"));
        assert_eq!(
            file.services.get("knife4j-demo-openapi2").unwrap().module.as_deref(),
            Some("demo-b")
        );
        assert_eq!(a.health.as_ref().unwrap().r#type, HealthType::Tcp);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn single_module_and_parent_hint() {
        let root = std::env::temp_dir().join(format!("st-scan-single-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("pom.xml"),
            r#"<project>
  <parent><groupId>g</groupId><artifactId>p</artifactId><version>1</version></parent>
  <artifactId>demo-app</artifactId>
  <build><plugins><plugin><artifactId>spring-boot-maven-plugin</artifactId></plugin></plugins></build>
</project>"#,
        )
        .unwrap();
        let (file, _warnings) = scan_draft(&root).unwrap();
        // 单模块（带本地 parent）：module="." 省略 -pl，cwd 不需要 reactor 根，可直接启动
        let svc = file.services.get("demo-app").unwrap();
        assert_eq!(svc.module.as_deref(), Some("."));
        assert_eq!(svc.cwd, None);
        assert_eq!(file.scripts.get("bootstrap").map(|s| s.cwd.clone()), Some(None));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn nested_reactor_under_subdir() {
        // 单仓库：工作区根无 pom，Maven reactor 在 server/ 子目录
        let root = std::env::temp_dir().join(format!("st-scan-nested-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("server/nest-store-bootstrap")).unwrap();
        fs::create_dir_all(root.join("server/nest-store-common")).unwrap();
        fs::create_dir_all(root.join("app")).unwrap(); // 非 node 前端，不应产生服务
        fs::write(
            root.join("server/pom.xml"),
            r#"<project><artifactId>nest-store</artifactId>
            <modules><module>nest-store-api</module><module>nest-store-common</module><module>nest-store-bootstrap</module></modules></project>"#,
        )
        .unwrap();
        fs::write(
            root.join("server/nest-store-common/pom.xml"),
            r#"<project><parent><relativePath/><artifactId>p</artifactId></parent><artifactId>nest-store-common</artifactId><packaging>jar</packaging></project>"#,
        )
        .unwrap();
        // bootstrap：无 boot 插件但有启动类 → 依据「两者结合」规则仍可启动
        fs::write(
            root.join("server/nest-store-bootstrap/pom.xml"),
            r#"<project><artifactId>nest-store-bootstrap</artifactId>
            <dependencies><dependency><artifactId>spring-boot-starter-web</artifactId></dependency></dependencies></project>"#,
        )
        .unwrap();
        let app_java = root.join("server/nest-store-bootstrap/src/main/java/com/neststore");
        fs::create_dir_all(&app_java).unwrap();
        fs::write(app_java.join("NestStoreApplication.java"), "@SpringBootApplication\npublic class NestStoreApplication {}").unwrap();

        let (file, _warnings) = scan_draft(&root).unwrap();
        let boot = file.services.get("nest-store-bootstrap").expect("bootstrap 未被识别");
        // cwd 指向 reactor 子目录；-pl 路径相对 reactor
        assert_eq!(boot.cwd.as_deref(), Some("server"));
        assert_eq!(boot.module.as_deref(), Some("nest-store-bootstrap"));
        // 库模块：pom 完全无 spring-boot 痕迹 → 静默跳过（不算用户错误）
        assert!(!file.services.contains_key("nest-store-common"));
        // 非 node 的 app/ 目录不误报
        assert_eq!(file.services.len(), 1);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn deep_node_monorepo() {
        // apps/web 二层 node 工程 + workspaces 根不算服务
        let root = std::env::temp_dir().join(format!("st-scan-monorepo-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("apps/web")).unwrap();
        fs::write(root.join("package.json"), r#"{"name":"repo","workspaces":["apps/*"]}"#).unwrap();
        fs::write(root.join("apps/web/package.json"), r#"{"name":"web","scripts":{"dev":"vite"}}"#).unwrap();
        let (file, _) = scan_draft(&root).unwrap();
        // workspaces 根自身不是服务；只有 apps/web 一个 node 服务
        assert_eq!(file.services.len(), 1);
        assert!(!file.services.values().any(|s| s.dir.as_deref() == Some(".")));
        let web = file.services.values().find(|s| s.kind == "node").expect("深层 web 未识别");
        assert_eq!(web.dir.as_deref(), Some("apps/web"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn library_module_without_launch_class_skipped_with_warning() {
        // 有 spring-boot 依赖、无插件、无启动类 → 跳过 + warning
        let root = std::env::temp_dir().join(format!("st-scan-lib-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("core-lib")).unwrap();
        fs::write(root.join("pom.xml"), r#"<project><modules><module>core-lib</module></modules></project>"#).unwrap();
        fs::write(
            root.join("core-lib/pom.xml"),
            r#"<project><artifactId>core-lib</artifactId>
            <dependencies><dependency><artifactId>spring-boot-starter-data-redis</artifactId></dependency></dependencies></project>"#,
        )
        .unwrap();
        let result = scan_draft(&root);
        match result {
            Ok((file, warnings)) => {
                assert!(file.services.is_empty());
                assert!(warnings.iter().any(|w| w.contains("core-lib")));
            }
            Err(e) => assert_eq!(e.code(), ErrorCode::NoYaml), // 全跳空时是正常报错路径
        }
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn actuator_uses_http_health() {
        let root = std::env::temp_dir().join(format!("st-scan-act-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("api")).unwrap();
        fs::write(
            root.join("pom.xml"),
            r#"<project><modules><module>api</module></modules></project>"#,
        )
        .unwrap();
        fs::write(
            root.join("api/pom.xml"),
            r#"<project><artifactId>api</artifactId>
            <dependencies><dependency><artifactId>spring-boot-starter-actuator</artifactId></dependency></dependencies>
            <build><plugins><plugin><artifactId>spring-boot-maven-plugin</artifactId></plugin></plugins></build></project>"#,
        )
        .unwrap();
        let (file, _) = scan_draft(&root).unwrap();
        let h = file.services.get("api").unwrap().health.as_ref().unwrap();
        assert_eq!(h.r#type, HealthType::Http);
        assert!(h.http.as_deref().unwrap().contains("/actuator/health"));
        let _ = fs::remove_dir_all(&root);
    }

    // ---- 1.3 §7 compose 导入 ----

    fn compose_config_fixture() -> String {
        r#"{
          "services": {
            "redis": {
              "image": "redis:7",
              "ports": [{"mode": "ingress", "target": 6379, "published": 6379, "protocol": "tcp"}],
              "build": {"context": "."}
            },
            "mysql": {
              "image": "mysql:8",
              "ports": [{"mode": "ingress", "target": 3306, "published": 3306, "protocol": "tcp"}],
              "depends_on": ["redis", "ghost"]
            },
            "my.db_1": {
              "image": "pg:16"
            }
          }
        }"#
        .to_string()
    }

    #[test]
    fn compose_candidates_and_draft_fields() {
        let root = std::env::temp_dir().join(format!("st-scan-comp-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("compose.yaml"), "services:\n  redis:\n    image: redis:7\n").unwrap();
        let fake = crate::docker::FakeDockerRunner::new();
        fake.push_ok(compose_config_fixture());
        let (file, warnings) = scan_draft_with_runner(&root, &fake).unwrap();

        // compose_file 显式写入草稿
        let docker = file.docker.as_ref().expect("docker section");
        assert_eq!(docker.compose_file.as_deref(), Some("compose.yaml"));

        // port = ports[0].published；build → labels 标记
        let redis = file.services.get("redis").expect("redis candidate");
        assert_eq!(redis.kind, "compose");
        assert_eq!(redis.service.as_deref(), Some("redis"));
        assert_eq!(redis.port, Some(6379));
        assert_eq!(
            redis.labels.get("supertask.docker.build").map(String::as_str),
            Some("true")
        );

        // depends_on 键映射；引用不存在 id 丢弃并警告
        let mysql = file.services.get("mysql").unwrap();
        assert_eq!(mysql.depends_on, vec!["redis".to_string()]);
        assert!(warnings.iter().any(|w| w.contains("ghost")));

        // 非法 id 字符替换 `_` 并警告
        let pg = file.services.get("my_db_1").expect("sanitized id");
        assert_eq!(pg.service.as_deref(), Some("my.db_1"));
        assert!(warnings.iter().any(|w| w.contains("my.db_1")));

        // config argv 固定：compose --ansi never -f <file> config --format json
        let calls = fake.calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(&calls[0].args[0..4], &["compose", "--ansi", "never", "-f"]);
        assert_eq!(&calls[0].args[5..], &["config", "--format", "json"]);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn compose_priority_and_multiple_files_warn() {
        let root = std::env::temp_dir().join(format!("st-scan-comppri-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        for name in ["compose.yaml", "compose.yml", "docker-compose.yml", "docker-compose.yaml"] {
            fs::write(root.join(name), "services:\n  redis:\n    image: redis:7\n").unwrap();
        }
        let fake = crate::docker::FakeDockerRunner::new();
        fake.push_ok(compose_config_fixture());
        let (file, warnings) = scan_draft_with_runner(&root, &fake).unwrap();
        assert_eq!(
            file.docker.as_ref().unwrap().compose_file.as_deref(),
            Some("compose.yaml")
        );
        assert!(warnings.iter().any(|w| w.contains("docker-compose.yaml") && w.contains("compose.yaml")));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn compose_docker_unavailable_is_warning_not_error() {
        let root = std::env::temp_dir().join(format!("st-scan-compnd-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(root.join("api")).unwrap();
        fs::write(
            root.join("pom.xml"),
            r#"<project><modules><module>api</module></modules></project>"#,
        )
        .unwrap();
        fs::write(
            root.join("api/pom.xml"),
            r#"<project><artifactId>api</artifactId><build><plugins><plugin><artifactId>spring-boot-maven-plugin</artifactId></plugin></plugins></build></project>"#,
        )
        .unwrap();
        fs::write(root.join("compose.yaml"), "services:\n  redis:\n    image: redis:7\n").unwrap();
        // PATH 无 docker → DOCKER_NOT_FOUND 警告，其余扫描照常
        let fake = crate::docker::FakeDockerRunner::new();
        fake.push_err(std::io::ErrorKind::NotFound);
        let (file, warnings) = scan_draft_with_runner(&root, &fake).unwrap();
        assert!(file.docker.is_none(), "Docker 不可用时不写 docker 段");
        assert!(warnings.iter().any(|w| w.contains("DOCKER_NOT_FOUND")), "{warnings:?}");
        assert!(file.services.contains_key("api"), "其余扫描照常");
        assert!(!file.services.values().any(|s| s.kind == "compose"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn compose_unparseable_config_warns_and_skips() {
        let root = std::env::temp_dir().join(format!("st-scan-compbad-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("compose.yaml"), "services:\n  redis:\n    image: redis:7\n").unwrap();
        let fake = crate::docker::FakeDockerRunner::new();
        fake.push_fail(1, "invalid compose file");
        // compose 段被跳过后无任何服务：既有工作区走 NoYaml 错误路径，二者都合法
        match scan_draft_with_runner(&root, &fake) {
            Ok((file, warnings)) => {
                assert!(file.docker.is_none());
                assert!(warnings.iter().any(|w| w.contains("invalid compose file")), "{warnings:?}");
            }
            Err(e) => assert_eq!(e.code(), ErrorCode::NoYaml),
        }
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn compose_only_workspace_scan_ok() {
        // 只有 compose 文件、无 Maven/Node → 也能产出草稿
        let root = std::env::temp_dir().join(format!("st-scan-componly-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("compose.yaml"), "services:\n  redis:\n    image: redis:7\n").unwrap();
        let fake = crate::docker::FakeDockerRunner::new();
        fake.push_ok(compose_config_fixture());
        let (file, _) = scan_draft_with_runner(&root, &fake).unwrap();
        assert_eq!(file.services.len(), 3);
        assert!(file.docker.as_ref().unwrap().compose_file.is_some());
        let _ = fs::remove_dir_all(&root);
    }
}
