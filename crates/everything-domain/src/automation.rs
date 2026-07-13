use crate::{ConnectorActionRequest, ConnectorProvider, ExecutionMode};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
pub enum AutonomyLevel {
    Observe,
    Assist,
    ActWithApproval,
    ActWithinPolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AutomationSchedule {
    Once {
        at_epoch_millis: u128,
    },
    Interval {
        every_millis: u64,
        #[serde(default)]
        anchor_epoch_millis: Option<u128>,
    },
    DailyLocal {
        hour: u8,
        minute: u8,
    },
    WeeklyLocal {
        weekdays: Vec<u8>,
        hour: u8,
        minute: u8,
    },
    DailyFixedOffset {
        hour: u8,
        minute: u8,
        utc_offset_minutes: i16,
    },
    WeeklyFixedOffset {
        weekdays: Vec<u8>,
        hour: u8,
        minute: u8,
        utc_offset_minutes: i16,
    },
}

impl AutomationSchedule {
    pub fn validate(&self) -> Result<(), String> {
        match self {
            Self::Once { at_epoch_millis } => {
                if *at_epoch_millis == 0 {
                    return Err("once schedule must have a non-zero timestamp".to_owned());
                }
            }
            Self::Interval { every_millis, .. } => {
                if *every_millis < 60_000 {
                    return Err("automation intervals must be at least 60 seconds".to_owned());
                }
            }
            Self::DailyLocal { hour, minute } => validate_local_wall_clock(*hour, *minute)?,
            Self::WeeklyLocal {
                weekdays,
                hour,
                minute,
            } => {
                validate_local_wall_clock(*hour, *minute)?;
                validate_weekdays(weekdays)?;
            }
            Self::DailyFixedOffset {
                hour,
                minute,
                utc_offset_minutes,
            } => validate_wall_clock(*hour, *minute, *utc_offset_minutes)?,
            Self::WeeklyFixedOffset {
                weekdays,
                hour,
                minute,
                utc_offset_minutes,
            } => {
                validate_wall_clock(*hour, *minute, *utc_offset_minutes)?;
                validate_weekdays(weekdays)?;
            }
        }
        Ok(())
    }
}

fn validate_local_wall_clock(hour: u8, minute: u8) -> Result<(), String> {
    if hour > 23 || minute > 59 {
        return Err("wall-clock time is invalid".to_owned());
    }
    Ok(())
}

fn validate_weekdays(weekdays: &[u8]) -> Result<(), String> {
    if weekdays.is_empty() || weekdays.iter().any(|day| *day > 6) {
        return Err("weekdays must contain values from 0 (Sunday) to 6".to_owned());
    }
    Ok(())
}

fn validate_wall_clock(hour: u8, minute: u8, offset: i16) -> Result<(), String> {
    if hour > 23 || minute > 59 {
        return Err("wall-clock time is invalid".to_owned());
    }
    if !(-14 * 60..=14 * 60).contains(&offset) {
        return Err("UTC offset is outside the supported range".to_owned());
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BriefingSource {
    pub provider: ConnectorProvider,
    pub action_id: String,
    #[serde(default)]
    pub input: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AutomationAction {
    Doctor {},
    Plan {
        objective: String,
        mode: ExecutionMode,
    },
    Skill {
        skill_id: String,
        #[serde(default)]
        input: Value,
    },
    Connector {
        request: ConnectorActionRequest,
    },
    Briefing {
        sources: Vec<BriefingSource>,
        prompt: String,
        mode: ExecutionMode,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutomationBudget {
    #[serde(default = "default_max_runtime_millis")]
    pub max_runtime_millis: u64,
    #[serde(default = "default_max_model_calls")]
    pub max_model_calls: u32,
    #[serde(default = "default_max_tool_invocations")]
    pub max_tool_invocations: u32,
    #[serde(default = "default_max_external_writes")]
    pub max_external_writes: u32,
    #[serde(default = "default_daily_execution_limit")]
    pub daily_execution_limit: u32,
}

impl Default for AutomationBudget {
    fn default() -> Self {
        Self {
            max_runtime_millis: default_max_runtime_millis(),
            max_model_calls: default_max_model_calls(),
            max_tool_invocations: default_max_tool_invocations(),
            max_external_writes: default_max_external_writes(),
            daily_execution_limit: default_daily_execution_limit(),
        }
    }
}

fn default_max_runtime_millis() -> u64 {
    10 * 60 * 1000
}
fn default_max_model_calls() -> u32 {
    4
}
fn default_max_tool_invocations() -> u32 {
    16
}
fn default_max_external_writes() -> u32 {
    1
}
fn default_daily_execution_limit() -> u32 {
    24
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
pub enum MissedRunPolicy {
    RunOnce,
    Skip,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutomationRetryPolicy {
    #[serde(default = "default_max_attempts")]
    pub max_attempts: u32,
    #[serde(default = "default_initial_backoff_millis")]
    pub initial_backoff_millis: u64,
    #[serde(default = "default_max_backoff_millis")]
    pub max_backoff_millis: u64,
    #[serde(default = "default_backoff_multiplier")]
    pub backoff_multiplier: u32,
}

impl Default for AutomationRetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: default_max_attempts(),
            initial_backoff_millis: default_initial_backoff_millis(),
            max_backoff_millis: default_max_backoff_millis(),
            backoff_multiplier: default_backoff_multiplier(),
        }
    }
}

fn default_max_attempts() -> u32 {
    3
}
fn default_initial_backoff_millis() -> u64 {
    30_000
}
fn default_max_backoff_millis() -> u64 {
    15 * 60 * 1_000
}
fn default_backoff_multiplier() -> u32 {
    2
}
fn default_missed_run_grace_millis() -> u64 {
    15 * 60 * 1_000
}
fn default_missed_run_policy() -> MissedRunPolicy {
    MissedRunPolicy::RunOnce
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutomationPolicy {
    #[serde(default = "default_autonomy_level")]
    pub autonomy_level: AutonomyLevel,
    #[serde(default)]
    pub allow_workspace_mutation: bool,
    #[serde(default)]
    pub allow_external_read: bool,
    #[serde(default)]
    pub allow_external_write: bool,
    #[serde(default)]
    pub approved_connector_actions: Vec<String>,
    #[serde(default)]
    pub budget: AutomationBudget,
    #[serde(default = "default_failure_threshold")]
    pub consecutive_failure_threshold: u32,
    #[serde(default)]
    pub retry: AutomationRetryPolicy,
    #[serde(default = "default_missed_run_grace_millis")]
    pub missed_run_grace_millis: u64,
    #[serde(default = "default_missed_run_policy")]
    pub missed_run_policy: MissedRunPolicy,
}

impl Default for AutomationPolicy {
    fn default() -> Self {
        Self {
            autonomy_level: default_autonomy_level(),
            allow_workspace_mutation: false,
            allow_external_read: true,
            allow_external_write: false,
            approved_connector_actions: Vec::new(),
            budget: AutomationBudget::default(),
            consecutive_failure_threshold: default_failure_threshold(),
            retry: AutomationRetryPolicy::default(),
            missed_run_grace_millis: default_missed_run_grace_millis(),
            missed_run_policy: default_missed_run_policy(),
        }
    }
}

fn default_autonomy_level() -> AutonomyLevel {
    AutonomyLevel::Assist
}
fn default_failure_threshold() -> u32 {
    3
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutomationUpsertRequest {
    #[serde(default)]
    pub automation_id: Option<String>,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub schedule: AutomationSchedule,
    pub action: AutomationAction,
    #[serde(default)]
    pub policy: AutomationPolicy,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

fn default_enabled() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutomationDefinition {
    pub automation_id: String,
    pub name: String,
    pub description: String,
    pub schedule: AutomationSchedule,
    pub action: AutomationAction,
    pub policy: AutomationPolicy,
    pub enabled: bool,
    pub created_at_epoch_millis: u128,
    pub updated_at_epoch_millis: u128,
    pub next_run_at_epoch_millis: Option<u128>,
    pub last_run_at_epoch_millis: Option<u128>,
    pub consecutive_failures: u32,
    #[serde(default)]
    pub retry_attempt: u32,
    #[serde(default)]
    pub retry_scheduled_for_epoch_millis: Option<u128>,
    #[serde(default)]
    pub suspended_reason: Option<String>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
pub enum AutomationExecutionStatus {
    Claimed,
    Running,
    AwaitingApproval,
    Approved,
    Completed,
    Failed,
    DeadLetter,
    Skipped,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutomationExecution {
    pub execution_id: String,
    pub automation_id: String,
    pub status: AutomationExecutionStatus,
    pub scheduled_for_epoch_millis: u128,
    pub started_at_epoch_millis: u128,
    #[serde(default)]
    pub finished_at_epoch_millis: Option<u128>,
    #[serde(default)]
    pub run_id: Option<String>,
    #[serde(default)]
    pub output: Value,
    #[serde(default)]
    pub error: Option<String>,
    pub attempt: u32,
    #[serde(default)]
    pub next_retry_at_epoch_millis: Option<u128>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutomationRunNowRequest {
    #[serde(default)]
    pub approval_granted: bool,
}
