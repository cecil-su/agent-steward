//! Bind host claims to independently read Task/Session and live context observations.
#[cfg(test)]
#[path = "host_context_tests.rs"]
mod tests;
use crate::db::{check_version, load_session, load_task_by_reference};
use crate::{
    AppError, HostEvidenceAssessment, ProjectContextOptions, Service, assess_host_evidence,
};
use serde_json::json;
use sha2::{Digest, Sha256};
use steward_core::{HostBinding, HostEvidence, SessionView, TaskStatus, TaskView};

/// Values supplied by the adapter's current runtime, not copied out of a report.
pub struct HostInstance<'a> {
    pub name: &'a str,
    pub version: &'a str,
    pub instance_id: &'a str,
}

/// Explicit read request. Does not claim a task, change its scope, or adopt a worktree.
pub struct HostContextRequest<'a> {
    pub task: &'a str,
    pub task_version: i64,
    pub session_id: &'a str,
    pub source_id: i64,
    pub context: ProjectContextOptions<'a>,
}

fn invalid(message: &str) -> AppError {
    AppError::invalid("hostContext", message)
}
fn current_time() -> u64 {
    u64::try_from(chrono::Utc::now().timestamp_millis()).unwrap_or(u64::MAX)
}

impl Service {
    fn host_task_snapshot(
        &self,
        request: &HostContextRequest<'_>,
    ) -> Result<(TaskView, SessionView), AppError> {
        let mut connection = self.connection()?;
        let tx = connection.transaction().map_err(AppError::from_sqlite)?;
        let task = load_task_by_reference(&tx, request.task)?;
        check_version(&task, request.task_version)?;
        if task.status == TaskStatus::Closed
            || task.current_session_id.as_deref() != Some(request.session_id)
        {
            return Err(invalid("request must use the open task's current Session"));
        }
        let session = load_session(&tx, request.session_id)?;
        if session.task_id != task.id || session.ended_at.is_some() {
            return Err(invalid("Session is ended or belongs to another task"));
        }
        if task.project_id.is_none() {
            return Err(invalid("task must explicitly belong to a project"));
        }
        tx.commit().map_err(AppError::from_sqlite)?;
        Ok((task, session))
    }

    /// Scope is derived here, never accepted from HostEvidence.binding.scopeSha256.
    /// The host instance remains a caller report, not an authenticated process identity.
    pub fn host_context_binding(
        &self,
        request: &HostContextRequest<'_>,
        host: &HostInstance<'_>,
    ) -> Result<HostBinding, AppError> {
        let before = self.host_task_snapshot(request)?;
        let database_identity = git_adapter::identify_existing(self.database_path())
            .map_err(|e| AppError::from_git(e, None))?;
        let project = before.0.project_id.expect("project checked");
        let context = self
            .project_context(
                &format!("##{project}"),
                request.source_id,
                ProjectContextOptions {
                    worktree: request.context.worktree,
                    files: request.context.files,
                    dependencies: request.context.dependencies,
                    budget_bytes: request.context.budget_bytes,
                },
            )?
            .data;
        if let Some(component) = context["source"]["componentId"].as_i64()
            && !before.0.component_ids.is_empty()
            && !before.0.component_ids.contains(&component)
        {
            return Err(invalid(
                "source component is outside the task's explicit component selection",
            ));
        }
        let after = self.host_task_snapshot(request)?;
        if before != after {
            return Err(invalid("Task or Session changed during context binding"));
        }
        git_adapter::verify_existing_identity(&database_identity)
            .map_err(|e| AppError::from_git(e, None))?;
        // Bind request presentation as well: one observation hash may produce different prefixes.
        let scope = json!({"bindingVersion":1,"databaseIdentity":database_identity,"taskId":before.0.id,"taskVersion":before.0.version,
            "projectId":project,"componentIds":before.0.component_ids,"sessionId":before.1.id,"sessionStartedAt":before.1.started_at,
            "sourceId":request.source_id,"contextVersion":context["contextVersion"],"contextFingerprint":context["fingerprint"],
            "files":request.context.files,"dependencies":request.context.dependencies,"budgetBytes":request.context.budget_bytes});
        let binding = HostBinding {
            host: host.name.into(),
            host_version: host.version.into(),
            instance_id: host.instance_id.into(),
            session_id: request.session_id.into(),
            scope_sha256: hex::encode(Sha256::digest(
                serde_json::to_vec(&scope).expect("scope serializes"),
            )),
        };
        binding.validate().map_err(invalid)?;
        Ok(binding)
    }

    pub fn assess_task_host_evidence(
        &self,
        request: &HostContextRequest<'_>,
        host: &HostInstance<'_>,
        report: &HostEvidence,
    ) -> Result<HostEvidenceAssessment, AppError> {
        self.assess_task_host_evidence_with_clock(request, host, report, current_time)
    }

    fn assess_task_host_evidence_with_clock(
        &self,
        request: &HostContextRequest<'_>,
        host: &HostInstance<'_>,
        report: &HostEvidence,
        clock: impl Fn() -> u64,
    ) -> Result<HostEvidenceAssessment, AppError> {
        let started = clock();
        let expected = self.host_context_binding(request, host)?;
        let assessment = assess_host_evidence(report, &expected, &clock)?;
        let final_binding = self.host_context_binding(request, host)?;
        if final_binding != expected {
            return Err(invalid("request context changed during host assessment"));
        }
        let ended = clock();
        if ended < started {
            return Err(invalid(
                "clock moved backwards during bound host assessment",
            ));
        }
        report.validate(&final_binding, ended).map_err(invalid)?;
        Ok(assessment)
    }
}
