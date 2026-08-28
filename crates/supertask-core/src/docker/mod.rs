//! 1.3 Docker CLI 适配层（规格 §4）。
//!
//! 职责边界：只做 argv 组装、`--format json` 输出解析、超时/输出上限与
//! `DOCKER_*` 错误映射；不含状态机、拓扑、端口检查等业务规则（复用现有实现）。
//! 全部通过 [`DockerRunner`] spawn 固定程序 `docker`，测试用 fake 注入，不真调 docker。

pub mod build;
pub mod compose_config;
pub mod probe;
pub mod ps;
pub mod runner;

pub use build::{compose_base_args, plan_build_entry, plan_compose_build};
pub use compose_config::{
    parse_compose_config, ComposeConfigLoader, ComposeModel, ComposeServiceInfo,
};
pub use probe::{ensure_compose_ready, probe_docker, DockerProbe};
pub use ps::{parse_images, parse_ps, PsContainer};
pub use runner::{
    DockerOutput, DockerRunner, DockerSpawn, DockerStream, FakeDockerRunner, ProcessDockerRunner,
    DOCKER_PROGRAM, OUTPUT_CAP_BYTES,
};
