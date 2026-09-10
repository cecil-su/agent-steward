//! Personal rules, not execution authority. No model, chat scanning, or secret detection.
use crate::{AppError, AppResult, Outcome, Service};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use storage_sqlite::now;

pub const MAX_RULE_BYTES: usize = 65536;
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuleSource {
    pub kind: String,
    pub evidence: String,
    pub task_id: Option<i64>,
    pub task_version: Option<i64>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuleContent {
    pub name: String,
    pub body: String,
    pub sources: Vec<RuleSource>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuleInput {
    pub scope: String,
    pub project_id: Option<i64>,
    pub status: String,
    pub content_version: i64,
    pub content: RuleContent,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuleView {
    pub id: i64,
    pub scope: String,
    pub project_id: Option<i64>,
    pub status: String,
    pub revision: i64,
    pub content_version: i64,
    pub content: RuleContent,
    pub created_at: String,
    pub updated_at: String,
}
fn text(field: &str, value: &str, max: usize) -> AppResult<()> {
    if value.trim().is_empty() || value.contains('\0') || value.len() > max {
        return Err(AppError::invalid(
            field,
            format!("nonblank text without NUL, at most {max} UTF-8 bytes required"),
        ));
    }
    Ok(())
}
fn validate(input: &RuleInput) -> AppResult<()> {
    if input.content_version != 1 {
        return Err(AppError::invalid(
            "contentVersion",
            "only version 1 is supported",
        ));
    }
    if !matches!(input.status.as_str(), "candidate" | "active" | "disabled") {
        return Err(AppError::invalid(
            "status",
            "expected candidate, active or disabled",
        ));
    }
    if !matches!(
        (input.scope.as_str(), input.project_id),
        ("global", None) | ("project", Some(1..))
    ) {
        return Err(AppError::invalid(
            "scope",
            "global requires null projectId; project requires positive projectId",
        ));
    }
    text("name", &input.content.name, 300)?;
    text("body", &input.content.body, 48000)?;
    if input.content.sources.len() > 32 {
        return Err(AppError::invalid("sources", "at most 32 sources"));
    }
    for (index, source) in input.content.sources.iter().enumerate() {
        if !matches!(source.kind.as_str(), "explicit" | "inferred") {
            return Err(AppError::invalid(
                "sources.kind",
                "expected explicit or inferred",
            ));
        }
        text("sources.evidence", &source.evidence, 4000)?;
        if !matches!(
            (source.task_id, source.task_version),
            (None, None) | (Some(1..), Some(1..))
        ) {
            return Err(AppError::invalid(
                "sources",
                "taskId/taskVersion must both be null or positive",
            ));
        }
        if input.content.sources[..index].contains(source) {
            return Err(AppError::invalid("sources", "duplicate source"));
        }
    }
    if serde_json::to_vec(&input.content)
        .map_err(|_| AppError::invalid("content", "cannot encode"))?
        .len()
        > MAX_RULE_BYTES
    {
        return Err(AppError::invalid(
            "content",
            "exceeds 65536 UTF-8 JSON bytes",
        ));
    }
    Ok(())
}
fn stored_error() -> AppError {
    AppError::new(
        "DATABASE_UNAVAILABLE",
        "stored rule is invalid or unsupported",
        false,
        json!({"entityType":"rule"}),
        10,
    )
}
fn from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<(RuleView, String)> {
    Ok((
        RuleView {
            id: row.get(0)?,
            scope: row.get(1)?,
            project_id: row.get(2)?,
            status: row.get(3)?,
            revision: row.get(4)?,
            content_version: row.get(5)?,
            content: RuleContent {
                name: String::new(),
                body: String::new(),
                sources: vec![],
            },
            created_at: row.get(7)?,
            updated_at: row.get(8)?,
        },
        row.get(6)?,
    ))
}
fn validate_view(view: &RuleView) -> AppResult<()> {
    validate(&RuleInput {
        scope: view.scope.clone(),
        project_id: view.project_id,
        status: view.status.clone(),
        content_version: view.content_version,
        content: view.content.clone(),
    })
    .map_err(|_| stored_error())?;
    if view.id < 1 || view.revision < 1 {
        return Err(stored_error());
    }
    Ok(())
}
fn decode((mut view, raw): (RuleView, String)) -> AppResult<RuleView> {
    if raw.len() > MAX_RULE_BYTES {
        return Err(stored_error());
    }
    view.content = serde_json::from_str(&raw).map_err(|_| stored_error())?;
    validate_view(&view)?;
    Ok(view)
}
const SELECT: &str = "SELECT id,scope,project_id,status,revision,content_version,content_json,created_at,updated_at FROM rules";
fn load(c: &Connection, id: i64) -> AppResult<RuleView> {
    let raw = c
        .query_row(&format!("{SELECT} WHERE id=?1"), [id], from_row)
        .optional()
        .map_err(AppError::from_sqlite)?
        .ok_or_else(|| AppError::not_found("rule", &id.to_string()))?;
    decode(raw)
}
pub(crate) fn session_rules(c: &Connection, project: Option<i64>) -> AppResult<Value> {
    let mut statement = c
        .prepare(&format!(
            "{SELECT} WHERE status='active' AND (scope='global' OR project_id=?1) ORDER BY scope,id"
        ))
        .map_err(AppError::from_sqlite)?;
    let rules = statement
        .query_map([project], from_row)
        .map_err(AppError::from_sqlite)?
        .map(|r| decode(r.map_err(AppError::from_sqlite)?))
        .collect::<AppResult<Vec<_>>>()?;
    Ok(json!({"formatVersion":1,"rules":rules}))
}
impl Service {
    pub fn rule_show(&self, id: i64) -> AppResult<Outcome> {
        Ok(Outcome::new(json!({"rule":load(&self.connection()?,id)?})))
    }
    /// All states, optionally filtered by exact scope/project. No implicit active-only hiding.
    pub fn rule_list(
        &self,
        scope: Option<&str>,
        project: Option<i64>,
        status: Option<&str>,
    ) -> AppResult<Outcome> {
        if scope.is_some_and(|v| !matches!(v, "global" | "project"))
            || status.is_some_and(|v| !matches!(v, "candidate" | "active" | "disabled"))
            || project.is_some_and(|v| v < 1)
        {
            return Err(AppError::invalid("filter", "invalid rule filter"));
        }
        let c = self.connection()?;
        let mut s = c.prepare(&format!("{SELECT} WHERE (?1 IS NULL OR scope=?1) AND (?2 IS NULL OR project_id=?2) AND (?3 IS NULL OR status=?3) ORDER BY id")).map_err(AppError::from_sqlite)?;
        let rules = s
            .query_map(params![scope, project, status], from_row)
            .map_err(AppError::from_sqlite)?
            .map(|r| decode(r.map_err(AppError::from_sqlite)?))
            .collect::<AppResult<Vec<_>>>()?;
        Ok(Outcome::new(json!({"rules":rules})))
    }
    pub fn rule_create(&self, input: RuleInput, reason: &str) -> AppResult<Outcome> {
        self.rule_write(None, Some(input), reason)
    }
    pub fn rule_update(
        &self,
        id: i64,
        revision: i64,
        input: RuleInput,
        reason: &str,
    ) -> AppResult<Outcome> {
        self.rule_write(Some((id, revision)), Some(input), reason)
    }
    pub fn rule_disable(&self, id: i64, revision: i64, reason: &str) -> AppResult<Outcome> {
        self.rule_write(Some((id, revision)), None, reason)
    }
    fn rule_write(
        &self,
        target: Option<(i64, i64)>,
        input: Option<RuleInput>,
        reason: &str,
    ) -> AppResult<Outcome> {
        text("reason", reason, 4000)?;
        if let Some(input) = &input {
            validate(input)?;
        }
        let mut c = self.connection()?;
        let tx = storage_sqlite::write_transaction(&mut c).map_err(AppError::from_storage)?;
        let before = target.map(|(id, _)| load(&tx, id)).transpose()?;
        if let Some((_, expected)) = target {
            let current = before.as_ref().unwrap();
            if expected != current.revision {
                return Err(AppError::new(
                    "VERSION_CONFLICT",
                    "rule revision does not match; re-read and reconsider",
                    true,
                    json!({"ruleId":current.id,"expectedRevision":expected,"currentRevision":current.revision,"rule":current}),
                    4,
                ));
            }
        }
        let disabling = input.is_none();
        let input = input.unwrap_or_else(|| {
            let b = before.as_ref().unwrap();
            RuleInput {
                scope: b.scope.clone(),
                project_id: b.project_id,
                status: "disabled".into(),
                content_version: b.content_version,
                content: b.content.clone(),
            }
        });
        if let Some(id) = input.project_id {
            crate::projects::load_project(&tx, id)?;
        }
        for source in &input.content.sources {
            if let Some(id) = source.task_id {
                let task = crate::db::load_task(&tx, id)?;
                // A historical reference, not CAS against a source task's current version.
                if source.task_version.unwrap() > task.version {
                    return Err(AppError::invalid(
                        "sources.taskVersion",
                        "cannot reference a future task version",
                    ));
                }
            }
        }
        if let Some(b) = &before
            && b.scope == input.scope
            && b.project_id == input.project_id
            && b.status == input.status
            && b.content == input.content
        {
            return Err(AppError::invalid("input", "no rule change"));
        }
        let at = now();
        let revision = before
            .as_ref()
            .map_or(Some(1), |b| b.revision.checked_add(1))
            .ok_or_else(|| AppError::constraint("rules.revision"))?;
        let content = serde_json::to_string(&input.content)
            .map_err(|_| AppError::invalid("content", "cannot encode"))?;
        let id = if let Some((id, expected)) = target {
            let changed=tx.execute("UPDATE rules SET scope=?1,project_id=?2,status=?3,revision=?4,content_version=?5,content_json=?6,updated_at=?7 WHERE id=?8 AND revision=?9",params![input.scope,input.project_id,input.status,revision,input.content_version,content,at,id,expected]).map_err(AppError::from_sqlite)?;
            if changed != 1 {
                return Err(AppError::constraint("rules.revision"));
            }
            id
        } else {
            tx.execute("INSERT INTO rules(scope,project_id,status,revision,content_version,content_json,created_at,updated_at) VALUES(?1,?2,?3,1,?4,?5,?6,?6)",params![input.scope,input.project_id,input.status,input.content_version,content,at]).map_err(AppError::from_sqlite)?;
            tx.last_insert_rowid()
        };
        let after = load(&tx, id)?;
        let operation = if target.is_none() {
            "rule.created"
        } else if disabling {
            "rule.disabled"
        } else {
            "rule.updated"
        };
        tx.execute("INSERT INTO rule_history(rule_id,revision,operation,reason,before_json,after_json,occurred_at) VALUES(?1,?2,?3,?4,?5,?6,?7)",params![id,revision,operation,reason,before.as_ref().map(|b|json!(b).to_string()),json!(after).to_string(),at]).map_err(AppError::from_sqlite)?;
        tx.commit().map_err(AppError::from_sqlite)?;
        Ok(Outcome::new(json!({"rule":after})))
    }
    pub fn rule_history(&self, id: i64) -> AppResult<Outcome> {
        let mut c = self.connection()?;
        let tx = c.transaction().map_err(AppError::from_sqlite)?;
        let rule = load(&tx, id)?;
        let history = {
            let mut s=tx.prepare("SELECT revision,operation,reason,before_json,after_json,occurred_at FROM rule_history WHERE rule_id=?1 ORDER BY revision").map_err(AppError::from_sqlite)?;
            let rows = s
                .query_map([id], |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, Option<String>>(3)?,
                        r.get::<_, String>(4)?,
                        r.get::<_, String>(5)?,
                    ))
                })
                .map_err(AppError::from_sqlite)?;
            rows.map(|r| {
                let (revision, operation, reason, before, after, at) =
                    r.map_err(AppError::from_sqlite)?;
                let before = before
                    .map(|v| serde_json::from_str::<RuleView>(&v).map_err(|_| stored_error()))
                    .transpose()?;
                let after: RuleView = serde_json::from_str(&after).map_err(|_| stored_error())?;
                validate_view(&after)?;
                if let Some(before) = &before {
                    validate_view(before)?;
                }
                if after.id != id
                    || after.revision != revision
                    || before
                        .as_ref()
                        .is_some_and(|b| b.id != id || b.revision != revision - 1)
                {
                    return Err(stored_error());
                }
                Ok(
                    json!({"ruleId":id,"revision":revision,"operation":operation,"reason":reason,
                    "before":before,"after":after,"occurredAt":at}),
                )
            })
            .collect::<AppResult<Vec<_>>>()?
        };
        tx.commit().map_err(AppError::from_sqlite)?;
        Ok(Outcome::new(json!({"rule":rule,"history":history})))
    }
}
