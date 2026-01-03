use std::path::Path;
use std::process::Command;
use std::time::Duration;

use axum::{
    Json, Router,
    extract::{Query, State},
    response::Json as ResponseJson,
    routing::{get, post},
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
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PlanPhaseProgress {
    pub total: u32,
    pub completed: u32,
    #[serde(rename = "inProgress")]
    pub in_progress: u32,
    pub pending: u32,
    pub percentage: u32,
}

/// Individual phase detail from plan
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PlanPhaseDetail {
    pub phase: u32,
    pub name: String,
    pub status: String,
    pub file: String,
    #[serde(rename = "linkText")]
    pub link_text: Option<String>,
}

/// Plan metadata from JS scanner output
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PlanMetadata {
    pub id: String,
    pub name: String,
    pub path: String,
    pub directory: String,
    pub phases: PlanPhaseProgress,
    #[serde(rename = "phaseDetails", default)]
    pub phase_details: Vec<PlanPhaseDetail>,
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

/// Selected phases for a plan during import
#[derive(Debug, Clone, Deserialize, TS)]
#[ts(export)]
pub struct PlanPhaseSelection {
    /// Plan ID
    pub plan_id: String,
    /// Phase numbers to import. If empty, import all phases.
    #[serde(default)]
    pub phases: Vec<u32>,
}

/// Request payload for plan import
#[derive(Debug, Deserialize, TS)]
#[ts(export)]
pub struct ImportPlansRequest {
    pub project_id: Uuid,
    /// Optional filter: only import these plan IDs (legacy - use selections for phase-level control)
    pub plan_ids: Option<Vec<String>>,
    /// Phase-level selection per plan
    #[serde(default)]
    pub selections: Vec<PlanPhaseSelection>,
}

/// Response for plan import
#[derive(Debug, Serialize, TS)]
#[ts(export)]
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

/// Query params for listing plans
#[derive(Debug, Deserialize)]
pub struct ListPlansQuery {
    pub project_id: Uuid,
}

/// List available plans for a project
pub async fn list_plans(
    State(deployment): State<DeploymentImpl>,
    Query(query): Query<ListPlansQuery>,
) -> Result<ResponseJson<ApiResponse<Vec<PlanMetadata>>>, ApiError> {
    let pool = &deployment.db().pool;

    // Get the first repo for the project to determine the project root
    let repos = ProjectRepo::find_repos_for_project(pool, query.project_id).await?;
    let first_repo = repos.first().ok_or(ApiError::BadRequest(
        "Project has no repositories configured".to_string(),
    ))?;

    let project_root = first_repo.path.to_string_lossy().to_string();

    // Scan plans using Node.js script
    let plans = scan_plans_via_node(&project_root)
        .map_err(|e| ApiError::BadRequest(format!("Failed to scan plans: {}", e)))?;

    Ok(ResponseJson(ApiResponse::success(plans)))
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

    // Build a map of plan_id -> selected phases for quick lookup
    let phase_selections: std::collections::HashMap<String, Vec<u32>> = payload
        .selections
        .iter()
        .map(|s| (s.plan_id.clone(), s.phases.clone()))
        .collect();

    // Determine which plans to import
    let plans_to_import: Vec<&PlanMetadata> = if !payload.selections.is_empty() {
        // New behavior: use selections
        let selected_ids: std::collections::HashSet<_> = phase_selections.keys().cloned().collect();
        plans.iter().filter(|p| selected_ids.contains(&p.id)).collect()
    } else if let Some(ref ids) = payload.plan_ids {
        // Legacy behavior: filter by plan_ids, import all phases
        plans.iter().filter(|p| ids.contains(&p.id)).collect()
    } else {
        plans.iter().collect()
    };

    let mut task_ids = Vec::new();
    let mut errors = Vec::new();

    for plan in plans_to_import {
        let plan_title = plan.title.clone().unwrap_or_else(|| plan.name.clone());

        // Check if specific phases are selected for this plan
        let selected_phases = phase_selections.get(&plan.id);

        // If plan has phases, create a task for each (or selected) phase
        if !plan.phase_details.is_empty() {
            for phase in &plan.phase_details {
                // Skip if specific phases are selected and this phase isn't in the list
                if let Some(phases) = selected_phases {
                    if !phases.is_empty() && !phases.contains(&phase.phase) {
                        continue;
                    }
                }
                let title = format!("{} - Phase {}: {}", plan_title, phase.phase, phase.name);
                let status = map_plan_status_to_task(&phase.status);
                let description = Some(format!(
                    "Plan: {}\nPhase file: {}",
                    plan.name,
                    phase.file
                ));

                let create_task = CreateTask {
                    project_id: payload.project_id,
                    title,
                    description,
                    status: Some(status),
                    parent_workspace_id: None,
                    image_ids: None,
                    shared_task_id: None,
                };

                let task_id = Uuid::new_v4();
                match Task::create(pool, &create_task, task_id).await {
                    Ok(task) => {
                        tracing::info!(
                            "Imported phase {} of plan '{}' as task {}",
                            phase.phase,
                            plan.name,
                            task.id
                        );
                        task_ids.push(task.id);
                    }
                    Err(e) => {
                        let err_msg = format!(
                            "Failed to create task for phase {} of '{}': {}",
                            phase.phase, plan.name, e
                        );
                        tracing::error!("{}", err_msg);
                        errors.push(err_msg);
                    }
                }
            }
        } else {
            // No phases, create single task for the plan
            let status = map_plan_status_to_task(&plan.status);

            let create_task = CreateTask {
                project_id: payload.project_id,
                title: plan_title.clone(),
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

/// Query params for reading plan file content
#[derive(Debug, Deserialize)]
pub struct ReadPlanFileQuery {
    pub project_id: Uuid,
    pub file_path: String,
}

/// Response for plan file content
#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct PlanFileContent {
    pub content: String,
    pub file_name: String,
}

/// Read a plan/phase markdown file content
pub async fn read_plan_file(
    State(deployment): State<DeploymentImpl>,
    Query(query): Query<ReadPlanFileQuery>,
) -> Result<ResponseJson<ApiResponse<PlanFileContent>>, ApiError> {
    let pool = &deployment.db().pool;

    // Get the first repo for the project to determine the project root
    let repos = ProjectRepo::find_repos_for_project(pool, query.project_id).await?;
    let first_repo = repos.first().ok_or(ApiError::BadRequest(
        "Project has no repositories configured".to_string(),
    ))?;

    let project_root = first_repo.path.to_string_lossy().to_string();

    // Validate path is within project
    let file_path = Path::new(&query.file_path);
    let project_path = Path::new(&project_root);

    // Canonicalize paths for comparison
    let canonical_file = file_path.canonicalize().map_err(|_| {
        ApiError::BadRequest("File not found".to_string())
    })?;
    let canonical_project = project_path.canonicalize().map_err(|_| {
        ApiError::BadRequest("Project path not found".to_string())
    })?;

    // Ensure file is within project directory
    if !canonical_file.starts_with(&canonical_project) {
        return Err(ApiError::BadRequest("File path must be within project".to_string()));
    }

    // Ensure it's a markdown file
    if canonical_file.extension().and_then(|e| e.to_str()) != Some("md") {
        return Err(ApiError::BadRequest("Only markdown files are allowed".to_string()));
    }

    // Read the file content
    let content = std::fs::read_to_string(&canonical_file).map_err(|e| {
        ApiError::BadRequest(format!("Failed to read file: {}", e))
    })?;

    let file_name = canonical_file
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown.md")
        .to_string();

    Ok(ResponseJson(ApiResponse::success(PlanFileContent {
        content,
        file_name,
    })))
}

pub fn router(_deployment: &DeploymentImpl) -> Router<DeploymentImpl> {
    Router::new()
        .route("/", get(list_plans))
        .route("/import", post(import_plans))
        .route("/file", get(read_plan_file))
}
