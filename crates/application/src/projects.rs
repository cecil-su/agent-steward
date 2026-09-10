use rusqlite::{Connection, OptionalExtension, Row, Transaction, params};
use serde_json::{Value, json};
use steward_core::{ProjectReference, ProjectView, normalize_project_name};
use storage_sqlite::now;

use crate::{AppError, AppResult, Outcome, Service};

pub(crate) fn resolve_project_id(connection: &Connection, reference: &str) -> AppResult<i64> {
    let reference_value = ProjectReference::parse(reference)
        .map_err(|reason| AppError::invalid("project", reason))?;
    let id = match reference_value {
        ProjectReference::Id(id) => {
            connection.query_row("SELECT id FROM projects WHERE id=?1", [id], |row| {
                row.get(0)
            })
        }
        ProjectReference::NameKey(key) => {
            connection.query_row("SELECT id FROM projects WHERE name_key=?1", [key], |row| {
                row.get(0)
            })
        }
    }
    .optional()
    .map_err(AppError::from_sqlite)?;
    id.ok_or_else(|| AppError::not_found("Project", reference))
}

fn project_from_row(row: &Row<'_>) -> rusqlite::Result<ProjectView> {
    Ok(ProjectView {
        id: row.get(0)?,
        name: row.get(1)?,
        revision: row.get(2)?,
        created_at: row.get(3)?,
        updated_at: row.get(4)?,
    })
}

pub(crate) fn load_project(connection: &Connection, id: i64) -> AppResult<ProjectView> {
    connection
        .query_row(
            "SELECT id,name,revision,created_at,updated_at FROM projects WHERE id=?1",
            [id],
            project_from_row,
        )
        .optional()
        .map_err(AppError::from_sqlite)?
        .ok_or_else(|| AppError::not_found("Project", &id.to_string()))
}

fn validate_page(after: i64, limit: u32) -> AppResult<()> {
    if after < 0 || !(1..=200).contains(&limit) {
        return Err(AppError::invalid(
            "page",
            "after must be non-negative and limit must be between 1 and 200",
        ));
    }
    Ok(())
}

pub(crate) fn history(
    tx: &Transaction<'_>,
    project: &ProjectView,
    kind: &str,
    payload: Value,
) -> AppResult<()> {
    tx.execute("INSERT INTO project_history(project_id,revision,change_type,occurred_at,payload_json) VALUES (?1,?2,?3,?4,?5)",
        params![project.id, project.revision, kind, project.updated_at, payload.to_string()])
        .map_err(AppError::from_sqlite)?;
    Ok(())
}

pub(crate) fn check_project_revision(project: &ProjectView, expected: i64) -> AppResult<()> {
    if project.revision != expected {
        return Err(AppError::new(
            "VERSION_CONFLICT",
            "project revision does not match",
            true,
            json!({"entityType":"Project", "projectId": project.id, "expectedRevision":expected, "currentRevision":project.revision}),
            4,
        ));
    }
    Ok(())
}

pub(crate) fn record_project_change(
    tx: &Transaction<'_>,
    project: &ProjectView,
    kind: &str,
    payload: Value,
) -> AppResult<ProjectView> {
    let count = tx
        .execute(
            "UPDATE projects SET revision=revision+1,updated_at=?3 WHERE id=?1 AND revision=?2",
            params![project.id, project.revision, now()],
        )
        .map_err(AppError::from_sqlite)?;
    if count != 1 {
        return Err(AppError::constraint("projects.revision"));
    }
    let updated = load_project(tx, project.id)?;
    history(tx, &updated, kind, payload)?;
    Ok(updated)
}

impl Service {
    pub fn project_create(&self, name: &str) -> AppResult<Outcome> {
        let (name, key) =
            normalize_project_name(name).map_err(|reason| AppError::invalid("name", reason))?;
        let mut connection = self.connection()?;
        let tx =
            storage_sqlite::write_transaction(&mut connection).map_err(AppError::from_storage)?;
        let timestamp = now();
        tx.execute("INSERT INTO projects(name,name_key,revision,created_at,updated_at) VALUES (?1,?2,1,?3,?3)", params![name, key, timestamp])
            .map_err(AppError::from_sqlite)?;
        let project = load_project(&tx, tx.last_insert_rowid())?;
        history(
            &tx,
            &project,
            "project.created",
            json!({"name": project.name}),
        )?;
        tx.commit().map_err(AppError::from_sqlite)?;
        Ok(Outcome::new(json!({"project": project})))
    }

    pub fn project_show(&self, reference: &str) -> AppResult<Outcome> {
        let mut connection = self.connection()?;
        let tx = connection.transaction().map_err(AppError::from_sqlite)?;
        let project = load_project(&tx, resolve_project_id(&tx, reference)?)?;
        let profile = crate::project_profiles::load_profile(&tx, project.id)?;
        tx.commit().map_err(AppError::from_sqlite)?;
        Ok(Outcome::new(json!({"project": project, "profile": profile})))
    }

    pub fn project_list(&self, after: i64, limit: u32) -> AppResult<Outcome> {
        validate_page(after, limit)?;
        let connection = self.connection()?;
        let mut statement = connection.prepare("SELECT id,name,revision,created_at,updated_at FROM projects WHERE id>?1 ORDER BY id LIMIT ?2").map_err(AppError::from_sqlite)?;
        let mut projects = statement
            .query_map(params![after, i64::from(limit) + 1], project_from_row)
            .map_err(AppError::from_sqlite)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(AppError::from_sqlite)?;
        let has_more = projects.len() > limit as usize;
        projects.truncate(limit as usize);
        let next_after = has_more.then(|| projects.last().expect("non-empty page").id);
        Ok(Outcome::new(
            json!({"projects": projects, "hasMore": has_more, "nextAfter": next_after}),
        ))
    }

    pub fn project_rename(
        &self,
        reference: &str,
        expected_revision: i64,
        name: &str,
    ) -> AppResult<Outcome> {
        let (name, key) =
            normalize_project_name(name).map_err(|reason| AppError::invalid("name", reason))?;
        let mut connection = self.connection()?;
        let tx =
            storage_sqlite::write_transaction(&mut connection).map_err(AppError::from_storage)?;
        let project = load_project(&tx, resolve_project_id(&tx, reference)?)?;
        check_project_revision(&project, expected_revision)?;
        if project.name == name {
            tx.commit().map_err(AppError::from_sqlite)?;
            return Ok(Outcome::new(json!({"project": project})));
        }
        tx.execute("UPDATE projects SET name=?1,name_key=?2,revision=revision+1,updated_at=?3 WHERE id=?4 AND revision=?5",
            params![name, key, now(), project.id, expected_revision]).map_err(AppError::from_sqlite)?;
        let updated = load_project(&tx, project.id)?;
        history(
            &tx,
            &updated,
            "project.renamed",
            json!({"previousName": project.name, "name": updated.name}),
        )?;
        tx.commit().map_err(AppError::from_sqlite)?;
        Ok(Outcome::new(json!({"project": updated})))
    }

    pub fn project_history(&self, reference: &str, after: i64, limit: u32) -> AppResult<Outcome> {
        validate_page(after, limit)?;
        let mut connection = self.connection()?;
        let tx = connection.transaction().map_err(AppError::from_sqlite)?;
        let id = resolve_project_id(&tx, reference)?;
        let mut entries = {
            let mut statement = tx.prepare("SELECT revision,change_type,occurred_at,payload_json FROM project_history WHERE project_id=?1 AND revision>?2 ORDER BY revision LIMIT ?3").map_err(AppError::from_sqlite)?;
            statement.query_map(params![id, after, i64::from(limit)+1], |row| {
                let payload: String = row.get(3)?;
                let payload: Value = serde_json::from_str(&payload).map_err(|error| rusqlite::Error::FromSqlConversionFailure(3, rusqlite::types::Type::Text, Box::new(error)))?;
                Ok(json!({"projectId": id, "revision": row.get::<_, i64>(0)?, "changeType": row.get::<_, String>(1)?, "occurredAt": row.get::<_, String>(2)?, "payload": payload}))
            }).map_err(AppError::from_sqlite)?.collect::<Result<Vec<_>, _>>().map_err(AppError::from_sqlite)?
        };
        tx.commit().map_err(AppError::from_sqlite)?;
        let has_more = entries.len() > limit as usize;
        entries.truncate(limit as usize);
        let next_after =
            has_more.then(|| entries.last().expect("non-empty page")["revision"].clone());
        Ok(Outcome::new(
            json!({"history": entries, "hasMore": has_more, "nextAfter": next_after}),
        ))
    }

    /// Project membership is not worktree ownership, Session selection, or authorization.
    pub fn task_set_project(
        &self,
        reference: &str,
        expected: i64,
        project: Option<&str>,
        confirmed: bool,
        reason: &str,
    ) -> AppResult<Outcome> {
        self.task_update_fields(
            reference, expected, &json!({"project": project}).to_string(),
            confirmed, reason, "task.project_changed",
        )
    }
}
