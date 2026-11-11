//! Task-local requirements and user decisions. The latest typed snapshot is stored in
//! the existing transactional History, not inferred from prose Notes or Checkpoints.
use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::json;
use storage_sqlite::now;

use crate::db::{bump_task, check_version, insert_history, load_task, load_task_by_reference};
use crate::{AppError, AppResult, Outcome, Service};

const EVENT: &str = "task.work_updated";
const MAX_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RequirementStatus {
    Todo,
    InProgress,
    Blocked,
    Paused,
    Done,
    Cancelled,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ConfirmationStatus {
    Open,
    Resolved,
    Withdrawn,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Requirement {
    pub id: String,
    pub title: String,
    pub acceptance_criteria: String,
    pub status: RequirementStatus,
    pub result: String,
    pub next_step: String,
    pub evidence: Vec<String>,
    pub updated_at: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Confirmation {
    pub id: String,
    pub requirement_ids: Vec<String>,
    pub question: String,
    pub recommendation: String,
    pub impact: String,
    pub status: ConfirmationStatus,
    pub answer: String,
    pub evidence: Vec<String>,
    pub updated_at: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskWork {
    pub format_version: u32,
    pub requirements: Vec<Requirement>,
    pub confirmations: Vec<Confirmation>,
}
impl Default for TaskWork {
    fn default() -> Self {
        Self {
            format_version: 1,
            requirements: vec![],
            confirmations: vec![],
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequirementPatch {
    pub id: String,
    pub title: Option<String>,
    pub acceptance_criteria: Option<String>,
    pub status: Option<RequirementStatus>,
    pub result: Option<String>,
    pub next_step: Option<String>,
    pub evidence: Option<Vec<String>>,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConfirmationPatch {
    pub id: String,
    pub requirement_ids: Option<Vec<String>>,
    pub question: Option<String>,
    pub recommendation: Option<String>,
    pub impact: Option<String>,
    pub status: Option<ConfirmationStatus>,
    pub answer: Option<String>,
    pub evidence: Option<Vec<String>>,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkPatch {
    pub reason: String,
    #[serde(default)]
    pub requirements: Vec<RequirementPatch>,
    #[serde(default)]
    pub confirmations: Vec<ConfirmationPatch>,
}

fn bounded(field: &str, value: &str, required: bool, max: usize) -> AppResult<()> {
    if (required && value.trim().is_empty()) || value.chars().count() > max {
        return Err(AppError::invalid(
            field,
            format!(
                "must be {} and at most {max} characters",
                if required { "non-empty" } else { "a string" }
            ),
        ));
    }
    Ok(())
}
fn id(value: &str) -> AppResult<()> {
    if value.is_empty()
        || value.len() > 64
        || !value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
    {
        return Err(AppError::invalid(
            "id",
            "use 1-64 ASCII letters, digits, hyphens or underscores",
        ));
    }
    Ok(())
}
fn evidence(values: &[String]) -> AppResult<()> {
    if values.len() > 20 {
        return Err(AppError::invalid("evidence", "at most 20 entries"));
    }
    for value in values {
        bounded("evidence", value, true, 2000)?;
    }
    Ok(())
}
fn unique<'a>(values: impl IntoIterator<Item = &'a str>, field: &str) -> AppResult<()> {
    let mut seen = std::collections::HashSet::new();
    for value in values {
        id(value)?;
        if !seen.insert(value) {
            return Err(AppError::invalid(field, "duplicate identifier"));
        }
    }
    Ok(())
}
fn validate(work: &TaskWork) -> AppResult<()> {
    if work.format_version != 1 {
        return Err(AppError::invalid(
            "formatVersion",
            "unsupported work format",
        ));
    }
    if work.requirements.len() > 100 || work.confirmations.len() > 100 {
        return Err(AppError::invalid(
            "work",
            "at most 100 requirements and 100 confirmations per task",
        ));
    }
    unique(
        work.requirements.iter().map(|r| r.id.as_str()),
        "requirements",
    )?;
    unique(
        work.confirmations.iter().map(|c| c.id.as_str()),
        "confirmations",
    )?;
    for r in &work.requirements {
        bounded("title", &r.title, true, 300)?;
        bounded("acceptanceCriteria", &r.acceptance_criteria, true, 4000)?;
        bounded("result", &r.result, false, 4000)?;
        bounded("nextStep", &r.next_step, false, 2000)?;
        evidence(&r.evidence)?;
        if r.status == RequirementStatus::Done
            && (r.result.trim().is_empty() || r.evidence.is_empty())
        {
            return Err(AppError::invalid(
                "requirements",
                "done requires a result and a factual evidence reference",
            ));
        }
        if r.status == RequirementStatus::Blocked && r.next_step.trim().is_empty() {
            return Err(AppError::invalid(
                "nextStep",
                "blocked requirements need a concrete blocker/recovery step",
            ));
        }
    }
    for c in &work.confirmations {
        if c.requirement_ids.is_empty() {
            return Err(AppError::invalid(
                "requirementIds",
                "link at least one requirement",
            ));
        }
        unique(
            c.requirement_ids.iter().map(String::as_str),
            "requirementIds",
        )?;
        if c.requirement_ids
            .iter()
            .any(|id| !work.requirements.iter().any(|r| &r.id == id))
        {
            return Err(AppError::invalid(
                "requirementIds",
                "reference a requirement in this task",
            ));
        }
        for (field, value) in [
            ("question", &c.question),
            ("recommendation", &c.recommendation),
            ("impact", &c.impact),
        ] {
            bounded(field, value, true, 4000)?;
        }
        bounded("answer", &c.answer, false, 4000)?;
        evidence(&c.evidence)?;
        if c.status != ConfirmationStatus::Open
            && (c.answer.trim().is_empty() || c.evidence.is_empty())
        {
            return Err(AppError::invalid(
                "confirmations",
                "closing requires the answer/reason and its evidence",
            ));
        }
    }
    if serde_json::to_vec(work)
        .map_err(|_| AppError::invalid("work", "cannot encode work"))?
        .len()
        > MAX_BYTES
    {
        return Err(AppError::invalid("work", "snapshot exceeds 256 KiB"));
    }
    Ok(())
}

pub(crate) fn load_work(connection: &Connection, task_id: i64) -> AppResult<Option<TaskWork>> {
    let raw: Option<String> = connection.query_row(
        "SELECT payload_json FROM history WHERE task_id=?1 AND change_type=?2 ORDER BY sequence DESC LIMIT 1",
        rusqlite::params![task_id, EVENT], |row| row.get(0),
    ).optional().map_err(AppError::from_sqlite)?;
    raw.map(|raw| {
        let invalid = || {
            AppError::new(
                "DATABASE_UNAVAILABLE",
                "stored task work is invalid or unsupported",
                false,
                json!({"taskId": task_id}),
                10,
            )
        };
        if raw.len() > MAX_BYTES * 2 {
            return Err(invalid());
        }
        let payload: serde_json::Value = serde_json::from_str(&raw).map_err(|_| invalid())?;
        let work: TaskWork =
            serde_json::from_value(payload["work"].clone()).map_err(|_| invalid())?;
        validate(&work).map_err(|_| invalid())?;
        Ok(work)
    })
    .transpose()
}

impl Service {
    pub fn task_work(&self, reference: &str) -> AppResult<Outcome> {
        let mut connection = self.context_connection()?;
        let tx = connection.transaction().map_err(AppError::from_sqlite)?;
        let task = load_task_by_reference(&tx, reference)?;
        let work = load_work(&tx, task.id)?;
        tx.commit().map_err(AppError::from_sqlite)?;
        Ok(Outcome::new(json!({"task": task, "work": work})))
    }

    pub fn task_work_update(
        &self,
        reference: &str,
        expected: i64,
        input: &str,
    ) -> AppResult<Outcome> {
        if input.len() > MAX_BYTES {
            return Err(AppError::invalid("input", "patch exceeds 256 KiB"));
        }
        let patch: WorkPatch = serde_json::from_str(input).map_err(|_| {
            AppError::invalid("input", "invalid requirement/confirmation patch; check field names, types and status values")
        })?;
        bounded("reason", &patch.reason, true, 4000)?;
        if patch.requirements.is_empty() && patch.confirmations.is_empty() {
            return Err(AppError::invalid(
                "input",
                "provide at least one requirement or confirmation patch",
            ));
        }
        unique(
            patch.requirements.iter().map(|r| r.id.as_str()),
            "requirements",
        )?;
        unique(
            patch.confirmations.iter().map(|c| c.id.as_str()),
            "confirmations",
        )?;
        let timestamp = now();
        let mut connection = self.connection()?;
        let tx =
            storage_sqlite::write_transaction(&mut connection).map_err(AppError::from_storage)?;
        let task = load_task_by_reference(&tx, reference)?;
        check_version(&task, expected)?;
        let before = load_work(&tx, task.id)?;
        let mut work = before.clone().unwrap_or_default();
        let mut changed_requirements = Vec::new();
        let mut changed_confirmations = Vec::new();
        for p in patch.requirements {
            let index = work.requirements.iter().position(|r| r.id == p.id);
            let mut r = index
                .map(|i| work.requirements[i].clone())
                .unwrap_or(Requirement {
                    id: p.id,
                    title: String::new(),
                    acceptance_criteria: String::new(),
                    status: RequirementStatus::Todo,
                    result: String::new(),
                    next_step: String::new(),
                    evidence: vec![],
                    updated_at: timestamp.clone(),
                });
            let old = r.clone();
            if let Some(v) = p.title {
                r.title = v;
            }
            if let Some(v) = p.acceptance_criteria {
                r.acceptance_criteria = v;
            }
            if let Some(v) = p.status {
                r.status = v;
            }
            if let Some(v) = p.result {
                r.result = v;
            }
            if let Some(v) = p.next_step {
                r.next_step = v;
            }
            if let Some(v) = p.evidence {
                r.evidence = v;
            }
            if index.is_none() || r != old {
                r.updated_at = timestamp.clone();
                changed_requirements.push(r.id.clone());
                match index {
                    Some(i) => work.requirements[i] = r,
                    None => work.requirements.push(r),
                }
            }
        }
        for p in patch.confirmations {
            let index = work.confirmations.iter().position(|c| c.id == p.id);
            let mut c = index
                .map(|i| work.confirmations[i].clone())
                .unwrap_or(Confirmation {
                    id: p.id,
                    requirement_ids: vec![],
                    question: String::new(),
                    recommendation: String::new(),
                    impact: String::new(),
                    status: ConfirmationStatus::Open,
                    answer: String::new(),
                    evidence: vec![],
                    updated_at: timestamp.clone(),
                });
            let old = c.clone();
            if let Some(v) = p.requirement_ids {
                c.requirement_ids = v;
            }
            if let Some(v) = p.question {
                c.question = v;
            }
            if let Some(v) = p.recommendation {
                c.recommendation = v;
            }
            if let Some(v) = p.impact {
                c.impact = v;
            }
            if let Some(v) = p.status {
                c.status = v;
            }
            if let Some(v) = p.answer {
                c.answer = v;
            }
            if let Some(v) = p.evidence {
                c.evidence = v;
            }
            if index.is_none() || c != old {
                c.updated_at = timestamp.clone();
                changed_confirmations.push(c.id.clone());
                match index {
                    Some(i) => work.confirmations[i] = c,
                    None => work.confirmations.push(c),
                }
            }
        }
        validate(&work)?;
        if before.as_ref() == Some(&work) {
            tx.commit().map_err(AppError::from_sqlite)?;
            return Ok(Outcome::new(json!({"task": task, "work": work})));
        }
        bump_task(&tx, task.id, expected, &timestamp)?;
        insert_history(
            &tx,
            task.id,
            EVENT,
            task.current_session_id.as_deref(),
            "task requirements and decisions updated",
            json!({"work": work, "reason": patch.reason, "changedRequirementIds": changed_requirements, "changedConfirmationIds": changed_confirmations}),
            &timestamp,
        )?;
        let response = load_task(&tx, task.id)?;
        tx.commit().map_err(AppError::from_sqlite)?;
        Ok(Outcome::new(json!({"task": response, "work": work})))
    }
}
