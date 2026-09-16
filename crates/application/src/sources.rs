use std::path::Path;

use rusqlite::{Connection, OptionalExtension, Row, params};
use serde_json::json;
use steward_core::{
    ComponentView, ProjectView, SourceRootView, normalize_project_name, validate_source_directory,
};
use storage_sqlite::now;

use crate::projects::{
    check_project_revision, load_project, record_project_change, resolve_project_id,
};
use crate::{AppError, AppResult, Outcome, Service};

/// Source locations are user-supplied records, not permission to observe the filesystem.
pub enum SourceLocation<'a> {
    Directory(&'a Path),
}

fn component_from_row(row: &Row<'_>) -> rusqlite::Result<ComponentView> {
    Ok(ComponentView {
        id: row.get(0)?,
        project_id: row.get(1)?,
        name: row.get(2)?,
        created_at: row.get(3)?,
    })
}
fn source_from_row(row: &Row<'_>) -> rusqlite::Result<SourceRootView> {
    Ok(SourceRootView {
        id: row.get(0)?,
        project_id: row.get(1)?,
        component_id: row.get(2)?,
        directory_path: row.get(3)?,
        created_at: row.get(4)?,
    })
}
fn expected_project(
    connection: &Connection,
    reference: &str,
    expected: i64,
) -> AppResult<ProjectView> {
    let project = load_project(connection, resolve_project_id(connection, reference)?)?;
    check_project_revision(&project, expected)?;
    Ok(project)
}

impl Service {
    pub fn task_set_components(
        &self,
        reference: &str,
        expected: i64,
        names: &[String],
        confirmed: bool,
        reason: &str,
    ) -> AppResult<Outcome> {
        self.task_update_fields(
            reference,
            expected,
            &json!({"components":names}).to_string(),
            confirmed,
            reason,
            "task.components_changed",
        )
    }

    pub fn project_component_add(
        &self,
        reference: &str,
        expected: i64,
        name: &str,
    ) -> AppResult<Outcome> {
        let (name, key) = normalize_project_name(name)
            .map_err(|reason| AppError::invalid("component", reason))?;
        let mut connection = self.connection()?;
        let tx =
            storage_sqlite::write_transaction(&mut connection).map_err(AppError::from_storage)?;
        let project = expected_project(&tx, reference, expected)?;
        let count: i64 = tx
            .query_row(
                "SELECT count(*) FROM components WHERE project_id=?1",
                [project.id],
                |row| row.get(0),
            )
            .map_err(AppError::from_sqlite)?;
        if count >= 200 {
            return Err(AppError::invalid(
                "components",
                "at most 200 components per project",
            ));
        }
        let timestamp = now();
        tx.execute(
            "INSERT INTO components(project_id,name,name_key,created_at) VALUES (?1,?2,?3,?4)",
            params![project.id, name, key, timestamp],
        )
        .map_err(AppError::from_sqlite)?;
        let component = ComponentView {
            id: tx.last_insert_rowid(),
            project_id: project.id,
            name,
            created_at: timestamp,
        };
        let project = record_project_change(
            &tx,
            &project,
            "component.created",
            json!({"componentId":component.id,"name":component.name}),
        )?;
        tx.commit().map_err(AppError::from_sqlite)?;
        Ok(Outcome::new(
            json!({"project":project,"component":component}),
        ))
    }

    pub fn project_components(&self, reference: &str) -> AppResult<Outcome> {
        let mut connection = self.connection()?;
        let tx = connection.transaction().map_err(AppError::from_sqlite)?;
        let project = load_project(&tx, resolve_project_id(&tx, reference)?)?;
        let components = {
            let mut statement = tx.prepare("SELECT id,project_id,name,created_at FROM components WHERE project_id=?1 ORDER BY id LIMIT 201").map_err(AppError::from_sqlite)?;
            statement
                .query_map([project.id], component_from_row)
                .map_err(AppError::from_sqlite)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(AppError::from_sqlite)?
        };
        if components.len() > 200 {
            return Err(AppError::invalid("components", "component limit exceeded"));
        }
        tx.commit().map_err(AppError::from_sqlite)?;
        Ok(Outcome::new(
            json!({"project":project,"components":components}),
        ))
    }

    /// Records path text without existence checks, canonicalization, Git, or file reads.
    pub fn project_source_add(
        &self,
        reference: &str,
        expected: i64,
        component: Option<&str>,
        location: SourceLocation<'_>,
    ) -> AppResult<Outcome> {
        let SourceLocation::Directory(path) = location;
        let text = path
            .to_str()
            .ok_or_else(|| AppError::invalid("directory", "expected UTF-8 path text"))?;
        let directory = validate_source_directory(text)
            .map_err(|reason| AppError::invalid("directory", reason))?;
        let mut connection = self.connection()?;
        let tx =
            storage_sqlite::write_transaction(&mut connection).map_err(AppError::from_storage)?;
        let project = expected_project(&tx, reference, expected)?;
        let component_id = component
            .map(|name| -> AppResult<i64> {
                let (_, key) = normalize_project_name(name)
                    .map_err(|reason| AppError::invalid("component", reason))?;
                tx.query_row(
                    "SELECT id FROM components WHERE project_id=?1 AND name_key=?2",
                    params![project.id, key],
                    |row| row.get(0),
                )
                .optional()
                .map_err(AppError::from_sqlite)?
                .ok_or_else(|| AppError::not_found("Component", name))
            })
            .transpose()?;
        let count: i64 = tx
            .query_row(
                "SELECT count(*) FROM source_roots WHERE project_id=?1",
                [project.id],
                |row| row.get(0),
            )
            .map_err(AppError::from_sqlite)?;
        if count >= 200 {
            return Err(AppError::invalid(
                "sources",
                "at most 200 source roots per project",
            ));
        }
        let timestamp = now();
        tx.execute("INSERT INTO source_roots(project_id,component_id,directory_path,created_at) VALUES (?1,?2,?3,?4)", params![project.id,component_id,directory,timestamp]).map_err(AppError::from_sqlite)?;
        let source = SourceRootView {
            id: tx.last_insert_rowid(),
            project_id: project.id,
            component_id,
            directory_path: directory,
            created_at: timestamp,
        };
        let project = record_project_change(
            &tx,
            &project,
            "source.added",
            json!({"sourceId":source.id,"componentId":component_id,"directoryPath":source.directory_path}),
        )?;
        tx.commit().map_err(AppError::from_sqlite)?;
        Ok(Outcome::new(json!({"project":project,"source":source})))
    }

    pub fn project_sources(&self, reference: &str) -> AppResult<Outcome> {
        let mut connection = self.connection()?;
        let tx = connection.transaction().map_err(AppError::from_sqlite)?;
        let project = load_project(&tx, resolve_project_id(&tx, reference)?)?;
        let sources = {
            let mut statement = tx.prepare("SELECT id,project_id,component_id,directory_path,created_at FROM source_roots WHERE project_id=?1 ORDER BY id LIMIT 201").map_err(AppError::from_sqlite)?;
            statement
                .query_map([project.id], source_from_row)
                .map_err(AppError::from_sqlite)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(AppError::from_sqlite)?
        };
        if sources.len() > 200 {
            return Err(AppError::invalid("sources", "source limit exceeded"));
        }
        tx.commit().map_err(AppError::from_sqlite)?;
        Ok(Outcome::new(json!({"project":project,"sources":sources})))
    }

    /// Read an explicit source's saved metadata, never its contents or live state.
    pub fn project_source(&self, reference: &str, source_id: i64) -> AppResult<Outcome> {
        let mut connection = self.connection()?;
        let tx = connection.transaction().map_err(AppError::from_sqlite)?;
        let project = load_project(&tx, resolve_project_id(&tx, reference)?)?;
        let source = tx.query_row("SELECT id,project_id,component_id,directory_path,created_at FROM source_roots WHERE project_id=?1 AND id=?2", params![project.id,source_id], source_from_row)
            .optional().map_err(AppError::from_sqlite)?.ok_or_else(|| AppError::not_found("SourceRoot", &source_id.to_string()))?;
        tx.commit().map_err(AppError::from_sqlite)?;
        Ok(Outcome::new(json!({"project":project,"source":source})))
    }

    pub fn project_source_remove(
        &self,
        reference: &str,
        expected: i64,
        source_id: i64,
    ) -> AppResult<Outcome> {
        let mut connection = self.connection()?;
        let tx =
            storage_sqlite::write_transaction(&mut connection).map_err(AppError::from_storage)?;
        let project = expected_project(&tx, reference, expected)?;
        let count = tx
            .execute(
                "DELETE FROM source_roots WHERE id=?1 AND project_id=?2",
                params![source_id, project.id],
            )
            .map_err(AppError::from_sqlite)?;
        if count != 1 {
            return Err(AppError::not_found("SourceRoot", &source_id.to_string()));
        }
        let project = record_project_change(
            &tx,
            &project,
            "source.removed",
            json!({"sourceId":source_id}),
        )?;
        tx.commit().map_err(AppError::from_sqlite)?;
        Ok(Outcome::new(
            json!({"project":project,"removedSourceId":source_id}),
        ))
    }
}
