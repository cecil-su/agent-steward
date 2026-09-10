//! Evidence-backed project profiles. Callers must not submit secrets or credentials.
//! Text validation enforces shape and size, not automatic secret detection.
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::json;
use storage_sqlite::now;

use crate::db::{check_version, load_task};
use crate::projects::{
    check_project_revision, load_project, record_project_change, resolve_project_id,
};
use crate::{AppError, AppResult, Outcome, Service};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectProfileInput {
    pub summary: String,
    pub architecture: String,
    pub development: String,
    pub source_task_id: i64,
    pub source_task_version: i64,
    pub evidence: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectProfileView {
    pub project_id: i64,
    pub revision: i64,
    pub summary: String,
    pub architecture: String,
    pub development: String,
    pub source_task_id: i64,
    pub source_task_version: i64,
    pub evidence: String,
    pub updated_at: String,
}

fn text(field: &str, value: &str, limit: usize) -> AppResult<String> {
    let value = value.trim();
    if value.is_empty() || value.contains('\0') || value.chars().count() > limit {
        return Err(AppError::invalid(
            field,
            format!("must contain 1..={limit} Unicode characters after trimming, without NUL"),
        ));
    }
    Ok(value.to_owned())
}

pub(crate) fn load_profile(
    connection: &Connection,
    project_id: i64,
) -> AppResult<Option<ProjectProfileView>> {
    connection
        .query_row(
            "SELECT project_id,revision,summary,architecture,development,source_task_id,
                    source_task_version,evidence,updated_at FROM project_profiles WHERE project_id=?1",
            [project_id],
            |row| {
                Ok(ProjectProfileView {
                    project_id: row.get(0)?,
                    revision: row.get(1)?,
                    summary: row.get(2)?,
                    architecture: row.get(3)?,
                    development: row.get(4)?,
                    source_task_id: row.get(5)?,
                    source_task_version: row.get(6)?,
                    evidence: row.get(7)?,
                    updated_at: row.get(8)?,
                })
            },
        )
        .optional()
        .map_err(AppError::from_sqlite)
}

impl Service {
    pub fn project_profile_show(&self, reference: &str) -> AppResult<Outcome> {
        let mut connection = self.connection()?;
        let tx = connection.transaction().map_err(AppError::from_sqlite)?;
        let project = load_project(&tx, resolve_project_id(&tx, reference)?)?;
        let profile = load_profile(&tx, project.id)?;
        tx.commit().map_err(AppError::from_sqlite)?;
        Ok(Outcome::new(
            json!({"project": project, "profile": profile}),
        ))
    }

    pub fn project_profile_set(
        &self,
        reference: &str,
        expected_revision: i64,
        input: ProjectProfileInput,
    ) -> AppResult<Outcome> {
        let summary = text("summary", &input.summary, 4000)?;
        let architecture = text("architecture", &input.architecture, 8000)?;
        let development = text("development", &input.development, 8000)?;
        let evidence = text("evidence", &input.evidence, 4000)?;
        if input.source_task_id <= 0 {
            return Err(AppError::invalid("sourceTaskId", "must be positive"));
        }
        if input.source_task_version <= 0 {
            return Err(AppError::invalid("sourceTaskVersion", "must be positive"));
        }
        let mut connection = self.connection()?;
        let tx =
            storage_sqlite::write_transaction(&mut connection).map_err(AppError::from_storage)?;
        let project = load_project(&tx, resolve_project_id(&tx, reference)?)?;
        check_project_revision(&project, expected_revision)?;
        let source = load_task(&tx, input.source_task_id)?;
        if source.project_id != Some(project.id) {
            return Err(AppError::invalid(
                "sourceTaskId",
                "source task must currently belong to this project",
            ));
        }
        check_version(&source, input.source_task_version)?;
        let before = load_profile(&tx, project.id)?;
        let profile = ProjectProfileView {
            project_id: project.id,
            revision: project
                .revision
                .checked_add(1)
                .ok_or_else(|| AppError::constraint("projects.revision"))?,
            summary,
            architecture,
            development,
            source_task_id: source.id,
            source_task_version: source.version,
            evidence,
            updated_at: now(),
        };
        tx.execute(
            "INSERT INTO project_profiles(project_id,revision,summary,architecture,development,
                 source_task_id,source_task_version,evidence,updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)
             ON CONFLICT(project_id) DO UPDATE SET revision=excluded.revision,
                 summary=excluded.summary,architecture=excluded.architecture,
                 development=excluded.development,source_task_id=excluded.source_task_id,
                 source_task_version=excluded.source_task_version,evidence=excluded.evidence,
                 updated_at=excluded.updated_at",
            params![
                profile.project_id,
                profile.revision,
                profile.summary,
                profile.architecture,
                profile.development,
                profile.source_task_id,
                profile.source_task_version,
                profile.evidence,
                profile.updated_at
            ],
        )
        .map_err(AppError::from_sqlite)?;
        let project = record_project_change(
            &tx,
            &project,
            "project.profile_updated",
            json!({"before": before, "after": profile, "sourceTaskId": source.id,
                "sourceTaskVersion": source.version, "evidence": profile.evidence}),
        )?;
        tx.commit().map_err(AppError::from_sqlite)?;
        Ok(Outcome::new(
            json!({"project": project, "profile": profile}),
        ))
    }
}
