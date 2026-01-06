//! CCS (Claude Code Switch) executor - routes to multiple AI providers
//! via a unified Claude-compatible interface.
//!
//! Uses the same protocol layer as Claude executor for bidirectional
//! stdin/stdout communication with control protocol support.

use std::{path::Path, process::Stdio, sync::Arc};

use async_trait::async_trait;
use command_group::AsyncCommandGroup;
use derivative::Derivative;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tokio::process::Command;
use ts_rs::TS;
use workspace_utils::msg_store::MsgStore;

use crate::{
    approvals::ExecutorApprovalService,
    command::{CmdOverrides, CommandBuilder, CommandParts, apply_overrides},
    env::ExecutionEnv,
    executors::{
        AppendPrompt, AvailabilityInfo, ExecutorError, SpawnedChild, StandardCodingAgentExecutor,
        claude::{
            ClaudeLogProcessor, HistoryStrategy,
            client::ClaudeAgentClient,
            protocol::ProtocolPeer,
            types::PermissionMode,
        },
        codex::client::LogWriter,
    },
    logs::{stderr_processor::normalize_stderr_logs, utils::EntryIndexProvider},
    stdout_dup::create_stdout_pipe_writer,
};

/// Allowed CCS providers - validated to prevent command injection
const ALLOWED_PROVIDERS: &[&str] = &["gemini", "codev", "agy", "qwen", "iflow", "kiro", "ghcp"];

/// CCS (Claude Code Switch) executor - routes to multiple AI providers
/// via a unified Claude-compatible interface.
///
/// Providers: gemini, codev, agy, qwen, iflow, kiro, ghcp
#[derive(Derivative, Clone, Serialize, Deserialize, TS, JsonSchema)]
#[derivative(Debug, PartialEq)]
pub struct Ccs {
    /// Provider to use (gemini, codev, agy, qwen, iflow, kiro, ghcp)
    pub provider: String,

    #[serde(default)]
    pub append_prompt: AppendPrompt,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dangerously_skip_permissions: Option<bool>,

    /// Enable interactive approvals via protocol (like Claude)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approvals: Option<bool>,

    #[serde(flatten)]
    pub cmd: CmdOverrides,

    /// Approval service for interactive tool approvals (injected at runtime)
    #[serde(skip)]
    #[ts(skip)]
    #[derivative(Debug = "ignore", PartialEq = "ignore")]
    approvals_service: Option<Arc<dyn ExecutorApprovalService>>,
}

impl Ccs {
    /// Validates provider and returns base command.
    /// Returns error if provider contains invalid characters (security).
    fn base_command(&self) -> Result<String, ExecutorError> {
        // Validate provider is alphanumeric (prevents command injection)
        let provider = self.provider.trim();
        if !provider.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            return Err(ExecutorError::UnknownExecutorType(format!(
                "Invalid CCS provider: {}. Provider must be alphanumeric.",
                provider
            )));
        }
        // Warn if not in known list (but still allow for extensibility)
        if !ALLOWED_PROVIDERS.contains(&provider) {
            tracing::warn!(
                "CCS provider '{}' not in known list: {:?}",
                provider,
                ALLOWED_PROVIDERS
            );
        }
        Ok(format!("ccs {}", provider))
    }

    fn build_command_builder(&self) -> Result<CommandBuilder, ExecutorError> {
        let base_cmd = self.base_command()?;
        // CCS takes prompt as positional arg at end (no -p flag)
        let mut builder = CommandBuilder::new(base_cmd);

        // Enable stdio permission prompt for approvals mode
        if self.approvals.unwrap_or(false) {
            builder = builder.extend_params(["--permission-prompt-tool=stdio"]);
            builder = builder.extend_params([format!(
                "--permission-mode={}",
                PermissionMode::BypassPermissions
            )]);
        }

        // Add flags before prompt (which gets appended last in spawn_internal)
        if self.dangerously_skip_permissions.unwrap_or(false) {
            builder = builder.extend_params(["--dangerously-skip-permissions"]);
        }

        builder = builder.extend_params([
            "--verbose",
            "--print",
            "--output-format=stream-json",
            "--input-format=stream-json",
            "--include-partial-messages",
            "--disallowedTools=AskUserQuestion",
        ]);

        if let Some(model) = &self.model {
            builder = builder.extend_params(["--model", model]);
        }

        Ok(apply_overrides(builder, &self.cmd))
    }

    /// Get the permission mode based on configuration
    pub fn permission_mode(&self) -> PermissionMode {
        if self.approvals.unwrap_or(false) {
            PermissionMode::Default
        } else {
            PermissionMode::BypassPermissions
        }
    }

    /// Get hooks configuration for approval mode
    pub fn get_hooks(&self) -> Option<serde_json::Value> {
        if self.approvals.unwrap_or(false) {
            Some(serde_json::json!({
                "PreToolUse": [
                    {
                        "matcher": "^(?!(Glob|Grep|NotebookRead|Read|Task|TodoWrite)$).*",
                        "hookCallbackIds": ["tool_approval"],
                    }
                ]
            }))
        } else {
            None
        }
    }

    async fn spawn_internal(
        &self,
        current_dir: &Path,
        prompt: &str,
        command_parts: CommandParts,
        env: &ExecutionEnv,
    ) -> Result<SpawnedChild, ExecutorError> {
        let (program_path, args) = command_parts.into_resolved().await?;
        let combined_prompt = self.append_prompt.combine_prompt(prompt);

        let mut command = Command::new(program_path);
        command
            .kill_on_drop(true)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .current_dir(current_dir)
            .args(&args)
            .arg(&combined_prompt);

        env.clone()
            .with_profile(&self.cmd)
            .apply_to_command(&mut command);

        let mut child = command.group_spawn()?;
        let child_stdout = child.inner().stdout.take().ok_or_else(|| {
            ExecutorError::Io(std::io::Error::other("CCS missing stdout"))
        })?;
        let child_stdin = child.inner().stdin.take().ok_or_else(|| {
            ExecutorError::Io(std::io::Error::other("CCS missing stdin"))
        })?;

        let new_stdout = create_stdout_pipe_writer(&mut child)?;
        let permission_mode = self.permission_mode();
        let hooks = self.get_hooks();

        // Create interrupt channel for graceful shutdown
        let (interrupt_tx, interrupt_rx) = tokio::sync::oneshot::channel::<()>();

        // Spawn task to handle the SDK client with control protocol
        let prompt_clone = combined_prompt.clone();
        let approvals_clone = self.approvals_service.clone();
        tokio::spawn(async move {
            let log_writer = LogWriter::new(new_stdout);
            let client = ClaudeAgentClient::new(log_writer.clone(), approvals_clone);
            let protocol_peer =
                ProtocolPeer::spawn(child_stdin, child_stdout, client.clone(), interrupt_rx);

            // Initialize control protocol
            if let Err(e) = protocol_peer.initialize(hooks).await {
                tracing::error!("Failed to initialize CCS control protocol: {e}");
                let _ = log_writer
                    .log_raw(&format!("Error: Failed to initialize - {e}"))
                    .await;
                return;
            }

            if let Err(e) = protocol_peer.set_permission_mode(permission_mode).await {
                tracing::warn!("Failed to set CCS permission mode to {permission_mode}: {e}");
            }

            // Send user message
            if let Err(e) = protocol_peer.send_user_message(prompt_clone).await {
                tracing::error!("Failed to send CCS prompt: {e}");
                let _ = log_writer
                    .log_raw(&format!("Error: Failed to send prompt - {e}"))
                    .await;
            }
        });

        Ok(SpawnedChild {
            child,
            exit_signal: None,
            interrupt_sender: Some(interrupt_tx),
        })
    }
}

#[async_trait]
impl StandardCodingAgentExecutor for Ccs {
    fn use_approvals(&mut self, approvals: Arc<dyn ExecutorApprovalService>) {
        self.approvals_service = Some(approvals);
    }

    async fn spawn(
        &self,
        current_dir: &Path,
        prompt: &str,
        env: &ExecutionEnv,
    ) -> Result<SpawnedChild, ExecutorError> {
        let command_parts = self.build_command_builder()?.build_initial()?;
        self.spawn_internal(current_dir, prompt, command_parts, env)
            .await
    }

    async fn spawn_follow_up(
        &self,
        current_dir: &Path,
        prompt: &str,
        session_id: &str,
        env: &ExecutionEnv,
    ) -> Result<SpawnedChild, ExecutorError> {
        let command_parts = self.build_command_builder()?.build_follow_up(&[
            "--fork-session".to_string(),
            "--resume".to_string(),
            session_id.to_string(),
        ])?;
        self.spawn_internal(current_dir, prompt, command_parts, env)
            .await
    }

    fn normalize_logs(&self, msg_store: Arc<MsgStore>, current_dir: &Path) {
        let entry_index_provider = EntryIndexProvider::start_from(&msg_store);

        // Reuse Claude's log processor - CCS outputs compatible JSON
        ClaudeLogProcessor::process_logs(
            msg_store.clone(),
            current_dir,
            entry_index_provider.clone(),
            HistoryStrategy::Default,
        );

        normalize_stderr_logs(msg_store, entry_index_provider);
    }

    fn default_mcp_config_path(&self) -> Option<std::path::PathBuf> {
        // CCS uses Claude's config
        dirs::home_dir().map(|home| home.join(".claude.json"))
    }

    fn get_availability_info(&self) -> AvailabilityInfo {
        // Check if ccs command exists
        match std::process::Command::new("which").arg("ccs").output() {
            Ok(output) if output.status.success() => AvailabilityInfo::InstallationFound,
            _ => AvailabilityInfo::NotFound,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_ccs(provider: &str) -> Ccs {
        Ccs {
            provider: provider.to_string(),
            append_prompt: AppendPrompt::default(),
            model: None,
            dangerously_skip_permissions: None,
            approvals: None,
            cmd: CmdOverrides::default(),
            approvals_service: None,
        }
    }

    #[test]
    fn test_base_command_with_provider() {
        let ccs = create_test_ccs("gemini");
        assert_eq!(ccs.base_command().unwrap(), "ccs gemini");

        let ccs_codev = create_test_ccs("codev");
        assert_eq!(ccs_codev.base_command().unwrap(), "ccs codev");
    }

    #[test]
    fn test_base_command_rejects_invalid_provider() {
        let ccs = create_test_ccs("gemini; rm -rf /");
        assert!(ccs.base_command().is_err());

        let ccs = create_test_ccs("foo|bar");
        assert!(ccs.base_command().is_err());

        let ccs = create_test_ccs("$(whoami)");
        assert!(ccs.base_command().is_err());
    }

    #[test]
    fn test_command_builder_includes_json_flags() {
        let ccs = create_test_ccs("agy");
        let builder = ccs.build_command_builder().unwrap();
        let parts = builder.build_initial().unwrap();

        let cmd_string = format!("{:?}", parts);
        assert!(cmd_string.contains("--output-format=stream-json"));
        assert!(cmd_string.contains("--input-format=stream-json"));
        // CCS uses positional prompt arg (no -p flag like Claude Code)
        assert!(!cmd_string.contains("\"-p\""));
    }

    #[test]
    fn test_command_builder_with_model() {
        let mut ccs = create_test_ccs("qwen");
        ccs.model = Some("qwen-max".to_string());

        let builder = ccs.build_command_builder().unwrap();
        let parts = builder.build_initial().unwrap();

        let cmd_string = format!("{:?}", parts);
        assert!(cmd_string.contains("--model"));
        assert!(cmd_string.contains("qwen-max"));
    }

    #[test]
    fn test_command_builder_skip_permissions() {
        let mut ccs = create_test_ccs("iflow");
        ccs.dangerously_skip_permissions = Some(true);

        let builder = ccs.build_command_builder().unwrap();
        let parts = builder.build_initial().unwrap();

        let cmd_string = format!("{:?}", parts);
        assert!(cmd_string.contains("--dangerously-skip-permissions"));
    }

    #[test]
    fn test_all_providers_valid() {
        let providers = ["gemini", "codev", "agy", "qwen", "iflow", "kiro", "ghcp"];

        for provider in providers {
            let ccs = create_test_ccs(provider);
            assert_eq!(ccs.provider, provider);
            assert!(ccs.base_command().unwrap().starts_with("ccs "));
        }
    }
}
