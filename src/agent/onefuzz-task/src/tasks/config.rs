// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

#![allow(clippy::large_enum_variant)]
#[cfg(any(target_os = "linux", target_os = "windows"))]
use crate::tasks::coverage;
use crate::tasks::{
    analysis, fuzz,
    heartbeat::{init_task_heartbeat, TaskHeartbeatClient},
    merge, regression, report,
};
use anyhow::Result;
use ipc_channel::ipc::{self, IpcOneShotServer, IpcReceiver, IpcSender};
use onefuzz::machine_id::MachineIdentity;
use onefuzz_telemetry::{
    self as telemetry, Event::task_start, EventData, InstanceTelemetryKey, MicrosoftTelemetryKey,
    Role,
};
use reqwest::Url;
use serde::{self, Deserialize};
use std::{path::PathBuf, sync::Arc, time::Duration};
use uuid::Uuid;

const DEFAULT_MIN_AVAILABLE_MEMORY_MB: u64 = 100;

fn default_min_available_memory_mb() -> u64 {
    DEFAULT_MIN_AVAILABLE_MEMORY_MB
}

#[derive(Debug, Deserialize, PartialEq, Eq, Clone)]
pub enum ContainerType {
    #[serde(alias = "inputs")]
    Inputs,
}

#[derive(Debug, Deserialize, Clone)]
pub struct CommonConfig {
    pub job_id: Uuid,

    pub task_id: Uuid,

    pub instance_id: Uuid,

    pub heartbeat_queue: Option<Url>,

    pub instance_telemetry_key: Option<InstanceTelemetryKey>,

    pub microsoft_telemetry_key: Option<MicrosoftTelemetryKey>,

    pub logs: Option<Url>,

    #[serde(default)]
    pub setup_dir: PathBuf,

    /// Lower bound on available system memory. If the available memory drops
    /// below the limit, the task will exit with an error. This is a fail-fast
    /// mechanism to support debugging.
    ///
    /// Can be disabled by setting to 0.
    #[serde(default = "default_min_available_memory_mb")]
    pub min_available_memory_mb: u64,

    pub machine_identity: MachineIdentity,

    pub from_agent_to_task_endpoint: Option<String>,
    pub from_task_to_agent_endpoint: Option<String>,
}

impl CommonConfig {
    pub async fn init_heartbeat(
        &self,
        initial_delay: Option<Duration>,
    ) -> Result<Option<TaskHeartbeatClient>> {
        match &self.heartbeat_queue {
            Some(url) => {
                let hb = init_task_heartbeat(
                    url.clone(),
                    self.task_id,
                    self.job_id,
                    initial_delay,
                    self.machine_identity.machine_id,
                    self.machine_identity.machine_name.clone(),
                )
                .await?;
                Ok(Some(hb))
            }
            None => Ok(None),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(tag = "task_type")]
pub enum Config {
    #[cfg(any(target_os = "linux", target_os = "windows"))]
    #[serde(alias = "coverage")]
    Coverage(coverage::generic::Config),

    #[cfg(any(target_os = "linux", target_os = "windows"))]
    #[serde(alias = "dotnet_coverage")]
    DotnetCoverage(coverage::dotnet::Config),

    #[serde(alias = "dotnet_crash_report")]
    DotnetCrashReport(report::dotnet::generic::Config),

    #[cfg(any(target_os = "linux", target_os = "windows"))]
    #[serde(alias = "libfuzzer_dotnet_fuzz")]
    LibFuzzerDotnetFuzz(fuzz::libfuzzer::dotnet::Config),

    #[serde(alias = "libfuzzer_fuzz")]
    LibFuzzerFuzz(fuzz::libfuzzer::generic::Config),

    #[serde(alias = "libfuzzer_crash_report")]
    LibFuzzerReport(report::libfuzzer_report::Config),

    #[serde(alias = "libfuzzer_merge")]
    LibFuzzerMerge(merge::libfuzzer_merge::Config),

    #[serde(alias = "libfuzzer_regression")]
    LibFuzzerRegression(regression::libfuzzer::Config),

    #[serde(alias = "generic_analysis")]
    GenericAnalysis(analysis::generic::Config),

    #[serde(alias = "generic_generator")]
    GenericGenerator(fuzz::generator::Config),

    #[serde(alias = "generic_supervisor")]
    GenericSupervisor(fuzz::supervisor::SupervisorConfig),

    #[serde(alias = "generic_merge")]
    GenericMerge(merge::generic::Config),

    #[serde(alias = "generic_crash_report")]
    GenericReport(report::generic::Config),

    #[serde(alias = "generic_regression")]
    GenericRegression(regression::generic::Config),
}

impl Config {
    pub fn from_file(path: PathBuf, setup_dir: PathBuf) -> Result<Self> {
        let json = std::fs::read_to_string(path)?;
        let json_config: serde_json::Value = serde_json::from_str(&json)?;

        // override the setup_dir in the config file with the parameter value if specified
        let mut config: Self = serde_json::from_value(json_config)?;
        config.common_mut().setup_dir = setup_dir;

        Ok(config)
    }

    fn common_mut(&mut self) -> &mut CommonConfig {
        match self {
            #[cfg(any(target_os = "linux", target_os = "windows"))]
            Config::Coverage(c) => &mut c.common,
            #[cfg(any(target_os = "linux", target_os = "windows"))]
            Config::DotnetCoverage(c) => &mut c.common,
            Config::DotnetCrashReport(c) => &mut c.common,
            Config::LibFuzzerDotnetFuzz(c) => &mut c.common,
            Config::LibFuzzerFuzz(c) => &mut c.common,
            Config::LibFuzzerMerge(c) => &mut c.common,
            Config::LibFuzzerReport(c) => &mut c.common,
            Config::LibFuzzerRegression(c) => &mut c.common,
            Config::GenericAnalysis(c) => &mut c.common,
            Config::GenericMerge(c) => &mut c.common,
            Config::GenericReport(c) => &mut c.common,
            Config::GenericSupervisor(c) => &mut c.common,
            Config::GenericGenerator(c) => &mut c.common,
            Config::GenericRegression(c) => &mut c.common,
        }
    }

    pub fn common(&self) -> &CommonConfig {
        match self {
            #[cfg(any(target_os = "linux", target_os = "windows"))]
            Config::Coverage(c) => &c.common,
            #[cfg(any(target_os = "linux", target_os = "windows"))]
            Config::DotnetCoverage(c) => &c.common,
            Config::DotnetCrashReport(c) => &c.common,
            Config::LibFuzzerDotnetFuzz(c) => &c.common,
            Config::LibFuzzerFuzz(c) => &c.common,
            Config::LibFuzzerMerge(c) => &c.common,
            Config::LibFuzzerReport(c) => &c.common,
            Config::LibFuzzerRegression(c) => &c.common,
            Config::GenericAnalysis(c) => &c.common,
            Config::GenericMerge(c) => &c.common,
            Config::GenericReport(c) => &c.common,
            Config::GenericSupervisor(c) => &c.common,
            Config::GenericGenerator(c) => &c.common,
            Config::GenericRegression(c) => &c.common,
        }
    }

    pub fn report_event(&self) {
        let event_type = match self {
            #[cfg(any(target_os = "linux", target_os = "windows"))]
            Config::Coverage(_) => "coverage",
            #[cfg(any(target_os = "linux", target_os = "windows"))]
            Config::DotnetCoverage(_) => "dotnet_coverage",
            Config::DotnetCrashReport(_) => "dotnet_crash_report",
            Config::LibFuzzerDotnetFuzz(_) => "libfuzzer_fuzz",
            Config::LibFuzzerFuzz(_) => "libfuzzer_fuzz",
            Config::LibFuzzerMerge(_) => "libfuzzer_merge",
            Config::LibFuzzerReport(_) => "libfuzzer_crash_report",
            Config::LibFuzzerRegression(_) => "libfuzzer_regression",
            Config::GenericAnalysis(_) => "generic_analysis",
            Config::GenericMerge(_) => "generic_merge",
            Config::GenericReport(_) => "generic_crash_report",
            Config::GenericSupervisor(_) => "generic_supervisor",
            Config::GenericGenerator(_) => "generic_generator",
            Config::GenericRegression(_) => "generic_regression",
        };

        match self {
            Config::GenericGenerator(c) => {
                event!(task_start; EventData::Type = event_type, EventData::ToolName = c.generator_exe.clone());
            }
            Config::GenericAnalysis(c) => {
                event!(task_start; EventData::Type = event_type, EventData::ToolName = c.analyzer_exe.clone());
            }
            _ => {
                event!(task_start; EventData::Type = event_type);
            }
        }
    }

    pub async fn run(self) -> Result<()> {
        telemetry::set_property(EventData::JobId(self.common().job_id));
        telemetry::set_property(EventData::TaskId(self.common().task_id));
        telemetry::set_property(EventData::MachineId(
            self.common().machine_identity.machine_id,
        ));
        telemetry::set_property(EventData::Version(env!("ONEFUZZ_VERSION").to_string()));
        telemetry::set_property(EventData::InstanceId(self.common().instance_id));
        telemetry::set_property(EventData::Role(Role::Agent));

        if let Some(scaleset_name) = &self.common().machine_identity.scaleset_name {
            telemetry::set_property(EventData::ScalesetId(scaleset_name.to_string()));
        }

        if let Some(from_agent_to_task_endpoint) = &self.common().from_agent_to_task_endpoint {
            info!("Creating channel from agent to task");
            let (agent_sender, receive_from_agent): (IpcSender<String>, IpcReceiver<String>) =
                ipc::channel().unwrap();
            info!("Conecting...");
            let oneshot_sender = IpcSender::connect(from_agent_to_task_endpoint.clone()).unwrap();
            info!("Sending sender to agent");
            oneshot_sender.send(agent_sender).unwrap();
        }

        if let Some(from_task_to_agent_endpoint) = &self.common().from_task_to_agent_endpoint {
            info!("Creating channel from task to agent");
            let (task_sender, receive_from_task): (IpcSender<String>, IpcReceiver<String>) =
                ipc::channel().unwrap();
            info!("Connecting...");
            let oneshot_receiver = IpcSender::connect(from_task_to_agent_endpoint.clone()).unwrap();
            info!("Sending receiver to agent");
            oneshot_receiver.send(receive_from_task).unwrap();

            task_sender.send("hiiiii".to_string());
        }

        info!("agent ready, dispatching task");
        self.report_event();

        match self {
            #[cfg(any(target_os = "linux", target_os = "windows"))]
            Config::Coverage(config) => coverage::generic::CoverageTask::new(config).run().await,
            #[cfg(any(target_os = "linux", target_os = "windows"))]
            Config::DotnetCoverage(config) => {
                coverage::dotnet::DotnetCoverageTask::new(config)
                    .run()
                    .await
            }
            Config::DotnetCrashReport(config) => {
                report::dotnet::generic::DotnetCrashReportTask::new(config)
                    .run()
                    .await
            }
            Config::LibFuzzerDotnetFuzz(config) => {
                fuzz::libfuzzer::dotnet::LibFuzzerDotnetFuzzTask::new(config)?
                    .run()
                    .await
            }
            Config::LibFuzzerFuzz(config) => {
                fuzz::libfuzzer::generic::LibFuzzerFuzzTask::new(config)?
                    .run()
                    .await
            }
            Config::LibFuzzerReport(config) => {
                report::libfuzzer_report::ReportTask::new(config)
                    .managed_run()
                    .await
            }
            Config::LibFuzzerMerge(config) => merge::libfuzzer_merge::spawn(Arc::new(config)).await,
            Config::GenericAnalysis(config) => analysis::generic::run(config).await,

            Config::GenericGenerator(config) => {
                fuzz::generator::GeneratorTask::new(config).run().await
            }
            Config::GenericSupervisor(config) => fuzz::supervisor::spawn(config).await,
            Config::GenericMerge(config) => merge::generic::spawn(Arc::new(config)).await,
            Config::GenericReport(config) => {
                report::generic::ReportTask::new(config).managed_run().await
            }
            Config::GenericRegression(config) => {
                regression::generic::GenericRegressionTask::new(config)
                    .run()
                    .await
            }
            Config::LibFuzzerRegression(config) => {
                regression::libfuzzer::LibFuzzerRegressionTask::new(config)
                    .run()
                    .await
            }
        }
    }
}
