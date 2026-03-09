//! Tamper-evident server audit log helpers.

use chrono::{DateTime, Utc};
use nex_core::{
    CodexError, CodexResult, atomic_write_bytes, atomic_write_json, load_bytes_with_backup,
    load_json_with_backup,
};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use uuid::Uuid;

pub const SERVER_AUDIT_FILE: &str = "server-audit.jsonl";
pub const SERVER_AUDIT_HEAD_FILE: &str = "server-audit.head.json";

const AUDIT_RECORD_VERSION: u32 = 1;
const AUDIT_HEAD_VERSION: u32 = 1;

#[derive(Debug, Clone)]
pub struct AuditRecordInput {
    pub timestamp: DateTime<Utc>,
    pub action: String,
    pub outcome: String,
    pub method: Option<String>,
    pub path: Option<String>,
    pub authenticated_agent: Option<String>,
    pub claimed_agent: Option<String>,
    pub intent_id: Option<Uuid>,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AuditRecord {
    pub version: u32,
    pub index: u64,
    pub timestamp: DateTime<Utc>,
    pub action: String,
    pub outcome: String,
    pub method: Option<String>,
    pub path: Option<String>,
    pub authenticated_agent: Option<String>,
    pub claimed_agent: Option<String>,
    pub intent_id: Option<Uuid>,
    pub detail: Option<String>,
    pub prev_hash: Option<String>,
    pub entry_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AuditHead {
    pub version: u32,
    pub record_count: u64,
    pub last_hash: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct AuditVerificationIssue {
    pub kind: String,
    pub line: Option<usize>,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct AuditVerificationReport {
    pub log_path: PathBuf,
    pub head_path: PathBuf,
    pub record_count: usize,
    pub valid: bool,
    pub anchored: bool,
    pub last_hash: Option<String>,
    pub issues: Vec<AuditVerificationIssue>,
}

#[derive(Debug, Clone, Deserialize)]
struct LegacyAuditRecord {
    timestamp: DateTime<Utc>,
    action: String,
    outcome: String,
    method: Option<String>,
    path: Option<String>,
    authenticated_agent: Option<String>,
    claimed_agent: Option<String>,
    intent_id: Option<Uuid>,
    detail: Option<String>,
}

#[derive(Serialize)]
struct AuditHashMaterial<'a> {
    version: u32,
    index: u64,
    timestamp: DateTime<Utc>,
    action: &'a str,
    outcome: &'a str,
    method: Option<&'a str>,
    path: Option<&'a str>,
    authenticated_agent: Option<&'a str>,
    claimed_agent: Option<&'a str>,
    intent_id: Option<Uuid>,
    detail: Option<&'a str>,
    prev_hash: Option<&'a str>,
}

#[derive(Debug, Clone, Copy)]
enum VerificationMode {
    Strict,
    RepairLegacy,
}

#[derive(Debug)]
struct LoadedAuditLog {
    records: Vec<AuditRecord>,
    head: Option<AuditHead>,
    issues: Vec<AuditVerificationIssue>,
    needs_log_rewrite: bool,
    needs_head_rewrite: bool,
}

impl AuditHead {
    fn from_records(records: &[AuditRecord]) -> Self {
        Self {
            version: AUDIT_HEAD_VERSION,
            record_count: records.len() as u64,
            last_hash: records.last().map(|record| record.entry_hash.clone()),
        }
    }
}

impl AuditVerificationReport {
    pub fn exit_code(&self) -> i32 {
        if self.valid { 0 } else { 1 }
    }
}

impl AuditRecordInput {
    fn into_record(self, index: u64, prev_hash: Option<String>) -> CodexResult<AuditRecord> {
        let mut record = AuditRecord {
            version: AUDIT_RECORD_VERSION,
            index,
            timestamp: self.timestamp,
            action: self.action,
            outcome: self.outcome,
            method: self.method,
            path: self.path,
            authenticated_agent: self.authenticated_agent,
            claimed_agent: self.claimed_agent,
            intent_id: self.intent_id,
            detail: self.detail,
            prev_hash,
            entry_hash: String::new(),
        };
        record.entry_hash = record.compute_hash()?;
        Ok(record)
    }
}

impl AuditRecord {
    fn from_legacy(
        index: u64,
        prev_hash: Option<String>,
        legacy: LegacyAuditRecord,
    ) -> CodexResult<Self> {
        AuditRecordInput {
            timestamp: legacy.timestamp,
            action: legacy.action,
            outcome: legacy.outcome,
            method: legacy.method,
            path: legacy.path,
            authenticated_agent: legacy.authenticated_agent,
            claimed_agent: legacy.claimed_agent,
            intent_id: legacy.intent_id,
            detail: legacy.detail,
        }
        .into_record(index, prev_hash)
    }

    fn compute_hash(&self) -> CodexResult<String> {
        let material = AuditHashMaterial {
            version: self.version,
            index: self.index,
            timestamp: self.timestamp,
            action: &self.action,
            outcome: &self.outcome,
            method: self.method.as_deref(),
            path: self.path.as_deref(),
            authenticated_agent: self.authenticated_agent.as_deref(),
            claimed_agent: self.claimed_agent.as_deref(),
            intent_id: self.intent_id,
            detail: self.detail.as_deref(),
            prev_hash: self.prev_hash.as_deref(),
        };
        let bytes = serde_json::to_vec(&material)?;
        Ok(blake3::hash(&bytes).to_hex().to_string())
    }
}

pub fn default_audit_log_path(repo_path: &Path) -> PathBuf {
    repo_path.join(".nex").join(SERVER_AUDIT_FILE)
}

pub fn audit_head_path(log_path: &Path) -> PathBuf {
    let file_name = log_path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(SERVER_AUDIT_FILE);
    let head_name = if let Some(stripped) = file_name.strip_suffix(".jsonl") {
        format!("{stripped}.head.json")
    } else {
        format!("{file_name}.head.json")
    };
    log_path.with_file_name(head_name)
}

pub fn append_audit_record(log_path: &Path, input: AuditRecordInput) -> CodexResult<()> {
    if let Some(parent) = log_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let mut loaded = load_audit_log(log_path, VerificationMode::RepairLegacy)?;
    if !loaded.issues.is_empty() {
        return Err(CodexError::Coordination(format!(
            "refusing to append to tampered audit log {}; run `nex audit verify`",
            log_path.display()
        )));
    }

    if loaded.needs_log_rewrite {
        rewrite_audit_log(log_path, &loaded.records)?;
    }
    if loaded.needs_head_rewrite {
        write_audit_head(log_path, &loaded.records)?;
    }

    let index = loaded.records.len() as u64;
    let prev_hash = loaded
        .records
        .last()
        .map(|record| record.entry_hash.clone());
    let record = input.into_record(index, prev_hash)?;

    {
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(log_path)?;
        serde_json::to_writer(&mut file, &record)?;
        use std::io::Write as _;
        file.write_all(b"\n")?;
        file.sync_data()?;
    }

    loaded.records.push(record);
    write_audit_head(log_path, &loaded.records)
}

pub fn verify_audit_log(
    repo_path: &Path,
    audit_log: Option<PathBuf>,
) -> CodexResult<AuditVerificationReport> {
    let log_path = audit_log.unwrap_or_else(|| default_audit_log_path(repo_path));
    let head_path = audit_head_path(&log_path);
    let loaded = load_audit_log(&log_path, VerificationMode::Strict)?;

    Ok(AuditVerificationReport {
        log_path,
        head_path,
        record_count: loaded.records.len(),
        valid: loaded.issues.is_empty(),
        anchored: loaded.head.is_some(),
        last_hash: loaded
            .records
            .last()
            .map(|record| record.entry_hash.clone()),
        issues: loaded.issues,
    })
}

fn load_audit_log(log_path: &Path, mode: VerificationMode) -> CodexResult<LoadedAuditLog> {
    let head_path = audit_head_path(log_path);
    let Some(bytes) = load_bytes_with_backup(log_path)? else {
        let head = load_json_with_backup::<AuditHead>(&head_path)?;
        let mut issues = Vec::new();
        let needs_head_rewrite = match head {
            Some(ref head) if head.record_count == 0 && head.last_hash.is_none() => false,
            Some(_) => {
                issues.push(AuditVerificationIssue {
                    kind: "orphaned_head".to_string(),
                    line: None,
                    description: format!(
                        "audit head {} exists without a matching audit log",
                        head_path.display()
                    ),
                });
                false
            }
            None => false,
        };
        return Ok(LoadedAuditLog {
            records: Vec::new(),
            head,
            issues,
            needs_log_rewrite: false,
            needs_head_rewrite,
        });
    };

    let content = String::from_utf8(bytes).map_err(|err| {
        CodexError::Coordination(format!(
            "audit log {} is not valid UTF-8: {err}",
            log_path.display()
        ))
    })?;

    let mut records = Vec::new();
    let mut issues = Vec::new();
    let mut needs_log_rewrite = false;
    let mut prev_hash: Option<String> = None;

    for (line_index, line) in content.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let expected_index = records.len() as u64;
        match serde_json::from_str::<AuditRecord>(line) {
            Ok(record) => {
                if let Some(issue) = validate_record(
                    &record,
                    expected_index,
                    prev_hash.as_deref(),
                    line_index + 1,
                )? {
                    issues.push(issue);
                }
                prev_hash = Some(record.entry_hash.clone());
                records.push(record);
            }
            Err(_) => match serde_json::from_str::<LegacyAuditRecord>(line) {
                Ok(legacy) => match mode {
                    VerificationMode::Strict => {
                        issues.push(AuditVerificationIssue {
                            kind: "legacy_record".to_string(),
                            line: Some(line_index + 1),
                            description: "audit record uses the pre-chain legacy format"
                                .to_string(),
                        });
                        let record =
                            AuditRecord::from_legacy(expected_index, prev_hash.clone(), legacy)?;
                        prev_hash = Some(record.entry_hash.clone());
                        records.push(record);
                    }
                    VerificationMode::RepairLegacy => {
                        let record =
                            AuditRecord::from_legacy(expected_index, prev_hash.clone(), legacy)?;
                        prev_hash = Some(record.entry_hash.clone());
                        records.push(record);
                        needs_log_rewrite = true;
                    }
                },
                Err(err) => {
                    issues.push(AuditVerificationIssue {
                        kind: "parse_error".to_string(),
                        line: Some(line_index + 1),
                        description: format!("invalid audit record JSON: {err}"),
                    });
                }
            },
        }
    }

    let head = load_json_with_backup::<AuditHead>(&head_path)?;
    let expected_head = AuditHead::from_records(&records);
    let mut needs_head_rewrite = false;

    match &head {
        Some(head)
            if head.version == AUDIT_HEAD_VERSION
                && head.record_count == expected_head.record_count
                && head.last_hash == expected_head.last_hash => {}
        Some(head) => match mode {
            VerificationMode::Strict => issues.push(AuditVerificationIssue {
                kind: "head_mismatch".to_string(),
                line: None,
                description: format!(
                    "audit head mismatch: expected count {} / hash {:?}, found count {} / hash {:?}",
                    expected_head.record_count,
                    expected_head.last_hash,
                    head.record_count,
                    head.last_hash
                ),
            }),
            VerificationMode::RepairLegacy => needs_head_rewrite = true,
        },
        None if !records.is_empty() => match mode {
            VerificationMode::Strict => issues.push(AuditVerificationIssue {
                kind: "missing_head".to_string(),
                line: None,
                description: format!(
                    "audit head {} is missing for a non-empty audit log",
                    head_path.display()
                ),
            }),
            VerificationMode::RepairLegacy => needs_head_rewrite = true,
        },
        None => {}
    }

    Ok(LoadedAuditLog {
        records,
        head,
        issues,
        needs_log_rewrite,
        needs_head_rewrite,
    })
}

fn validate_record(
    record: &AuditRecord,
    expected_index: u64,
    expected_prev_hash: Option<&str>,
    line: usize,
) -> CodexResult<Option<AuditVerificationIssue>> {
    if record.version != AUDIT_RECORD_VERSION {
        return Ok(Some(AuditVerificationIssue {
            kind: "unsupported_version".to_string(),
            line: Some(line),
            description: format!(
                "audit record version {} is unsupported; expected {}",
                record.version, AUDIT_RECORD_VERSION
            ),
        }));
    }

    if record.index != expected_index {
        return Ok(Some(AuditVerificationIssue {
            kind: "index_mismatch".to_string(),
            line: Some(line),
            description: format!(
                "audit record index {} does not match expected {}",
                record.index, expected_index
            ),
        }));
    }

    if record.prev_hash.as_deref() != expected_prev_hash {
        return Ok(Some(AuditVerificationIssue {
            kind: "chain_mismatch".to_string(),
            line: Some(line),
            description: format!(
                "audit record prev_hash {:?} does not match expected {:?}",
                record.prev_hash, expected_prev_hash
            ),
        }));
    }

    let computed_hash = record.compute_hash()?;
    if record.entry_hash != computed_hash {
        return Ok(Some(AuditVerificationIssue {
            kind: "hash_mismatch".to_string(),
            line: Some(line),
            description: format!(
                "audit record hash {} does not match computed {}",
                record.entry_hash, computed_hash
            ),
        }));
    }

    Ok(None)
}

fn rewrite_audit_log(log_path: &Path, records: &[AuditRecord]) -> CodexResult<()> {
    let bytes = render_audit_log(records)?;
    atomic_write_bytes(log_path, &bytes)
}

fn write_audit_head(log_path: &Path, records: &[AuditRecord]) -> CodexResult<()> {
    let head_path = audit_head_path(log_path);
    atomic_write_json(&head_path, &AuditHead::from_records(records))
}

fn render_audit_log(records: &[AuditRecord]) -> CodexResult<Vec<u8>> {
    let mut bytes = Vec::new();
    for record in records {
        let line = serde_json::to_vec(record)?;
        bytes.extend_from_slice(&line);
        bytes.push(b'\n');
    }
    Ok(bytes)
}
