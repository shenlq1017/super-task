//! SuperTask engine. Spec: `docs/spec/`.

pub mod appdata;
pub mod discover;
pub mod docker;
pub mod engine;
pub mod error;
pub mod features;
pub mod git;
pub mod graph;pub mod health;
pub mod ide;
pub mod ipc;
pub mod launcher;
pub mod log;
pub mod merge;
pub mod metrics;
pub mod network;
pub mod operation;
pub mod ports;
pub mod proc;
pub mod probe;
pub mod profiles;
pub mod runtime;
pub mod sandbox;
pub mod scan;
pub mod secrets;
pub mod spec;
pub mod taskfile;
pub mod template;
pub mod toolchain;

pub use engine::{
    Engine, EngineEvent, ExitView, HealthView, RuntimeSnapshot, ScriptRuntimeView,
    ServiceRuntimeView, YamlView,
};
pub use error::{Error, ErrorCode};
pub use features::{features, Feature, FeatureStatus};
pub use probe::{probe_toolchain, ToolchainProbe};
pub use sandbox::{confine, strip_verbatim};
pub use scan::scan_draft;
pub use spec::{parse_yaml, spec_hash, to_yaml, SuperTaskFile};
