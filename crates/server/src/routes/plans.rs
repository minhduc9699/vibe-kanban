use std::path::Path;
use std::process::Command;
use std::time::Duration;

use axum::{
    Json, Router,
    extract::State,
    response::Json as ResponseJson,
    routing::post,
};
use db::models::{
    project_repo::ProjectRepo,
    task::{CreateTask, Task, TaskStatus},
};
use deployment::Deployment;
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use utils::response::ApiResponse;
use uuid::Uuid;

use crate::{DeploymentImpl, error::ApiError};

/// Plan phase progress from scanner
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanPhaseProgress {
    pub total: u32,
    pub completed: u32,
    #[serde(rename = "inProgress")]
    pub in_progress: u32,
    pub pending: u32,
    pub percentage: u32,
}

/// Plan metadata from JS scanner output
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanMetadata {
    pub id: String,
    pub name: String,
    pub path: String,
    pub directory: String,
    pub phases: PlanPhaseProgress,
    pub progress: u32,
    #[serde(rename = "lastModified")]
    pub last_modified: String,
    pub status: String,
    pub description: Option<String>,
    pub priority: Option<String>,
    pub branch: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    pub title: Option<String>,
}

/// Request payload for plan import
#[derive(Debug, Deserialize, TS)]
pub struct ImportPlansRequest {
    pub project_id: Uuid,
    /// Optional filter: only import these plan IDs
    pub plan_ids: Option<Vec<String>>,
}

/// Response for plan import
#[derive(Debug, Serialize, TS)]
pub struct ImportPlansResponse {
    pub imported_count: u32,
    pub task_ids: Vec<Uuid>,
    pub errors: Vec<String>,
}

/// Map plan status string to TaskStatus
fn map_plan_status_to_task(plan_status: &str) -> TaskStatus {
    match plan_status.to_lowercase().as_str() {
        "in-progress" | "inprogress" | "in_progress" => TaskStatus::InProgress,
        "in-review" | "inreview" | "in_review" => TaskStatus::InReview,
        "completed" | "done" => TaskStatus::Done,
        "cancelled" | "canceled" => TaskStatus::Cancelled,
        other => {
            if !other.is_empty() && other != "pending" {
                tracing::debug!("Unknown plan status '{}', defaulting to Todo", other);
            }
            TaskStatus::Todo
        }
    }
}

/// Validate project path for safety
fn validate_project_path(project_root: &str) -> Result<(), String> {
    let path = Path::new(project_root);

    // Must be absolute path
    if !path.is_absolute() {
        return Err("Project path must be absolute".to_string());
    }

    // Must exist and be a directory
    if !path.is_dir() {
        return Err("Project path does not exist or is not a directory".to_string());
    }

    // Check for shell metacharacters that could be dangerous
    let dangerous_chars = [';', '|', '&', '\n', '\r', '\0', '`', '$', '(', ')'];
    if project_root.chars().any(|c| dangerous_chars.contains(&c)) {
        return Err("Project path contains invalid characters".to_string());
    }

    Ok(())
}

const SCANNER_TIMEOUT_SECS: u64 = 30;

/// Execute plan scanner and parse output
fn scan_plans_via_node(project_root: &str) -> Result<Vec<PlanMetadata>, String> {
    // Validate path first
    validate_project_path(project_root)?;

    // Check if node is available
    let node_check = Command::new("which")
        .arg("node")
        .output()
        .map_err(|e| format!("Failed to check node availability: {}", e))?;

    if !node_check.status.success() {
        return Err("Node.js is not installed or not in PATH".to_string());
    }

    let plans_dir = format!("{}/plans", project_root);
    let script_path = format!("{}/scripts/plan-scanner-json.cjs", project_root);

    // Check if script exists
    if !Path::new(&script_path).exists() {
        return Err(format!("Plan scanner script not found at {}", script_path));
    }

    // Execute with timeout using spawn and wait_with_output
    let mut child = Command::new("node")
        .arg(&script_path)
        .arg(&plans_dir)
        .current_dir(project_root)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to spawn plan scanner: {}", e))?;

    // Wait with timeout
    let timeout = Duration::from_secs(SCANNER_TIMEOUT_SECS);
    let start = std::time::Instant::now();

    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let output = child.wait_with_output()
                    .map_err(|e| format!("Failed to get scanner output: {}", e))?;

                if !status.success() {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    return Err(format!("Plan scanner failed: {}", stderr));
                }

                let stdout = String::from_utf8_lossy(&output.stdout);
                return serde_json::from_str(&stdout)
                    .map_err(|e| format!("Failed to parse scanner output: {} - output: {}", e, stdout));
            }
            Ok(None) => {
                if start.elapsed() > timeout {
                    let _ = child.kill();
                    return Err(format!(
                        "Plan scanner timed out after {}s",
                        SCANNER_TIMEOUT_SECS
                    ));
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => {
                return Err(format!("Failed to check scanner status: {}", e));
            }
        }
    }
}

/// Import plans from the plans directory into tasks
pub async fn import_plans(
    State(deployment): State<DeploymentImpl>,
    Json(payload): Json<ImportPlansRequest>,
) -> Result<ResponseJson<ApiResponse<ImportPlansResponse>>, ApiError> {
    let pool = &deployment.db().pool;

    // Get the first repo for the project to determine the project root
    let repos = ProjectRepo::find_repos_for_project(pool, payload.project_id).await?;
    let first_repo = repos.first().ok_or(ApiError::BadRequest(
        "Project has no repositories configured".to_string(),
    ))?;

    let project_root = first_repo.path.to_string_lossy().to_string();

    // Scan plans using Node.js script
    let plans = scan_plans_via_node(&project_root)
        .map_err(|e| ApiError::BadRequest(format!("Failed to scan plans: {}", e)))?;

    // Filter plans if specific IDs requested
    let plans_to_import: Vec<&PlanMetadata> = if let Some(ref ids) = payload.plan_ids {
        plans.iter().filter(|p| ids.contains(&p.id)).collect()
    } else {
        plans.iter().collect()
    };

    let mut task_ids = Vec::new();
    let mut errors = Vec::new();

    for plan in plans_to_import {
        let title = plan.title.clone().unwrap_or_else(|| plan.name.clone());
        let status = map_plan_status_to_task(&plan.status);

        let create_task = CreateTask {
            project_id: payload.project_id,
            title,
            description: plan.description.clone(),
            status: Some(status),
            parent_workspace_id: None,
            image_ids: None,
            shared_task_id: None,
        };

        let task_id = Uuid::new_v4();
        match Task::create(pool, &create_task, task_id).await {
            Ok(task) => {
                tracing::info!("Imported plan '{}' as task {}", plan.name, task.id);
                task_ids.push(task.id);
            }
            Err(e) => {
                let err_msg = format!("Failed to create task for plan '{}': {}", plan.name, e);
                tracing::error!("{}", err_msg);
                errors.push(err_msg);
            }
        }
    }

    let response = ImportPlansResponse {
        imported_count: task_ids.len() as u32,
        task_ids,
        errors,
    };

    deployment
        .track_if_analytics_allowed(
            "plans_imported",
            serde_json::json!({
                "project_id": payload.project_id.to_string(),
                "imported_count": response.imported_count,
                "error_count": response.errors.len(),
            }),
        )
        .await;

    Ok(ResponseJson(ApiResponse::success(response)))
}

pub fn router(_deployment: &DeploymentImpl) -> Router<DeploymentImpl> {
    Router::new().route("/import", post(import_plans))
}
