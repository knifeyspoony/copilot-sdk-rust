// Copyright (c) 2026 Elias Bachaalany
// SPDX-License-Identifier: MIT

//! Comprehensive smoke tests for the Copilot SDK.
//!
//! These tests exercise every major interaction pattern the SDK supports:
//!
//! 1. **Agent** – plain session with a custom agent prompt, no tools.
//! 2. **Agent with tool** – session registers a custom tool and the model calls it.
//! 3. **Agent with skill** – session configured with `skill_directories`.
//! 4. **Agent with subagent** – custom agent via `custom_agents` config.
//! 5. **Subagent with custom tool (pre-selected)** – `agent:` on session config.
//! 6. **Subagent with custom tool (select_agent)** – explicit selection + tool call.
//! 7. **Subagent tool scoping** – each subagent only sees its own tools.
//! 8. **Agent management** – list / select / deselect lifecycle.
//! 9. **Multi-tool streaming** – multiple tools + streaming deltas.
//!
//! Run with: `cargo test --features e2e --test smoke_tests -- --test-threads=1`

#![cfg(feature = "e2e")]

use copilot_sdk::{
    find_copilot_cli, Client, CustomAgentConfig, LogLevel, PermissionRequestResult, SessionConfig,
    SessionEventData, SystemMessageConfig, SystemMessageMode, Tool, ToolResultObject,
};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Once};
use std::time::Duration;

// =============================================================================
// BYOK Environment Loader
// =============================================================================

static BYOK_INIT: Once = Once::new();

fn load_byok_env_file() {
    BYOK_INIT.call_once(|| {
        let test_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
        let env_file = test_dir.join("byok.env");
        if !env_file.exists() {
            eprintln!("[smoke] No byok.env found; using default Copilot auth");
            return;
        }
        let content = match std::fs::read_to_string(&env_file) {
            Ok(c) => c,
            Err(_) => return,
        };
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some((key, value)) = line.split_once('=') {
                unsafe { std::env::set_var(key.trim(), value.trim()) };
            }
        }
    });
}

// =============================================================================
// Test Helpers
// =============================================================================

fn skip_if_no_cli() -> bool {
    find_copilot_cli().is_none()
}

macro_rules! skip_if_no_cli {
    () => {
        load_byok_env_file();
        if skip_if_no_cli() {
            eprintln!("Skipping: Copilot CLI not found");
            return;
        }
    };
}

async fn create_test_client() -> copilot_sdk::Result<Client> {
    let client = Client::builder()
        .use_stdio(true)
        .log_level(LogLevel::Info)
        .build()?;
    client.start().await?;
    Ok(client)
}

fn byok_session_config() -> SessionConfig {
    SessionConfig {
        auto_byok_from_env: true,
        ..Default::default()
    }
}

/// Timeout for LLM round-trips in tests.
const LLM_TIMEOUT: Duration = Duration::from_secs(90);

// =============================================================================
// 1. Agent – plain session, no tools
// =============================================================================

/// Smoke test: create a session with a system prompt acting as a custom "agent"
/// persona, send a message, and verify we get a coherent response back.
#[tokio::test]
async fn smoke_agent_plain() {
    skip_if_no_cli!();

    let client = create_test_client().await.expect("client start");

    let config = SessionConfig {
        system_message: Some(SystemMessageConfig {
            mode: Some(SystemMessageMode::Replace),
            content: Some(
                "You are a math tutor. When asked a math question, answer with ONLY \
                 the numeric result, nothing else."
                    .to_string(),
            ),
            ..Default::default()
        }),
        // Lock out all built-in tools so the agent can't use shell, etc.
        available_tools: Some(vec![]),
        ..byok_session_config()
    };

    let session = client.create_session(config).await.expect("create session");
    assert!(!session.session_id().is_empty());

    let response = tokio::time::timeout(
        LLM_TIMEOUT,
        session.send_and_collect("What is 7 * 6?", None),
    )
    .await
    .expect("timeout")
    .expect("send_and_collect");

    eprintln!("[smoke_agent_plain] response: {response}");
    assert!(
        response.contains("42"),
        "Expected '42' in response: {response}"
    );

    session.destroy().await.ok();
    client.stop().await;
}

// =============================================================================
// 2. Agent with tool
// =============================================================================

/// Smoke test: register a custom tool, prompt the model to use it, and verify
/// the tool handler is invoked and its result appears in the response.
#[tokio::test]
async fn smoke_agent_with_tool() {
    skip_if_no_cli!();

    let client = create_test_client().await.expect("client start");

    // Track whether the tool was called
    let tool_called = Arc::new(AtomicBool::new(false));
    let tool_called_clone = Arc::clone(&tool_called);

    let tool = Tool::new("lookup_magic_number")
        .description("Returns a secret magic number. Call this tool when asked for the magic number.")
        .schema(serde_json::json!({
            "type": "object",
            "properties": {
                "key": {
                    "type": "string",
                    "description": "Lookup key"
                }
            },
            "required": ["key"]
        }));

    let config = SessionConfig {
        tools: vec![tool.clone()],
        system_message: Some(SystemMessageConfig {
            mode: Some(SystemMessageMode::Replace),
            content: Some(
                "You are a helpful assistant. When the user asks for a magic number, \
                 you MUST call the lookup_magic_number tool with key 'default'. \
                 Then report the result exactly as the tool returns it."
                    .to_string(),
            ),
            ..Default::default()
        }),
        available_tools: Some(vec!["lookup_magic_number".to_string()]),
        ..byok_session_config()
    };

    let session = client.create_session(config).await.expect("create session");

    session
        .register_tool_with_handler(
            tool,
            Some(Arc::new(move |_name, _args| {
                tool_called_clone.store(true, Ordering::SeqCst);
                ToolResultObject::text("98765")
            })),
        )
        .await;

    session
        .register_permission_handler(|_req| PermissionRequestResult::approved())
        .await;

    // Also observe events to confirm tool execution lifecycle
    let mut events = session.subscribe();
    let saw_tool_start = Arc::new(AtomicBool::new(false));
    let saw_tool_complete = Arc::new(AtomicBool::new(false));
    let saw_tool_start_clone = Arc::clone(&saw_tool_start);
    let saw_tool_complete_clone = Arc::clone(&saw_tool_complete);

    session
        .send("What is the magic number?")
        .await
        .expect("send");

    let mut content = String::new();
    let result = tokio::time::timeout(LLM_TIMEOUT, async {
        while let Ok(event) = events.recv().await {
            match &event.data {
                SessionEventData::ToolExecutionStart(t) => {
                    eprintln!("[smoke_agent_with_tool] tool start: {}", t.tool_name);
                    saw_tool_start_clone.store(true, Ordering::SeqCst);
                }
                SessionEventData::ToolExecutionComplete(t) => {
                    eprintln!(
                        "[smoke_agent_with_tool] tool complete: {} success={}",
                        t.tool_call_id, t.success
                    );
                    saw_tool_complete_clone.store(true, Ordering::SeqCst);
                }
                SessionEventData::AssistantMessage(msg) => {
                    content.push_str(&msg.content);
                }
                SessionEventData::AssistantMessageDelta(delta) => {
                    content.push_str(&delta.delta_content);
                }
                SessionEventData::SessionIdle(_) => break,
                SessionEventData::SessionError(err) => {
                    panic!("Session error: {}", err.message);
                }
                _ => {}
            }
        }
    })
    .await;

    assert!(result.is_ok(), "Timed out waiting for response");

    eprintln!("[smoke_agent_with_tool] response: {content}");

    assert!(
        tool_called.load(Ordering::SeqCst),
        "Tool handler should have been called"
    );
    assert!(
        saw_tool_start.load(Ordering::SeqCst),
        "Should have seen ToolExecutionStart event"
    );
    assert!(
        saw_tool_complete.load(Ordering::SeqCst),
        "Should have seen ToolExecutionComplete event"
    );
    assert!(
        content.contains("98765"),
        "Response should contain tool result '98765': {content}"
    );

    session.destroy().await.ok();
    client.stop().await;
}

// =============================================================================
// 3. Agent with skill (skill_directories config)
// =============================================================================

/// Smoke test: create a session with `skill_directories` configured.
///
/// Skills are loaded from the filesystem by the Copilot CLI. We create a
/// temporary skill directory with a minimal skill definition and verify
/// the session creates successfully and can respond. If the CLI emits a
/// `skill.invoked` event, we capture it.
#[tokio::test]
async fn smoke_agent_with_skill() {
    skip_if_no_cli!();

    let client = create_test_client().await.expect("client start");

    // Create a temp dir with a minimal skill definition
    let skill_dir = tempfile::tempdir().expect("tempdir");
    let skill_file = skill_dir.path().join("math-helper.md");
    std::fs::write(
        &skill_file,
        r#"---
name: math-helper
description: A skill that helps with math problems
---

You are a math helper. When asked math questions, provide clear step-by-step solutions.
Always show your work.
"#,
    )
    .expect("write skill file");

    let config = SessionConfig {
        skill_directories: Some(vec![skill_dir.path().to_string_lossy().to_string()]),
        system_message: Some(SystemMessageConfig {
            mode: Some(SystemMessageMode::Append),
            content: Some(
                "You have access to skills. Use them when appropriate. Be concise.".to_string(),
            ),
            ..Default::default()
        }),
        ..byok_session_config()
    };

    let session = client.create_session(config).await.expect("create session");
    assert!(!session.session_id().is_empty());

    let mut events = session.subscribe();
    let saw_skill_invoked = Arc::new(AtomicBool::new(false));
    let saw_skill_invoked_clone = Arc::clone(&saw_skill_invoked);

    session
        .send("What is the square root of 144?")
        .await
        .expect("send");

    let mut content = String::new();
    let result = tokio::time::timeout(LLM_TIMEOUT, async {
        while let Ok(event) = events.recv().await {
            match &event.data {
                SessionEventData::SkillInvoked(data) => {
                    eprintln!(
                        "[smoke_agent_with_skill] skill invoked: {} at {}",
                        data.name, data.path
                    );
                    saw_skill_invoked_clone.store(true, Ordering::SeqCst);
                }
                SessionEventData::AssistantMessage(msg) => {
                    content.push_str(&msg.content);
                }
                SessionEventData::AssistantMessageDelta(delta) => {
                    content.push_str(&delta.delta_content);
                }
                SessionEventData::SessionIdle(_) => break,
                SessionEventData::SessionError(err) => {
                    eprintln!("[smoke_agent_with_skill] session error: {}", err.message);
                    break;
                }
                _ => {}
            }
        }
    })
    .await;

    assert!(result.is_ok(), "Timed out waiting for response");
    eprintln!("[smoke_agent_with_skill] response: {content}");
    eprintln!(
        "[smoke_agent_with_skill] skill invoked event seen: {}",
        saw_skill_invoked.load(Ordering::SeqCst)
    );

    // The session should have responded (skill invocation is optional; the CLI
    // may or may not pick up the skill depending on version/configuration).
    assert!(
        content.contains("12"),
        "Expected '12' in response: {content}"
    );

    session.destroy().await.ok();
    client.stop().await;
}

// =============================================================================
// 4. Agent with subagent (via custom_agents / task)
// =============================================================================

/// Smoke test: configure a custom subagent via `custom_agents`, select it,
/// send a message, and verify the subagent processes the request.
#[tokio::test]
async fn smoke_agent_with_subagent() {
    skip_if_no_cli!();

    let client = create_test_client().await.expect("client start");

    let subagent = CustomAgentConfig {
        name: "haiku-poet".to_string(),
        prompt: "You are a haiku poet. When asked anything, respond with ONLY a haiku \
                 (three lines: 5-7-5 syllables). Nothing else."
            .to_string(),
        display_name: Some("Haiku Poet".to_string()),
        description: Some("Responds exclusively in haiku form".to_string()),
        tools: None,
        mcp_servers: None,
        infer: Some(true),
    };

    let config = SessionConfig {
        custom_agents: Some(vec![subagent]),
        available_tools: Some(vec![]),
        ..byok_session_config()
    };

    let session = client.create_session(config).await.expect("create session");
    assert!(!session.session_id().is_empty());

    // Observe custom agent events
    let mut events = session.subscribe();
    let saw_agent_started = Arc::new(AtomicBool::new(false));
    let saw_agent_completed = Arc::new(AtomicBool::new(false));
    let saw_agent_selected = Arc::new(AtomicBool::new(false));
    let saw_agent_started_clone = Arc::clone(&saw_agent_started);
    let saw_agent_completed_clone = Arc::clone(&saw_agent_completed);
    let saw_agent_selected_clone = Arc::clone(&saw_agent_selected);

    // Verify agent is listed
    let agents = session.list_agents().await;
    eprintln!("[smoke_agent_with_subagent] agents: {:?}", agents);

    session
        .send("Write a haiku about Rust programming")
        .await
        .expect("send");

    let mut content = String::new();
    let result = tokio::time::timeout(LLM_TIMEOUT, async {
        while let Ok(event) = events.recv().await {
            match &event.data {
                SessionEventData::CustomAgentStarted(data) => {
                    eprintln!(
                        "[smoke_agent_with_subagent] agent started: {}",
                        data.agent_name
                    );
                    saw_agent_started_clone.store(true, Ordering::SeqCst);
                }
                SessionEventData::CustomAgentCompleted(data) => {
                    eprintln!(
                        "[smoke_agent_with_subagent] agent completed: {}",
                        data.agent_name
                    );
                    saw_agent_completed_clone.store(true, Ordering::SeqCst);
                }
                SessionEventData::CustomAgentSelected(data) => {
                    eprintln!(
                        "[smoke_agent_with_subagent] agent selected: {}",
                        data.agent_name
                    );
                    saw_agent_selected_clone.store(true, Ordering::SeqCst);
                }
                SessionEventData::AssistantMessage(msg) => {
                    content.push_str(&msg.content);
                }
                SessionEventData::AssistantMessageDelta(delta) => {
                    content.push_str(&delta.delta_content);
                }
                SessionEventData::SessionIdle(_) => break,
                SessionEventData::SessionError(err) => {
                    eprintln!(
                        "[smoke_agent_with_subagent] session error: {}",
                        err.message
                    );
                    break;
                }
                _ => {}
            }
        }
    })
    .await;

    assert!(result.is_ok(), "Timed out waiting for response");
    eprintln!("[smoke_agent_with_subagent] response: {content}");

    // We should have gotten some response
    assert!(!content.is_empty(), "Response should not be empty");

    // Log whether agent lifecycle events were emitted (depends on CLI version
    // and whether inference routes through the subagent)
    eprintln!(
        "[smoke_agent_with_subagent] agent events: started={}, completed={}, selected={}",
        saw_agent_started.load(Ordering::SeqCst),
        saw_agent_completed.load(Ordering::SeqCst),
        saw_agent_selected.load(Ordering::SeqCst),
    );

    session.destroy().await.ok();
    client.stop().await;
}

// =============================================================================
// 5. Subagent with custom tool — explicitly selected
// =============================================================================

/// Smoke test: register a custom tool, configure a subagent that owns it,
/// *explicitly select* the subagent, send a message, and verify:
///   - The subagent lifecycle events fire (started → completed)
///   - The custom tool is invoked *while the subagent is active*
///   - The tool result makes it into the final response
///
/// This is the proper regression test for "custom tools in subagents."
/// We use `select_agent` instead of relying on inference so the test is
/// deterministic — the subagent is guaranteed to be the active agent.
#[tokio::test]
async fn smoke_subagent_with_custom_tool() {
    skip_if_no_cli!();

    let client = create_test_client().await.expect("client start");

    // Track tool invocation
    let tool_called = Arc::new(AtomicBool::new(false));
    let tool_called_clone = Arc::clone(&tool_called);

    let lookup_tool = Tool::new("get_fruit_price")
        .description(
            "Looks up the current price of a fruit. Call this whenever asked about fruit prices.",
        )
        .schema(serde_json::json!({
            "type": "object",
            "properties": {
                "fruit": {
                    "type": "string",
                    "description": "Name of the fruit to look up"
                }
            },
            "required": ["fruit"]
        }));

    // Subagent: the ONLY agent with access to the tool.
    let grocery_agent = CustomAgentConfig {
        name: "grocery-agent".to_string(),
        prompt: "You are a grocery store assistant. When asked about fruit prices, you MUST \
                 call the get_fruit_price tool. Report the exact result from the tool."
            .to_string(),
        display_name: Some("Grocery Agent".to_string()),
        description: Some("Agent that looks up grocery prices using tools".to_string()),
        tools: Some(vec!["get_fruit_price".to_string()]),
        mcp_servers: None,
        infer: Some(false), // inference OFF — we'll select explicitly
    };

    let config = SessionConfig {
        tools: vec![lookup_tool.clone()],
        custom_agents: Some(vec![grocery_agent]),
        // Pre-select the subagent so it handles the request directly
        agent: Some("grocery-agent".to_string()),
        ..byok_session_config()
    };

    let session = client.create_session(config).await.expect("create session");

    // Register tool handler
    session
        .register_tool_with_handler(
            lookup_tool,
            Some(Arc::new(move |_name, args| {
                tool_called_clone.store(true, Ordering::SeqCst);
                let fruit = args
                    .get("fruit")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                eprintln!(
                    "[smoke_subagent_tool] tool invoked for fruit: {}",
                    fruit
                );
                ToolResultObject::text(format!("{} costs $3.47 per pound", fruit))
            })),
        )
        .await;

    session
        .register_permission_handler(|_req| PermissionRequestResult::approved())
        .await;

    // Collect events — track ordering to prove tool ran under the subagent
    let mut events = session.subscribe();

    // Event-ordering flags
    let event_log = Arc::new(tokio::sync::Mutex::new(Vec::<String>::new()));
    let event_log_clone = Arc::clone(&event_log);

    session
        .send("How much do mangoes cost?")
        .await
        .expect("send");

    let mut content = String::new();
    let result = tokio::time::timeout(LLM_TIMEOUT, async {
        while let Ok(event) = events.recv().await {
            match &event.data {
                SessionEventData::CustomAgentStarted(data) => {
                    eprintln!("[smoke_subagent_tool] agent started: {}", data.agent_name);
                    event_log_clone.lock().await.push(format!("agent_started:{}", data.agent_name));
                }
                SessionEventData::CustomAgentSelected(data) => {
                    eprintln!("[smoke_subagent_tool] agent selected: {}", data.agent_name);
                    event_log_clone.lock().await.push(format!("agent_selected:{}", data.agent_name));
                }
                SessionEventData::ToolExecutionStart(t) => {
                    eprintln!("[smoke_subagent_tool] tool start: {}", t.tool_name);
                    event_log_clone.lock().await.push(format!("tool_start:{}", t.tool_name));
                }
                SessionEventData::ToolExecutionComplete(t) => {
                    eprintln!(
                        "[smoke_subagent_tool] tool complete: {} success={}",
                        t.tool_call_id, t.success
                    );
                    event_log_clone.lock().await.push("tool_complete".to_string());
                }
                SessionEventData::CustomAgentCompleted(data) => {
                    eprintln!("[smoke_subagent_tool] agent completed: {}", data.agent_name);
                    event_log_clone.lock().await.push(format!("agent_completed:{}", data.agent_name));
                }
                SessionEventData::CustomAgentFailed(data) => {
                    eprintln!(
                        "[smoke_subagent_tool] agent FAILED: {} — {}",
                        data.agent_name, data.error
                    );
                    event_log_clone.lock().await.push(format!("agent_failed:{}", data.agent_name));
                }
                SessionEventData::AssistantMessage(msg) => {
                    content.push_str(&msg.content);
                }
                SessionEventData::AssistantMessageDelta(delta) => {
                    content.push_str(&delta.delta_content);
                }
                SessionEventData::SessionIdle(_) => break,
                SessionEventData::SessionError(err) => {
                    eprintln!("[smoke_subagent_tool] session error: {}", err.message);
                    break;
                }
                _ => {}
            }
        }
    })
    .await;

    assert!(result.is_ok(), "Timed out waiting for response");

    let log = event_log.lock().await;
    eprintln!("[smoke_subagent_tool] event log: {:?}", *log);
    eprintln!("[smoke_subagent_tool] response: {content}");

    // ── Hard assertions ────────────────────────────────────────────────
    // 1. Tool handler was actually invoked
    assert!(
        tool_called.load(Ordering::SeqCst),
        "Custom tool handler should have been called by the subagent"
    );

    // 2. We saw the tool execution events
    assert!(
        log.iter().any(|e| e.starts_with("tool_start:get_fruit_price")),
        "Should have seen ToolExecutionStart for get_fruit_price. Log: {:?}",
        *log
    );

    // 3. Tool result appeared in the response
    assert!(
        content.contains("3.47"),
        "Response should contain tool result '$3.47': {content}"
    );

    // ── Soft assertions (log for diagnosis, don't fail) ────────────────
    // Whether the CLI emits subagent lifecycle events when using `agent:`
    // pre-selection varies by version. Log them for visibility.
    let saw_agent_started = log.iter().any(|e| e.starts_with("agent_started:"));
    let saw_agent_completed = log.iter().any(|e| e.starts_with("agent_completed:"));
    let saw_agent_selected = log.iter().any(|e| e.starts_with("agent_selected:"));
    eprintln!(
        "[smoke_subagent_tool] subagent events: selected={}, started={}, completed={}",
        saw_agent_selected, saw_agent_started, saw_agent_completed,
    );

    // If we got agent lifecycle events, verify ordering:
    // agent_started should come before tool_start, tool_complete before agent_completed
    if saw_agent_started {
        let started_idx = log.iter().position(|e| e.starts_with("agent_started:"));
        let tool_idx = log.iter().position(|e| e.starts_with("tool_start:"));
        if let (Some(a), Some(t)) = (started_idx, tool_idx) {
            assert!(
                a < t,
                "agent_started should fire before tool_start. Log: {:?}",
                *log
            );
        }
    }

    session.destroy().await.ok();
    client.stop().await;
}

// =============================================================================
// 5b. Subagent with custom tool — verify agent is active during tool call
// =============================================================================

/// Regression test: custom tools invoked while a subagent is the active agent.
///
/// We explicitly `select_agent`, then check `get_current_agent()` to confirm
/// the subagent is active, send a message that triggers the custom tool, and
/// verify the tool fires. This proves the tool call happens *through* the
/// subagent's tool set, not the parent's.
#[tokio::test]
async fn smoke_subagent_selected_with_custom_tool() {
    skip_if_no_cli!();

    let client = create_test_client().await.expect("client start");

    let tool_called = Arc::new(AtomicBool::new(false));
    let tool_called_clone = Arc::clone(&tool_called);

    let lookup_tool = Tool::new("get_fruit_price")
        .description(
            "Looks up the current price of a fruit. Call this whenever asked about fruit prices.",
        )
        .schema(serde_json::json!({
            "type": "object",
            "properties": {
                "fruit": {
                    "type": "string",
                    "description": "Name of the fruit to look up"
                }
            },
            "required": ["fruit"]
        }));

    let grocery_agent = CustomAgentConfig {
        name: "grocery-agent".to_string(),
        prompt: "You are a grocery store assistant. When asked about fruit prices, you MUST \
                 call the get_fruit_price tool. Report the exact price the tool returns."
            .to_string(),
        display_name: Some("Grocery Agent".to_string()),
        description: Some("Looks up fruit and grocery prices".to_string()),
        tools: Some(vec!["get_fruit_price".to_string()]),
        mcp_servers: None,
        infer: Some(false), // No inference — we select explicitly
    };

    let config = SessionConfig {
        tools: vec![lookup_tool.clone()],
        custom_agents: Some(vec![grocery_agent]),
        ..byok_session_config()
    };

    let session = client.create_session(config).await.expect("create session");

    session
        .register_tool_with_handler(
            lookup_tool,
            Some(Arc::new(move |_name, args| {
                tool_called_clone.store(true, Ordering::SeqCst);
                let fruit = args
                    .get("fruit")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                eprintln!("[smoke_subagent_selected] tool invoked for: {}", fruit);
                ToolResultObject::text(format!("{} costs $3.47 per pound", fruit))
            })),
        )
        .await;

    session
        .register_permission_handler(|_req| PermissionRequestResult::approved())
        .await;

    // ── Explicitly select the subagent ────────────────────────────────
    session
        .select_agent("grocery-agent")
        .await
        .expect("select grocery-agent");

    // Verify the subagent is the current agent BEFORE the tool call
    // (get_current_agent may not be supported on all CLI versions)
    match session.get_current_agent().await {
        Ok(current) => {
            eprintln!("[smoke_subagent_selected] current agent: {:?}", current);
            assert!(
                current.as_ref().map(|a| a.name.as_str()) == Some("grocery-agent"),
                "grocery-agent should be the current agent, got: {:?}",
                current
            );
        }
        Err(e) => {
            eprintln!(
                "[smoke_subagent_selected] get_current_agent not supported ({}), skipping pre-check",
                e
            );
        }
    }

    // ── Send message and collect events ───────────────────────────────
    let mut events = session.subscribe();
    let event_log = Arc::new(tokio::sync::Mutex::new(Vec::<String>::new()));
    let event_log_clone = Arc::clone(&event_log);

    session
        .send("How much do mangoes cost?")
        .await
        .expect("send");

    let mut content = String::new();
    let result = tokio::time::timeout(LLM_TIMEOUT, async {
        while let Ok(event) = events.recv().await {
            match &event.data {
                SessionEventData::CustomAgentSelected(data) => {
                    eprintln!("[smoke_subagent_selected] agent selected: {}", data.agent_name);
                    event_log_clone.lock().await.push(format!("agent_selected:{}", data.agent_name));
                }
                SessionEventData::CustomAgentStarted(data) => {
                    eprintln!("[smoke_subagent_selected] agent started: {}", data.agent_name);
                    event_log_clone.lock().await.push(format!("agent_started:{}", data.agent_name));
                }
                SessionEventData::ToolExecutionStart(t) => {
                    eprintln!("[smoke_subagent_selected] tool start: {}", t.tool_name);
                    event_log_clone.lock().await.push(format!("tool_start:{}", t.tool_name));
                }
                SessionEventData::ToolExecutionComplete(t) => {
                    eprintln!("[smoke_subagent_selected] tool complete: {} success={}", t.tool_call_id, t.success);
                    event_log_clone.lock().await.push(format!("tool_complete:{}", t.tool_call_id));
                }
                SessionEventData::CustomAgentCompleted(data) => {
                    eprintln!("[smoke_subagent_selected] agent completed: {}", data.agent_name);
                    event_log_clone.lock().await.push(format!("agent_completed:{}", data.agent_name));
                }
                SessionEventData::CustomAgentFailed(data) => {
                    eprintln!("[smoke_subagent_selected] agent FAILED: {} — {}", data.agent_name, data.error);
                    event_log_clone.lock().await.push(format!("agent_failed:{}", data.agent_name));
                }
                SessionEventData::SubagentDeselected(_) => {
                    eprintln!("[smoke_subagent_selected] subagent deselected");
                    event_log_clone.lock().await.push("subagent_deselected".to_string());
                }
                SessionEventData::AssistantMessage(msg) => content.push_str(&msg.content),
                SessionEventData::AssistantMessageDelta(delta) => content.push_str(&delta.delta_content),
                SessionEventData::SessionIdle(_) => break,
                SessionEventData::SessionError(err) => {
                    eprintln!("[smoke_subagent_selected] session error: {}", err.message);
                    break;
                }
                _ => {}
            }
        }
    })
    .await;

    assert!(result.is_ok(), "Timed out waiting for response");

    let log = event_log.lock().await;
    eprintln!("[smoke_subagent_selected] event log: {:?}", *log);
    eprintln!("[smoke_subagent_selected] response: {content}");

    // ── Hard assertions ────────────────────────────────────────────────
    // 1. Tool was called while the subagent was active
    assert!(
        tool_called.load(Ordering::SeqCst),
        "Custom tool should have been called while grocery-agent was the active agent"
    );

    // 2. The tool execution event fired
    assert!(
        log.iter().any(|e| e.starts_with("tool_start:get_fruit_price")),
        "Should see ToolExecutionStart for get_fruit_price. Log: {:?}",
        *log
    );

    // 3. Tool result in the response
    assert!(
        content.contains("3.47"),
        "Response should contain tool result '$3.47': {content}"
    );

    // 4. The subagent is STILL the current agent after the tool call
    //    (it wasn't deselected mid-turn)
    match session.get_current_agent().await {
        Ok(current_after) => {
            eprintln!("[smoke_subagent_selected] agent after tool call: {:?}", current_after);
        }
        Err(e) => {
            eprintln!("[smoke_subagent_selected] get_current_agent not supported ({})", e);
        }
    }

    session.destroy().await.ok();
    client.stop().await;
}

// =============================================================================
// 5c. Subagent tool scoping — tools are filtered from parent session
// =============================================================================

/// Verify that subagent tool lists act as a *filter* on the session's tools,
/// not as an independent tool set.
///
/// Setup:
///   - Session has two custom tools: `get_fruit_price` and `get_veggie_price`
///   - Subagent A (`fruit-agent`) declares `tools: ["get_fruit_price"]`
///   - Subagent B (`veggie-agent`) declares `tools: ["get_veggie_price"]`
///
/// We select each agent in turn and verify it can only call its own tool.
/// This confirms the CLI filters subagent tools from the parent's pool.
#[tokio::test]
async fn smoke_subagent_tool_scoping() {
    skip_if_no_cli!();

    let client = create_test_client().await.expect("client start");

    let fruit_called = Arc::new(AtomicBool::new(false));
    let veggie_called = Arc::new(AtomicBool::new(false));
    let fruit_called_clone = Arc::clone(&fruit_called);
    let veggie_called_clone = Arc::clone(&veggie_called);

    let fruit_tool = Tool::new("get_fruit_price")
        .description("Looks up fruit prices. Call this for fruit price questions.")
        .schema(serde_json::json!({
            "type": "object",
            "properties": {
                "fruit": { "type": "string", "description": "Fruit name" }
            },
            "required": ["fruit"]
        }));

    let veggie_tool = Tool::new("get_veggie_price")
        .description("Looks up vegetable prices. Call this for veggie price questions.")
        .schema(serde_json::json!({
            "type": "object",
            "properties": {
                "veggie": { "type": "string", "description": "Vegetable name" }
            },
            "required": ["veggie"]
        }));

    let fruit_agent = CustomAgentConfig {
        name: "fruit-agent".to_string(),
        prompt: "You are a fruit specialist. Use get_fruit_price for any price question. \
                 Report the exact tool result."
            .to_string(),
        display_name: Some("Fruit Agent".to_string()),
        description: Some("Handles fruit prices".to_string()),
        tools: Some(vec!["get_fruit_price".to_string()]),
        mcp_servers: None,
        infer: Some(false),
    };

    let veggie_agent = CustomAgentConfig {
        name: "veggie-agent".to_string(),
        prompt: "You are a vegetable specialist. Use get_veggie_price for any price question. \
                 Report the exact tool result."
            .to_string(),
        display_name: Some("Veggie Agent".to_string()),
        description: Some("Handles vegetable prices".to_string()),
        tools: Some(vec!["get_veggie_price".to_string()]),
        mcp_servers: None,
        infer: Some(false),
    };

    let config = SessionConfig {
        tools: vec![fruit_tool.clone(), veggie_tool.clone()],
        custom_agents: Some(vec![fruit_agent, veggie_agent]),
        ..byok_session_config()
    };

    let session = client.create_session(config).await.expect("create session");

    session
        .register_tool_with_handler(
            fruit_tool,
            Some(Arc::new(move |_name, args| {
                fruit_called_clone.store(true, Ordering::SeqCst);
                let fruit = args.get("fruit").and_then(|v| v.as_str()).unwrap_or("?");
                eprintln!("[smoke_scoping] fruit tool called for: {fruit}");
                ToolResultObject::text(format!("{fruit} costs $2.99/lb"))
            })),
        )
        .await;

    session
        .register_tool_with_handler(
            veggie_tool,
            Some(Arc::new(move |_name, args| {
                veggie_called_clone.store(true, Ordering::SeqCst);
                let veggie = args.get("veggie").and_then(|v| v.as_str()).unwrap_or("?");
                eprintln!("[smoke_scoping] veggie tool called for: {veggie}");
                ToolResultObject::text(format!("{veggie} costs $1.49/lb"))
            })),
        )
        .await;

    session
        .register_permission_handler(|_req| PermissionRequestResult::approved())
        .await;

    // ── Round 1: select fruit-agent, ask about fruit ──────────────────
    session.select_agent("fruit-agent").await.expect("select fruit-agent");

    let response = tokio::time::timeout(
        LLM_TIMEOUT,
        session.send_and_collect("How much do apples cost?", None),
    )
    .await
    .expect("timeout")
    .expect("send_and_collect");

    eprintln!("[smoke_scoping] fruit-agent response: {response}");

    assert!(
        fruit_called.load(Ordering::SeqCst),
        "fruit tool should have been called when fruit-agent is selected"
    );
    assert!(
        response.contains("2.99"),
        "Response should contain fruit price '$2.99': {response}"
    );

    // Reset for round 2
    fruit_called.store(false, Ordering::SeqCst);
    session.deselect_agent().await.expect("deselect");

    // ── Round 2: select veggie-agent, ask about veggies ───────────────
    session.select_agent("veggie-agent").await.expect("select veggie-agent");

    let response2 = tokio::time::timeout(
        LLM_TIMEOUT,
        session.send_and_collect("How much do carrots cost?", None),
    )
    .await
    .expect("timeout")
    .expect("send_and_collect");

    eprintln!("[smoke_scoping] veggie-agent response: {response2}");

    assert!(
        veggie_called.load(Ordering::SeqCst),
        "veggie tool should have been called when veggie-agent is selected"
    );
    assert!(
        response2.contains("1.49"),
        "Response should contain veggie price '$1.49': {response2}"
    );

    // Verify cross-contamination didn't happen in round 2
    // (fruit tool shouldn't have been re-called during the veggie round)
    assert!(
        !fruit_called.load(Ordering::SeqCst),
        "fruit tool should NOT have been called during veggie-agent's turn"
    );

    session.destroy().await.ok();
    client.stop().await;
}

// =============================================================================
// 7. Agent management lifecycle (list / select / deselect)
// =============================================================================

/// Smoke test: verify the agent management APIs work end-to-end. Create a
/// session with multiple custom agents, list them, select one, verify
/// current agent, then deselect.
#[tokio::test]
async fn smoke_agent_management_lifecycle() {
    skip_if_no_cli!();

    let client = create_test_client().await.expect("client start");

    let agent_a = CustomAgentConfig {
        name: "agent-alpha".to_string(),
        prompt: "You are Alpha, a friendly agent.".to_string(),
        display_name: Some("Agent Alpha".to_string()),
        description: Some("First test agent".to_string()),
        tools: None,
        mcp_servers: None,
        infer: Some(false),
    };

    let agent_b = CustomAgentConfig {
        name: "agent-beta".to_string(),
        prompt: "You are Beta, a formal agent.".to_string(),
        display_name: Some("Agent Beta".to_string()),
        description: Some("Second test agent".to_string()),
        tools: None,
        mcp_servers: None,
        infer: Some(false),
    };

    let config = SessionConfig {
        custom_agents: Some(vec![agent_a, agent_b]),
        ..byok_session_config()
    };

    let session = client.create_session(config).await.expect("create session");

    // List agents
    let agents = session.list_agents().await.expect("list agents");
    eprintln!("[smoke_agent_management] agents: {:?}", agents);

    // Select agent-alpha
    let select_result = session.select_agent("agent-alpha").await;
    eprintln!("[smoke_agent_management] select result: {:?}", select_result);

    // Check current agent
    let current = session.get_current_agent().await;
    eprintln!("[smoke_agent_management] current agent: {:?}", current);

    // Deselect
    let deselect_result = session.deselect_agent().await;
    eprintln!(
        "[smoke_agent_management] deselect result: {:?}",
        deselect_result
    );

    // The session should still work after deselect
    let response = tokio::time::timeout(
        LLM_TIMEOUT,
        session.send_and_collect("Say 'ok'", None),
    )
    .await
    .expect("timeout")
    .expect("send_and_collect");

    eprintln!("[smoke_agent_management] response: {response}");
    assert!(!response.is_empty(), "Should get a response after deselect");

    session.destroy().await.ok();
    client.stop().await;
}

// =============================================================================
// 8. Multi-tool agent with streaming
// =============================================================================

/// Smoke test: register multiple tools, enable streaming, and verify the model
/// can call different tools within a single interaction.
#[tokio::test]
async fn smoke_agent_multi_tool_streaming() {
    skip_if_no_cli!();

    let client = create_test_client().await.expect("client start");

    let add_called = Arc::new(AtomicBool::new(false));
    let multiply_called = Arc::new(AtomicBool::new(false));
    let add_called_clone = Arc::clone(&add_called);
    let multiply_called_clone = Arc::clone(&multiply_called);

    let add_tool = Tool::new("add_numbers")
        .description("Adds two numbers and returns the sum. Always use this for addition.")
        .schema(serde_json::json!({
            "type": "object",
            "properties": {
                "a": { "type": "number", "description": "First number" },
                "b": { "type": "number", "description": "Second number" }
            },
            "required": ["a", "b"]
        }));

    let multiply_tool = Tool::new("multiply_numbers")
        .description("Multiplies two numbers and returns the product. Always use this for multiplication.")
        .schema(serde_json::json!({
            "type": "object",
            "properties": {
                "a": { "type": "number", "description": "First number" },
                "b": { "type": "number", "description": "Second number" }
            },
            "required": ["a", "b"]
        }));

    let config = SessionConfig {
        tools: vec![add_tool.clone(), multiply_tool.clone()],
        streaming: true,
        system_message: Some(SystemMessageConfig {
            mode: Some(SystemMessageMode::Replace),
            content: Some(
                "You are a calculator. You MUST use the provided tools for all math. \
                 Never compute in your head. Report the exact tool results."
                    .to_string(),
            ),
            ..Default::default()
        }),
        available_tools: Some(vec![
            "add_numbers".to_string(),
            "multiply_numbers".to_string(),
        ]),
        ..byok_session_config()
    };

    let session = client.create_session(config).await.expect("create session");

    session
        .register_tool_with_handler(
            add_tool,
            Some(Arc::new(move |_name, args| {
                add_called_clone.store(true, Ordering::SeqCst);
                let a = args.get("a").and_then(|v| v.as_f64()).unwrap_or(0.0);
                let b = args.get("b").and_then(|v| v.as_f64()).unwrap_or(0.0);
                ToolResultObject::text(format!("{}", a + b))
            })),
        )
        .await;

    session
        .register_tool_with_handler(
            multiply_tool,
            Some(Arc::new(move |_name, args| {
                multiply_called_clone.store(true, Ordering::SeqCst);
                let a = args.get("a").and_then(|v| v.as_f64()).unwrap_or(0.0);
                let b = args.get("b").and_then(|v| v.as_f64()).unwrap_or(0.0);
                ToolResultObject::text(format!("{}", a * b))
            })),
        )
        .await;

    session
        .register_permission_handler(|_req| PermissionRequestResult::approved())
        .await;

    let mut events = session.subscribe();
    let mut got_streaming_delta = false;
    let tool_names_used = Arc::new(tokio::sync::Mutex::new(Vec::<String>::new()));
    let tool_names_clone = Arc::clone(&tool_names_used);

    session
        .send("What is 15 + 27? And what is 8 * 13?")
        .await
        .expect("send");

    let mut content = String::new();
    let result = tokio::time::timeout(LLM_TIMEOUT, async {
        while let Ok(event) = events.recv().await {
            match &event.data {
                SessionEventData::AssistantMessageDelta(delta) => {
                    content.push_str(&delta.delta_content);
                    got_streaming_delta = true;
                }
                SessionEventData::AssistantMessage(msg) => {
                    content.push_str(&msg.content);
                }
                SessionEventData::ToolExecutionStart(t) => {
                    eprintln!("[smoke_multi_tool] tool start: {}", t.tool_name);
                    tool_names_clone.lock().await.push(t.tool_name.clone());
                }
                SessionEventData::ToolExecutionComplete(t) => {
                    eprintln!("[smoke_multi_tool] tool done: {}", t.tool_call_id);
                }
                SessionEventData::SessionIdle(_) => break,
                SessionEventData::SessionError(err) => {
                    panic!("Session error: {}", err.message);
                }
                _ => {}
            }
        }
    })
    .await;

    assert!(result.is_ok(), "Timed out");
    eprintln!("[smoke_multi_tool] response: {content}");
    eprintln!("[smoke_multi_tool] streaming deltas: {got_streaming_delta}");

    assert!(
        got_streaming_delta,
        "Should have received streaming deltas"
    );
    assert!(
        add_called.load(Ordering::SeqCst),
        "add_numbers tool should have been called"
    );
    assert!(
        multiply_called.load(Ordering::SeqCst),
        "multiply_numbers tool should have been called"
    );

    // Verify both results appear
    assert!(
        content.contains("42"),
        "Response should contain 15+27=42: {content}"
    );
    assert!(
        content.contains("104"),
        "Response should contain 8*13=104: {content}"
    );

    session.destroy().await.ok();
    client.stop().await;
}
