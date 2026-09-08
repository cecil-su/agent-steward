use std::path::{Path, PathBuf};

use git_adapter::{ExistingPathIdentity, ExistingPathIdentityRecord, RepositoryInfo};
use rusqlite::{Connection, OptionalExtension, Row, params};
use serde_json::json;
use steward_core::{
    ComponentView, ProjectView, SourceRootView, normalize_project_name,
    validate_source_relative_path,
};
use storage_sqlite::now;

use crate::projects::{
    check_project_revision, load_project, record_project_change, resolve_project_id,
};
use crate::{AppError, AppResult, Outcome, Service, warning};

pub enum SourceLocation<'a> {
    Git {
        worktree: &'a Path,
        relative_path: &'a str,
    },
    Directory(&'a Path),
}

struct SourceRecord {
    view: SourceRootView,
    directory_identity: Option<ExistingPathIdentityRecord>,
}
struct RepositoryRecord {
    id: i64,
    identity: ExistingPathIdentityRecord,
}

fn git<T>(result: Result<T, git_adapter::GitError>) -> AppResult<T> {
    result.map_err(|error| AppError::from_git(error, None))
}
fn path_text(path: &Path) -> AppResult<&str> {
    path.to_str()
        .ok_or_else(|| AppError::invalid("path", "expected a UTF-8 path"))
}
fn decode_identity(column: usize, text: &str) -> rusqlite::Result<ExistingPathIdentityRecord> {
    serde_json::from_str(text).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            column,
            rusqlite::types::Type::Text,
            Box::new(error),
        )
    })
}
fn component_from_row(row: &Row<'_>) -> rusqlite::Result<ComponentView> {
    Ok(ComponentView {
        id: row.get(0)?,
        project_id: row.get(1)?,
        name: row.get(2)?,
        created_at: row.get(3)?,
    })
}
fn source_from_row(row: &Row<'_>) -> rusqlite::Result<SourceRecord> {
    let identity: Option<String> = row.get(7)?;
    Ok(SourceRecord {
        view: SourceRootView {
            id: row.get(0)?,
            project_id: row.get(1)?,
            component_id: row.get(2)?,
            repository_id: row.get(3)?,
            relative_path: row.get(4)?,
            directory_path: row.get(5)?,
            created_at: row.get(6)?,
        },
        directory_identity: identity.map(|text| decode_identity(7, &text)).transpose()?,
    })
}
fn load_sources(connection: &Connection, project: Option<i64>) -> AppResult<Vec<SourceRecord>> {
    let mut statement = connection.prepare("SELECT id,project_id,component_id,repository_id,relative_path,directory_path,created_at,directory_identity_json FROM source_roots WHERE (?1 IS NULL OR project_id=?1) ORDER BY id LIMIT 2001").map_err(AppError::from_sqlite)?;
    let sources = statement
        .query_map([project], source_from_row)
        .map_err(AppError::from_sqlite)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(AppError::from_sqlite)?;
    if sources.len() > 2000 {
        return Err(AppError::invalid(
            "sources",
            "source scan exceeds 2000 entries; use an explicit project filter",
        ));
    }
    Ok(sources)
}
fn load_repository(connection: &Connection, id: i64) -> AppResult<RepositoryRecord> {
    let (path, text): (String, String) = connection
        .query_row(
            "SELECT common_dir,common_identity_json FROM repositories WHERE id=?1",
            [id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(AppError::from_sqlite)?
        .ok_or_else(|| AppError::not_found("Repository", &id.to_string()))?;
    let identity = decode_identity(1, &text).map_err(AppError::from_sqlite)?;
    if path_text(&identity.canonical_path)? != path {
        return Err(AppError::constraint("repositories.identity"));
    }
    Ok(RepositoryRecord { id, identity })
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
fn directory_source(path: &Path) -> AppResult<ExistingPathIdentity> {
    let identity = git(git_adapter::identify_existing(path))?;
    if !identity.canonical_path.is_dir() {
        return Err(AppError::invalid(
            "directory",
            "expected an existing directory",
        ));
    }
    if git(git_adapter::directory_has_git_marker(
        &identity.canonical_path,
    ))? {
        return Err(AppError::invalid(
            "directory",
            "Git directories require --repo and --path, not --directory",
        ));
    }
    git(git_adapter::verify_existing_identity(&identity))?;
    Ok(identity)
}
fn resolve_git_path(checkout: &RepositoryInfo, relative: &str) -> AppResult<ExistingPathIdentity> {
    validate_source_relative_path(relative).map_err(|reason| AppError::invalid("path", reason))?;
    let source = git(git_adapter::identify_existing(
        &checkout.repository_path.join(relative),
    ))?;
    if !source.canonical_path.is_dir()
        || !source.canonical_path.starts_with(&checkout.repository_path)
    {
        return Err(AppError::invalid(
            "path",
            "source must remain a directory inside the selected checkout",
        ));
    }
    // Reject symlinks into another repository and nested repositories/submodules.
    let source_checkout = git(git_adapter::checkout_info(&source.canonical_path))?;
    if !source_checkout
        .common_identity()
        .same_object(checkout.common_identity())
    {
        return Err(AppError::invalid(
            "path",
            "source belongs to a different or nested repository",
        ));
    }
    git(git_adapter::verify_repository_identity(checkout))?;
    git(git_adapter::verify_existing_identity(&source))?;
    Ok(source)
}
fn resolve_source(
    source: &SourceRecord,
    repository: Option<&RepositoryRecord>,
    worktree: Option<&Path>,
) -> AppResult<PathBuf> {
    if let Some(repository) = repository {
        let worktree = worktree.ok_or_else(|| {
            AppError::invalid("worktree", "Git sources require an explicit worktree")
        })?;
        let registered = git(git_adapter::observe_recorded_identity(&repository.identity))?;
        let checkout = git(git_adapter::checkout_info(worktree))?;
        if !checkout.common_identity().same_object(&registered) {
            return Err(AppError::invalid(
                "worktree",
                "worktree belongs to a different repository",
            ));
        }
        let relative = source
            .view
            .relative_path
            .as_deref()
            .ok_or_else(|| AppError::constraint("source_roots.location"))?;
        let resolved = resolve_git_path(&checkout, relative)?;
        git(git_adapter::verify_existing_identity(&registered))?;
        Ok(resolved.canonical_path)
    } else {
        if worktree.is_some() {
            return Err(AppError::invalid(
                "worktree",
                "directory-only sources do not accept a worktree",
            ));
        }
        let identity = source
            .directory_identity
            .as_ref()
            .ok_or_else(|| AppError::constraint("source_roots.identity"))?;
        if source.view.directory_path.as_deref() != Some(path_text(&identity.canonical_path)?) {
            return Err(AppError::constraint("source_roots.identity"));
        }
        let identity = git(git_adapter::observe_recorded_identity(identity))?;
        let resolved = directory_source(&identity.canonical_path)?;
        if resolved != identity {
            return Err(AppError::invalid(
                "directory",
                "registered directory was replaced",
            ));
        }
        git(git_adapter::verify_existing_identity(&identity))?;
        Ok(resolved.canonical_path)
    }
}

impl Service {
    /// Set a task's component scope within its current project. Empty names clear the scope.
    pub fn task_set_components(
        &self,
        reference: &str,
        expected: i64,
        names: &[String],
    ) -> AppResult<Outcome> {
        if names.len() > 200 {
            return Err(AppError::invalid("components", "at most 200 components"));
        }
        let mut connection = self.connection()?;
        let tx =
            storage_sqlite::write_transaction(&mut connection).map_err(AppError::from_storage)?;
        let task = crate::db::load_task_by_reference(&tx, reference)?;
        crate::db::check_version(&task, expected)?;
        if task.status == steward_core::TaskStatus::Closed {
            return Err(AppError::constraint("task.closed"));
        }
        let mut ids = std::collections::BTreeSet::new();
        for name in names {
            let project_id = task.project_id.ok_or_else(|| {
                AppError::invalid(
                    "project",
                    "associate the task with a project before selecting components",
                )
            })?;
            let (_, key) = normalize_project_name(name)
                .map_err(|reason| AppError::invalid("component", reason))?;
            let id: i64 = tx
                .query_row(
                    "SELECT id FROM components WHERE project_id=?1 AND name_key=?2",
                    params![project_id, key],
                    |row| row.get(0),
                )
                .optional()
                .map_err(AppError::from_sqlite)?
                .ok_or_else(|| AppError::not_found("Component", name))?;
            if !ids.insert(id) {
                return Err(AppError::invalid(
                    "component",
                    "duplicate component selection",
                ));
            }
        }
        let ids: Vec<i64> = ids.into_iter().collect();
        if ids == task.component_ids {
            tx.commit().map_err(AppError::from_sqlite)?;
            return Ok(Outcome::new(json!({"task":task})));
        }
        tx.execute("DELETE FROM task_components WHERE task_id=?1", [task.id])
            .map_err(AppError::from_sqlite)?;
        for id in &ids {
            tx.execute(
                "INSERT INTO task_components(task_id,project_id,component_id) VALUES (?1,?2,?3)",
                params![task.id, task.project_id, id],
            )
            .map_err(AppError::from_sqlite)?;
        }
        let timestamp = now();
        crate::db::bump_task(&tx, task.id, expected, &timestamp)?;
        crate::db::insert_history(
            &tx,
            task.id,
            "task.components_changed",
            task.current_session_id.as_deref(),
            "task component scope changed",
            json!({"previousComponentIds":task.component_ids,"componentIds":ids}),
            &timestamp,
        )?;
        let updated = crate::db::load_task(&tx, task.id)?;
        tx.commit().map_err(AppError::from_sqlite)?;
        Ok(Outcome::new(json!({"task":updated})))
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

    /// Explicit metadata registration. All Git subprocesses run outside the write transaction.
    pub fn project_source_add(
        &self,
        reference: &str,
        expected: i64,
        component: Option<&str>,
        location: SourceLocation<'_>,
    ) -> AppResult<Outcome> {
        let project = expected_project(&self.connection()?, reference, expected)?;
        let (checkout, relative, directory, observed_source) = match location {
            SourceLocation::Git {
                worktree,
                relative_path,
            } => {
                let relative = validate_source_relative_path(relative_path)
                    .map_err(|reason| AppError::invalid("path", reason))?;
                let checkout = git(git_adapter::checkout_info(worktree))?;
                let observed_source = resolve_git_path(&checkout, &relative)?;
                (Some(checkout), Some(relative), None, observed_source)
            }
            SourceLocation::Directory(path) => {
                let identity = directory_source(path)?;
                (None, None, Some(identity.clone()), identity)
            }
        };
        let mut connection = self.connection()?;
        let tx =
            storage_sqlite::write_transaction(&mut connection).map_err(AppError::from_storage)?;
        // Recheck by stable ID so a concurrent rename cannot redirect a name reference.
        let project = expected_project(&tx, &format!("##{}", project.id), expected)?;
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
        // Only filesystem identity checks while holding SQLite's write lock.
        git(git_adapter::verify_existing_identity(&observed_source))?;
        let repository_id = if let Some(checkout) = &checkout {
            git(git_adapter::verify_existing_identity(
                checkout.common_identity(),
            ))?;
            let records = {
                let mut statement = tx
                    .prepare("SELECT id FROM repositories ORDER BY id LIMIT 2001")
                    .map_err(AppError::from_sqlite)?;
                statement
                    .query_map([], |row| row.get::<_, i64>(0))
                    .map_err(AppError::from_sqlite)?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(AppError::from_sqlite)?
            };
            if records.len() > 2000 {
                return Err(AppError::invalid(
                    "repositories",
                    "repository scan limit exceeded",
                ));
            }
            let repository_count = records.len();
            let mut existing = None;
            for id in records {
                let record = load_repository(&tx, id)?;
                if record.identity.same_object(checkout.common_identity()) {
                    git(git_adapter::observe_recorded_identity(&record.identity))?;
                    existing = Some(record.id);
                    break;
                }
                if record.identity.canonical_path == checkout.common_dir {
                    return Err(AppError::invalid(
                        "repository",
                        "registered common directory was replaced; explicit reconciliation is required",
                    ));
                }
            }
            if let Some(id) = existing {
                Some(id)
            } else {
                if repository_count >= 2000 {
                    return Err(AppError::invalid(
                        "repositories",
                        "at most 2000 repository registrations",
                    ));
                }
                tx.execute("INSERT INTO repositories(common_dir,common_identity_json,created_at) VALUES (?1,?2,?3)", params![path_text(&checkout.common_dir)?,serde_json::to_string(checkout.common_identity()).expect("identity serializes"),now()]).map_err(AppError::from_sqlite)?;
                Some(tx.last_insert_rowid())
            }
        } else {
            None
        };
        let directory_path = directory
            .as_ref()
            .map(|identity| path_text(&identity.canonical_path).map(ToOwned::to_owned))
            .transpose()?;
        tx.execute("INSERT INTO source_roots(project_id,component_id,repository_id,relative_path,directory_path,directory_identity_json,created_at) VALUES (?1,?2,?3,?4,?5,?6,?7)", params![project.id,component_id,repository_id,relative,directory_path,directory.as_ref().map(|identity| serde_json::to_string(identity).expect("identity serializes")),now()]).map_err(AppError::from_sqlite)?;
        let id = tx.last_insert_rowid();
        let source = load_sources(&tx, Some(project.id))?
            .into_iter()
            .find(|source| source.view.id == id)
            .expect("inserted source");
        let project = record_project_change(
            &tx,
            &project,
            "source.added",
            json!({"sourceId":id,"componentId":component_id,"repositoryId":repository_id,"relativePath":relative,"directoryPath":directory_path}),
        )?;
        tx.commit().map_err(AppError::from_sqlite)?;
        let mut result = Outcome::new(json!({"project":project,"source":source.view}));
        // Registration is not a promise of atomic Git/DB state. Surface races after commit.
        let verified = if let Some(checkout) = &checkout {
            resolve_git_path(checkout, relative.as_deref().expect("Git path")).map(|_| ())
        } else {
            directory_source(&observed_source.canonical_path)
                .and_then(|_| git(git_adapter::verify_existing_identity(&observed_source)))
        };
        if verified.is_err() {
            result.warnings.push(warning("SOURCE_OBSERVATION_CHANGED", "Source metadata was saved but its live location could not be reverified; resolve it before use", json!({"sourceId":id})));
        }
        Ok(result)
    }

    pub fn project_sources(&self, reference: &str) -> AppResult<Outcome> {
        let mut connection = self.connection()?;
        let tx = connection.transaction().map_err(AppError::from_sqlite)?;
        let project = load_project(&tx, resolve_project_id(&tx, reference)?)?;
        let sources: Vec<_> = load_sources(&tx, Some(project.id))?
            .into_iter()
            .map(|source| source.view)
            .collect();
        let mut repositories = Vec::new();
        for id in sources
            .iter()
            .filter_map(|source| source.repository_id)
            .collect::<std::collections::BTreeSet<_>>()
        {
            let repository = load_repository(&tx, id)?;
            repositories.push(json!({"id":id,"commonDir":repository.identity.canonical_path}));
        }
        tx.commit().map_err(AppError::from_sqlite)?;
        Ok(Outcome::new(
            json!({"project":project,"sources":sources,"repositories":repositories,"liveStateObserved":false}),
        ))
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

    /// HTTP callers choose only an advertised local checkout, never an arbitrary path.
    /// Validate the request lexically before any IO; list candidates from trusted registration
    /// metadata without canonicalizing the request or unrelated candidate paths.
    pub fn project_source_http_worktree(
        &self,
        reference: &str,
        source_id: i64,
        requested: &Path,
    ) -> AppResult<PathBuf> {
        let requested = git_adapter::local_worktree_path(requested)
            .map_err(|_| AppError::invalid("worktree", "expected a local absolute checkout path without traversal or device/network prefixes"))?;
        let mut connection = self.connection()?;
        let tx = connection.transaction().map_err(AppError::from_sqlite)?;
        let project = resolve_project_id(&tx, reference)?;
        let source = load_sources(&tx, Some(project))?
            .into_iter()
            .find(|s| s.view.id == source_id)
            .ok_or_else(|| AppError::not_found("SourceRoot", &source_id.to_string()))?;
        let repository = load_repository(
            &tx,
            source.view.repository_id.ok_or_else(|| {
                AppError::invalid(
                    "worktree",
                    "directory-only sources do not accept a worktree",
                )
            })?,
        )?;
        tx.commit().map_err(AppError::from_sqlite)?;
        git_adapter::local_worktree_path(&repository.identity.canonical_path).map_err(|_| {
            AppError::invalid(
                "worktree",
                "HTTP checkout selection requires a registered local repository",
            )
        })?;
        let registered = git(git_adapter::observe_recorded_identity(&repository.identity))?;
        let selected =
            git_adapter::registered_worktree_path(&registered.canonical_path, &requested).map_err(
                |error| match error {
                    git_adapter::GitError::PathIdentity(_) => AppError::invalid(
                        "worktree",
                        "select an advertised checkout of the registered repository",
                    ),
                    error => AppError::from_git(error, None),
                },
            )?;
        git(git_adapter::verify_existing_identity(&registered))?;
        Ok(selected)
    }

    pub fn project_source_resolve(
        &self,
        reference: &str,
        source_id: i64,
        worktree: Option<&Path>,
    ) -> AppResult<Outcome> {
        let mut connection = self.connection()?;
        let tx = connection.transaction().map_err(AppError::from_sqlite)?;
        let project = load_project(&tx, resolve_project_id(&tx, reference)?)?;
        let source = load_sources(&tx, Some(project.id))?
            .into_iter()
            .find(|source| source.view.id == source_id)
            .ok_or_else(|| AppError::not_found("SourceRoot", &source_id.to_string()))?;
        let repository = source
            .view
            .repository_id
            .map(|id| load_repository(&tx, id))
            .transpose()?;
        tx.commit().map_err(AppError::from_sqlite)?;
        let path = resolve_source(&source, repository.as_ref(), worktree)?;
        Ok(Outcome::new(
            json!({"project":project,"source":source.view,"resolvedPath":path,"observedAt":now()}),
        ))
    }

    /// Return all matching candidates, never select/claim a task or persist an inferred link.
    pub fn project_here(&self, directory: &Path, reference: Option<&str>) -> AppResult<Outcome> {
        let current = git(git_adapter::identify_existing(directory))?;
        if !current.canonical_path.is_dir() {
            return Err(AppError::invalid("directory", "expected a directory"));
        }
        let checkout = if git(git_adapter::directory_has_git_marker(
            &current.canonical_path,
        ))? {
            Some(git(git_adapter::checkout_info(&current.canonical_path))?)
        } else {
            None
        };
        let mut connection = self.connection()?;
        let tx = connection.transaction().map_err(AppError::from_sqlite)?;
        let project_id = reference
            .map(|reference| resolve_project_id(&tx, reference))
            .transpose()?;
        let sources = load_sources(&tx, project_id)?;
        let mut entries = Vec::new();
        for source in sources {
            let repository = source
                .view
                .repository_id
                .map(|id| load_repository(&tx, id))
                .transpose()?;
            let project = load_project(&tx, source.view.project_id)?;
            entries.push((project, source, repository));
        }
        tx.commit().map_err(AppError::from_sqlite)?;
        let mut candidates = Vec::new();
        let mut warnings = Vec::new();
        for (project, source, repository) in entries {
            let worktree = match (&checkout, &repository) {
                (Some(checkout), Some(repository))
                    if repository.identity.same_object(checkout.common_identity())
                        || checkout.common_dir == repository.identity.canonical_path =>
                {
                    Some(checkout.repository_path.as_path())
                }
                (None, None) => None,
                _ => continue,
            };
            match resolve_source(&source, repository.as_ref(), worktree) {
                Ok(path) => {
                    let matched_by = if current.canonical_path.starts_with(&path) { "source" }
                        else if repository.is_some() { "repository" }
                        else if path.starts_with(&current.canonical_path) { "ancestor" }
                        else { continue };
                    candidates.push(json!({"project":project,"source":source.view,"matchedBy":matched_by,"resolvedPath":path}));
                }
                Err(error) => warnings.push(warning("SOURCE_UNAVAILABLE", "A registered source could not be verified; it was not used as a candidate", json!({"projectId":project.id,"sourceId":source.view.id,"reasonCode":error.body.code}))),
            }
        }
        git(git_adapter::verify_existing_identity(&current))?;
        if let Some(checkout) = &checkout {
            git(git_adapter::verify_repository_identity(checkout))?;
        } else if git(git_adapter::directory_has_git_marker(
            &current.canonical_path,
        ))? {
            return Err(AppError::invalid(
                "directory",
                "Git identity changed during candidate lookup",
            ));
        }
        candidates.sort_by_key(|candidate| {
            (
                candidate["matchedBy"] != "source",
                std::cmp::Reverse(if candidate["matchedBy"] == "source" {
                    candidate["resolvedPath"]
                        .as_str()
                        .map(|path| Path::new(path).components().count())
                        .unwrap_or(0)
                } else {
                    0
                }),
                candidate["project"]["id"].as_i64(),
                candidate["source"]["id"].as_i64(),
            )
        });
        Ok(Outcome {
            data: json!({"directory":current.canonical_path,"candidates":candidates,"observedAt":now(),"selectedProjectId":null}),
            warnings,
        })
    }
}
