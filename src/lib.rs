// Copyright (c) 2026 Elias Bachaalany
// SPDX-License-Identifier: MIT

#![forbid(unsafe_code)]

//! # Copilot SDK for Rust
//!
//! A Rust SDK for interacting with the GitHub Copilot CLI.
//!
//! ## Quick Start
//!
//! ```no_run
//! use copilot_sdk::{Client, SessionConfig, SessionEventData};
//!
//! #[tokio::main]
//! async fn main() -> copilot_sdk::Result<()> {
//!     let client = Client::builder().build()?;
//!     client.start().await?;
//!
//!     let session = client.create_session(SessionConfig::default()).await?;
//!     let mut events = session.subscribe();
//!
//!     session.send("What is the capital of France?").await?;
//!
//!     while let Ok(event) = events.recv().await {
//!         match &event.data {
//!             SessionEventData::AssistantMessage(msg) => println!("{}", msg.content),
//!             SessionEventData::SessionIdle(_) => break,
//!             _ => {}
//!         }
//!     }
//!
//!     client.stop().await;
//!     Ok(())
//! }
//! ```

pub mod client;
pub mod debug_log;
pub mod error;
pub mod events;
pub mod jsonrpc;
pub mod process;
pub mod session;
pub mod tools;
pub mod transport;
pub mod types;

// Re-export tool utilities
pub use tools::define_tool;

// Re-export main types at crate root for convenience
pub use error::{CopilotError, Result};
pub use types::{
    // Session lifecycle event type constants
    session_lifecycle_event_types,
    // Config types
    AgentInfo,
    // Enums
    AttachmentType,
    AzureOptions,
    ClientOptions,
    // Command types
    CommandContext,
    CommandDefinition,
    CommandHandler,
    ConnectionState,
    CustomAgentConfig,
    // Hook types
    ErrorOccurredHandler,
    ErrorOccurredHookInput,
    ErrorOccurredHookOutput,
    FleetStartOptions,
    // Response types
    GetAuthStatusResponse,
    GetForegroundSessionResponse,
    GetStatusResponse,
    InfiniteSessionConfig,
    LogLevel,
    LogOptions,
    LogResult,
    McpLocalServerConfig,
    McpRemoteServerConfig,
    McpServerConfig,
    MessageOptions,
    ModelBilling,
    ModelCapabilities,
    ModelInfo,
    ModelLimits,
    ModelPolicy,
    ModelSupports,
    ModelVisionLimits,
    // Permission types
    PermissionRequest,
    PermissionRequestResult,
    PingResponse,
    PlanData,
    PostToolUseHandler,
    PostToolUseHookInput,
    PostToolUseHookOutput,
    PreToolUseHandler,
    PreToolUseHookInput,
    PreToolUseHookOutput,
    ProviderConfig,
    // Quota types
    QuotaResult,
    QuotaSnapshot,
    ResumeSessionConfig,
    // Section override types
    SectionOverride,
    SectionOverrideAction,
    // Selection types
    SelectionAttachment,
    SelectionPosition,
    SelectionRange,
    SessionConfig,
    SessionEndHandler,
    SessionEndHookInput,
    SessionEndHookOutput,
    SessionHooks,
    // Session lifecycle types
    SessionLifecycleEvent,
    SessionLifecycleEventMetadata,
    SessionLogLevel,
    SessionMetadata,
    SessionMode,
    SessionStartHandler,
    SessionStartHookInput,
    SessionStartHookOutput,
    SetForegroundSessionResponse,
    SetModelOptions,
    // Shell types
    ShellExecOptions,
    ShellExecResult,
    ShellSignal,
    StopError,
    SystemMessageConfig,
    SystemMessageMode,
    // Telemetry types
    TelemetryConfig,
    // Tool types
    Tool,
    ToolBinaryResult,
    ToolInfo,
    ToolInvocation,
    ToolResult,
    ToolResultObject,
    ToolsListResult,
    // User input types
    UserInputInvocation,
    UserInputRequest,
    UserInputResponse,
    UserMessageAttachment,
    UserPromptSubmittedHandler,
    UserPromptSubmittedHookInput,
    UserPromptSubmittedHookOutput,
    // Wire command type
    WireCommand,
    // Workspace types
    WorkspaceFile,
    // Constants
    SDK_PROTOCOL_VERSION,
};

pub use types::section;
pub use types::OnEventHandler;
pub use types::{ElicitationRequest, ElicitationResponse};

// Re-export session types
pub use session::EventSubscription;

// Re-export event types
pub use events::{
    // Event data types
    AbortData,
    AssistantIntentData,
    AssistantMessageData,
    AssistantMessageDeltaData,
    AssistantReasoningData,
    AssistantReasoningDeltaData,
    AssistantStreamingDeltaData,
    AssistantTurnEndData,
    AssistantTurnStartData,
    AssistantUsageData,
    CommandCompletedData,
    CommandExecuteData,
    CommandQueuedData,
    CompactionTokensUsed,
    CustomAgentCompletedData,
    CustomAgentFailedData,
    CustomAgentSelectedData,
    CustomAgentStartedData,
    ElicitationCompletedData,
    ElicitationRequestedData,
    ExitPlanModeCompletedData,
    ExitPlanModeRequestedData,
    ExternalToolCompletedData,
    ExternalToolRequestedData,
    HandoffSourceType,
    HookEndData,
    HookError,
    HookStartData,
    PendingMessagesModifiedData,
    PermissionCompletedData,
    PermissionRequestedData,
    // Main event types
    RawSessionEvent,
    RepositoryInfo,
    SessionCompactionCompleteData,
    SessionCompactionStartData,
    SessionContextChangedData,
    SessionErrorData,
    SessionEvent,
    SessionEventData,
    SessionHandoffData,
    SessionIdleData,
    SessionInfoData,
    SessionModeChangedData,
    SessionModelChangeData,
    SessionPlanChangedData,
    SessionResumeData,
    SessionShutdownData,
    SessionSnapshotRewindData,
    SessionStartData,
    SessionTaskCompleteData,
    SessionTitleChangedData,
    SessionTruncationData,
    SessionUsageInfoData,
    SessionWarningData,
    SessionWorkspaceFileChangedData,
    ShutdownCodeChanges,
    ShutdownType,
    SkillInvokedData,
    SubagentDeselectedData,
    SystemMessageEventData,
    SystemMessageMetadata,
    SystemMessageRole,
    SystemNotificationData,
    ToolExecutionCompleteData,
    ToolExecutionError,
    ToolExecutionPartialResultData,
    ToolExecutionProgressData,
    ToolExecutionStartData,
    ToolRequestItem,
    ToolResultContent,
    ToolUserRequestedData,
    UserInputCompletedData,
    UserInputRequestedData,
    UserMessageAttachmentItem,
    UserMessageData,
};

// Re-export transport types
pub use transport::{MessageFramer, StdioTransport, Transport};

// Re-export JSON-RPC types
pub use jsonrpc::{
    JsonRpcClient, JsonRpcError, JsonRpcId, JsonRpcRequest, JsonRpcResponse, NotificationHandler,
    RequestHandler,
};

// Re-export process types
pub use process::{
    find_copilot_cli, find_executable, find_node, is_node_script, CopilotProcess, ProcessOptions,
};

// Re-export session types
pub use session::{
    EventHandler, InvokeFuture, PermissionHandler, RegisteredTool, Session, ToolHandler,
    UserInputHandler,
};

// Re-export client types
pub use client::{Client, ClientBuilder, LifecycleHandler};
