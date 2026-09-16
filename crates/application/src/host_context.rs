//! Bind host claims to independently read Task/Session and registered source metadata.
#[cfg(test)]
#[path = "host_context_tests.rs"]
mod tests;
use crate::db::{check_version, load_session, load_task_by_reference};
use crate::{AppError, HostEvidenceAssessment, Service, assess_host_evidence};
use serde_json::json;
use sha2::{Digest, Sha256};
use steward_core::{HostBinding, HostEvidence, SessionView, TaskView};

/// Values supplied by the adapter's current runtime, not copied out of a report.
pub struct HostInstance<'a> {
    pub name: &'a str,
    pub version: &'a str,
    pub instance_id: &'a str,
}

/// Explicit metadata read request. Does not claim a task or read source files.
pub struct HostContextRequest<'a> {
    pub task: &'a str,
    pub task_version: i64,
    pub session_id: &'a str,
    pub source_id: i64,
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
        if task.current_session_id.as_deref() != Some(request.session_id) {
            return Err(invalid("request must use the task's current Session"));
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
        let database_identity = crate::path_safety::identify_existing(self.database_path())
            .map_err(|e| AppError::from_path(e, None))?;
        let project = before.0.project_id.expect("project checked");
        let context = self
            .project_source(&format!("##{project}"), request.source_id)?
            .data;
        let source = &context["source"];
        if let Some(component) = source["componentId"].as_i64()
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
        if context
            != self
                .project_source(&format!("##{project}"), request.source_id)?
                .data
        {
            return Err(invalid(
                "project source metadata changed during context binding",
            ));
        }
        crate::path_safety::verify_existing_identity(&database_identity)
            .map_err(|e| AppError::from_path(e, None))?;
        // Version 2 binds registration data only, not source contents or Git state.
        let scope = json!({"bindingVersion":2,"databaseIdentity":database_identity,"taskId":before.0.id,"taskVersion":before.0.version,
            "projectId":project,"project":context["project"],"componentIds":before.0.component_ids,"sessionId":before.1.id,"sessionStartedAt":before.1.started_at,
            "sourceId":request.source_id,"source":source});
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
