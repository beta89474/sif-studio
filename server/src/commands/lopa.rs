//! LOPA（保护层分析）commands —— SIL 定级依据
//!
//! 业务规则（IEC 61511-1 Annex E）：
//!   - scenario.code 在 project 内唯一
//!   - severity/sil_claim/layer_type 受 CHECK 约束（schema 已建）
//!   - sif_id 可空（定级前场景）；关联 SIF 删除 → SET NULL
//!   - scenario 删除 → lopa_layer CASCADE
//!   - 阶段 B：全部查询强制 org_id 作用域，project/sif join 同 org 校验
//!   - 审计：lopa_scenario_create/update/delete + lopa_layer_create/update/delete

use crate::commands::audit::{
    compute_field_diff, entity_audit_payload, snapshot, write_audit,
};
use crate::{AppError, AppResult};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

// ---------------------------------------------------------------------------
// 常量 & SQL 片段
// ---------------------------------------------------------------------------

const SELECT_SCENARIO: &str = "SELECT ls.id, ls.org_id, ls.project_id, ls.sif_id,
            p.code AS project_code, s.code AS sif_code,
            s.sil_design AS sif_sil_design, s.sil_verified AS sif_sil_verified,
            s.demand_mode AS sif_demand_mode, s.proof_interval AS sif_proof_interval,
            s.mttr_hours AS sif_mttr_hours, s.beta_factor AS sif_beta_factor,
            s.sensor_arch AS sif_sensor_arch, s.logic_arch AS sif_logic_arch,
            s.final_arch AS sif_final_arch,
            ls.code, ls.title, ls.hazard, ls.cause, ls.consequence,
            ls.severity, ls.init_freq, ls.risk_tol, ls.sil_claim, ls.notes,
            ls.created_at, ls.updated_at,
            (SELECT COUNT(*) FROM lopa_layer ll WHERE ll.scenario_id = ls.id) AS layer_count
         FROM lopa_scenario ls
         JOIN project p ON p.id = ls.project_id AND p.org_id = ls.org_id
         LEFT JOIN sif s ON s.id = ls.sif_id AND s.org_id = ls.org_id
        WHERE ls.org_id = ?";

const SELECT_LAYER: &str = "SELECT id, org_id, scenario_id, seq, layer_type, description,
            pfd, credit, created_at
         FROM lopa_layer WHERE org_id = ?";

// ---------------------------------------------------------------------------
// 类型
// ---------------------------------------------------------------------------

/// LOPA 场景列表项（含保护层 COUNT 聚合 + 运行时 gap analysis）
#[derive(Debug, Serialize, FromRow, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LopaScenario {
    pub id: i64,
    pub org_id: i64,
    pub project_id: i64,
    pub sif_id: Option<i64>,
    pub project_code: String,
    pub sif_code: Option<String>,
    // ---- 关联 SIF 快照（LEFT JOIN，无关联时为 None；中间计算参数不序列化） ----
    pub sif_sil_design: Option<String>,
    pub sif_sil_verified: Option<String>,
    #[sqlx(default)]
    #[serde(skip)]
    sif_demand_mode: Option<String>,
    #[sqlx(default)]
    #[serde(skip)]
    sif_proof_interval: Option<i64>,
    #[sqlx(default)]
    #[serde(skip)]
    sif_mttr_hours: Option<f64>,
    #[sqlx(default)]
    #[serde(skip)]
    sif_beta_factor: Option<f64>,
    #[sqlx(default)]
    #[serde(skip)]
    sif_sensor_arch: Option<String>,
    #[sqlx(default)]
    #[serde(skip)]
    sif_logic_arch: Option<String>,
    #[sqlx(default)]
    #[serde(skip)]
    sif_final_arch: Option<String>,
    pub code: String,
    pub title: String,
    pub hazard: String,
    pub cause: String,
    pub consequence: String,
    pub severity: String,
    pub init_freq: Option<f64>,
    pub risk_tol: Option<f64>,
    pub sil_claim: String,
    pub notes: String,
    pub created_at: String,
    pub updated_at: String,
    pub layer_count: i64,
    // ---- 运行时 gap analysis（不落库） ----
    /// 需要的风险降低因子 RRF = init_freq / risk_tol
    #[sqlx(default)]
    pub rrf_required: Option<f64>,
    /// RRF 按 IEC 61511 区间反推的 SIL（NA/A/B/C/D）
    #[sqlx(default)]
    pub sil_from_rrf: Option<String>,
    /// 关联 SIF 计算出的 silAchieved（无失效数据或无关联时为 None）
    #[sqlx(default)]
    pub sif_sil_achieved: Option<String>,
    /// LOPA claim ↔ SIF design/achieved 三方对照状态：
    /// unlinked / na / pending_data / covered / gap / drift
    #[sqlx(default)]
    pub gap_status: String,
    /// 人类可读对照说明（含 RRF 推导与手填 claim 不一致的告警）
    #[sqlx(default)]
    pub gap_message: String,
}

/// 新建/更新场景输入
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LopaScenarioInput {
    pub project_id: i64,
    #[serde(default)]
    pub sif_id: Option<i64>,
    pub code: String,
    pub title: String,
    #[serde(default)]
    pub hazard: String,
    #[serde(default)]
    pub cause: String,
    #[serde(default)]
    pub consequence: String,
    #[serde(default = "default_severity")]
    pub severity: String,
    pub init_freq: Option<f64>,
    pub risk_tol: Option<f64>,
    #[serde(default = "default_sil_claim")]
    pub sil_claim: String,
    #[serde(default)]
    pub notes: String,
}

fn default_severity() -> String {
    "medium".into()
}

fn default_sil_claim() -> String {
    "NA".into()
}

impl Default for LopaScenarioInput {
    fn default() -> Self {
        LopaScenarioInput {
            project_id: 0,
            sif_id: None,
            code: String::new(),
            title: String::new(),
            hazard: String::new(),
            cause: String::new(),
            consequence: String::new(),
            severity: default_severity(),
            init_freq: None,
            risk_tol: None,
            sil_claim: default_sil_claim(),
            notes: String::new(),
        }
    }
}

/// 独立保护层
#[derive(Debug, Serialize, FromRow, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LopaLayer {
    pub id: i64,
    pub org_id: i64,
    pub scenario_id: i64,
    pub seq: i64,
    pub layer_type: String,
    pub description: String,
    pub pfd: Option<f64>,
    pub credit: f64,
    pub created_at: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LopaLayerInput {
    pub scenario_id: i64,
    #[serde(default)]
    pub seq: i64,
    pub layer_type: String,
    #[serde(default)]
    pub description: String,
    pub pfd: Option<f64>,
    #[serde(default)]
    pub credit: f64,
}

// ---------------------------------------------------------------------------
// Gap analysis（LOPA sil_claim ↔ SIF sil_design / sil_achieved 三方对照）
// ---------------------------------------------------------------------------

/// SIL 字母 → 序号（A=SIL1 … D=SIL4）；NA 为 None
fn sil_idx(sil: &str) -> Option<usize> {
    match sil {
        "A" => Some(1),
        "B" => Some(2),
        "C" => Some(3),
        "D" => Some(4),
        _ => None,
    }
}

/// RRF → SIL 区间映射（IEC 61511-1 Annex A / IEC 61508-5）：
///   <10 NA · [10,100) SIL1 · [100,1e3) SIL2 · [1e3,1e4) SIL3 · ≥1e4 SIL4
fn sil_from_rrf(rrf: f64) -> String {
    if rrf < 10.0 {
        "NA".into()
    } else if rrf < 100.0 {
        "A".into()
    } else if rrf < 1000.0 {
        "B".into()
    } else if rrf < 10_000.0 {
        "C".into()
    } else {
        "D".into()
    }
}

/// 对一批场景填充运行时 gap analysis 字段。
///
/// 关联了 SIF 的场景复用 sifs::compute_sif_metrics 计算 silAchieved，
/// 再按状态机判定：unlinked / na / pending_data / covered / gap / drift。
async fn enrich_gap_analysis(
    pool: &sqlx::SqlitePool,
    org_id: i64,
    rows: &mut [LopaScenario],
) -> AppResult<()> {
    for scn in rows.iter_mut() {
        // 1) RRF 反推（init_freq 与 risk_tol 均为正才有意义）
        if let (Some(f), Some(tol)) = (scn.init_freq, scn.risk_tol) {
            if f > 0.0 && tol > 0.0 {
                let rrf = f / tol;
                scn.rrf_required = Some(rrf);
                scn.sil_from_rrf = Some(sil_from_rrf(rrf));
            }
        }

        let mut warnings: Vec<String> = Vec::new();
        // RRF 推导与手填 sil_claim 不一致 → 数据质量告警（不改变主状态）
        if let Some(rrf_sil) = &scn.sil_from_rrf {
            if rrf_sil != &scn.sil_claim {
                warnings.push(format!(
                    "RRF={:.0} 反推 SIL {rrf_sil}，与手填定级 SIL {} 不一致",
                    scn.rrf_required.unwrap_or_default(),
                    scn.sil_claim
                ));
            }
        }

        // 2) 三方对照
        let Some(sif_id) = scn.sif_id else {
            scn.gap_status = "unlinked".into();
            scn.gap_message = "未关联 SIF（定级前场景）".into();
            if !warnings.is_empty() {
                scn.gap_message.push_str("；");
                scn.gap_message.push_str(&warnings.join("；"));
            }
            continue;
        };

        if sil_idx(&scn.sil_claim).is_none() {
            scn.gap_status = "na".into();
            scn.gap_message = "LOPA 判定无需 SIS（SIL NA）".into();
            continue;
        }

        // 取 SIF 计算参数（JOIN 字段齐全时）
        let ready = match (
            scn.sif_demand_mode.as_deref(),
            scn.sif_proof_interval,
            scn.sif_mttr_hours,
            scn.sif_beta_factor,
            scn.sif_sensor_arch.as_deref(),
            scn.sif_logic_arch.as_deref(),
            scn.sif_final_arch.as_deref(),
        ) {
            (Some(mode), Some(ti), Some(mttr), Some(beta), Some(sa), Some(la), Some(fa)) => {
                Some((mode, ti, mttr, beta, sa, la, fa))
            }
            _ => None,
        };

        let claim_idx = sil_idx(&scn.sil_claim).expect("claim checked non-NA");
        let sif_code = scn.sif_code.as_deref().unwrap_or("?");

        let Some((mode, ti, mttr, beta, sa, la, fa)) = ready else {
            scn.gap_status = "pending_data".into();
            scn.gap_message =
                format!("SIF {sif_code} 参数不完整，无法验证是否覆盖 SIL {}", scn.sil_claim);
            continue;
        };

        let m = crate::commands::sifs::compute_sif_metrics(
            pool, org_id, sif_id, ti, mttr, beta, mode, sa, la, fa,
        )
        .await?;
        let has_data = m.pfd.is_some() || m.pfh.is_some();
        if !has_data {
            scn.gap_status = "pending_data".into();
            scn.gap_message = format!(
                "SIF {sif_code} 缺失效数据，无法验证是否覆盖 SIL {}",
                scn.sil_claim
            );
            continue;
        }
        scn.sif_sil_achieved = Some(m.sil_achieved.clone());

        let achieved_idx = sil_idx(&m.sil_achieved);
        scn.gap_status = match achieved_idx {
            Some(a) if a >= claim_idx => {
                let design = scn.sif_sil_design.as_deref().unwrap_or("NA");
                if design != scn.sil_claim {
                    "drift".into()
                } else {
                    "covered".into()
                }
            }
            _ => "gap".into(),
        };
        scn.gap_message = match scn.gap_status.as_str() {
            "gap" => format!(
                "SIF {sif_code} 计算达到 SIL {}，低于 LOPA 定级 SIL {}，存在残余风险缺口",
                m.sil_achieved, scn.sil_claim
            ),
            "drift" => format!(
                "LOPA 定级 SIL {} 与 SIF {sif_code} 设计目标 SIL {} 不一致（计算值 {} 当前可覆盖）",
                scn.sil_claim,
                scn.sif_sil_design.as_deref().unwrap_or("NA"),
                m.sil_achieved
            ),
            _ => format!(
                "SIF {sif_code} 计算达到 SIL {}，覆盖 LOPA 定级 SIL {}",
                m.sil_achieved, scn.sil_claim
            ),
        };
        if !warnings.is_empty() {
            scn.gap_message.push_str("；");
            scn.gap_message.push_str(&warnings.join("；"));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 场景 CRUD
// ---------------------------------------------------------------------------

pub async fn list_lopa_scenarios_inner(
    pool: &sqlx::SqlitePool,
    org_id: i64,
    project_id: Option<i64>,
) -> AppResult<Vec<LopaScenario>> {
    let mut rows = if let Some(pid) = project_id {
        sqlx::query_as::<_, LopaScenario>(&format!(
            "{SELECT_SCENARIO} AND ls.project_id = ? ORDER BY ls.code"
        ))
        .bind(org_id)
        .bind(pid)
        .fetch_all(pool)
        .await?
    } else {
        sqlx::query_as::<_, LopaScenario>(&format!(
            "{SELECT_SCENARIO} ORDER BY ls.code"
        ))
        .bind(org_id)
        .fetch_all(pool)
        .await?
    };
    enrich_gap_analysis(pool, org_id, &mut rows).await?;
    Ok(rows)
}

pub async fn get_lopa_scenario_inner(
    pool: &sqlx::SqlitePool,
    org_id: i64,
    id: i64,
) -> AppResult<LopaScenario> {
    let mut row = sqlx::query_as::<_, LopaScenario>(&format!("{SELECT_SCENARIO} AND ls.id = ?"))
        .bind(org_id)
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("lopa_scenario id={id}")))?;
    enrich_gap_analysis(pool, org_id, std::slice::from_mut(&mut row)).await?;
    Ok(row)
}

async fn ensure_scenario_scope(
    pool: &sqlx::SqlitePool,
    org_id: i64,
    input: &LopaScenarioInput,
) -> AppResult<()> {
    if input.code.trim().is_empty() || input.title.trim().is_empty() {
        return Err(AppError::Validation("code and title are required".into()));
    }
    let proj_exists: Option<i64> =
        sqlx::query_scalar("SELECT id FROM project WHERE org_id = ? AND id = ?")
            .bind(org_id)
            .bind(input.project_id)
            .fetch_optional(pool)
            .await?;
    if proj_exists.is_none() {
        return Err(AppError::Validation(format!(
            "project_id {} not found in current organization",
            input.project_id
        )));
    }
    if let Some(sid) = input.sif_id {
        let sif_exists: Option<i64> =
            sqlx::query_scalar("SELECT id FROM sif WHERE org_id = ? AND id = ?")
                .bind(org_id)
                .bind(sid)
                .fetch_optional(pool)
                .await?;
        if sif_exists.is_none() {
            return Err(AppError::Validation(format!(
                "sif_id {} not found in current organization",
                sid
            )));
        }
    }
    Ok(())
}

pub async fn create_lopa_scenario_inner(
    pool: &sqlx::SqlitePool,
    org_id: i64,
    input: &LopaScenarioInput,
    actor: &str,
) -> AppResult<LopaScenario> {
    ensure_scenario_scope(pool, org_id, input).await?;

    let res = sqlx::query(
        "INSERT INTO lopa_scenario (org_id, project_id, sif_id, code, title, hazard, cause,
                                    consequence, severity, init_freq, risk_tol, sil_claim, notes)
         VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?)",
    )
    .bind(org_id)
    .bind(input.project_id)
    .bind(input.sif_id)
    .bind(&input.code)
    .bind(&input.title)
    .bind(&input.hazard)
    .bind(&input.cause)
    .bind(&input.consequence)
    .bind(&input.severity)
    .bind(input.init_freq)
    .bind(input.risk_tol)
    .bind(&input.sil_claim)
    .bind(&input.notes)
    .execute(pool)
    .await;

    let id = match res {
        Ok(r) => r.last_insert_rowid(),
        Err(sqlx::Error::Database(db)) if db.is_unique_violation() => {
            return Err(AppError::Conflict(format!(
                "lopa scenario code '{}' already exists in project {}",
                input.code, input.project_id
            )));
        }
        Err(e) => return Err(AppError::from(e)),
    };

    let created = get_lopa_scenario_inner(pool, org_id, id).await?;
    let payload = entity_audit_payload(None, Some(snapshot(&created)), vec!["*".to_string()]);
    let _ = write_audit(
        pool,
        org_id,
        actor,
        "lopa_scenario_create",
        "lopa_scenario",
        Some(id),
        payload,
    )
    .await;

    Ok(created)
}

pub async fn update_lopa_scenario_inner(
    pool: &sqlx::SqlitePool,
    org_id: i64,
    id: i64,
    input: &LopaScenarioInput,
    actor: &str,
) -> AppResult<LopaScenario> {
    let before = get_lopa_scenario_inner(pool, org_id, id).await?;
    ensure_scenario_scope(pool, org_id, input).await?;

    let n = sqlx::query(
        "UPDATE lopa_scenario SET
            project_id=?, sif_id=?, code=?, title=?, hazard=?, cause=?, consequence=?,
            severity=?, init_freq=?, risk_tol=?, sil_claim=?, notes=?,
            updated_at=datetime('now')
         WHERE org_id = ? AND id = ?",
    )
    .bind(input.project_id)
    .bind(input.sif_id)
    .bind(&input.code)
    .bind(&input.title)
    .bind(&input.hazard)
    .bind(&input.cause)
    .bind(&input.consequence)
    .bind(&input.severity)
    .bind(input.init_freq)
    .bind(input.risk_tol)
    .bind(&input.sil_claim)
    .bind(&input.notes)
    .bind(org_id)
    .bind(id)
    .execute(pool)
    .await?
    .rows_affected();
    if n == 0 {
        return Err(AppError::NotFound(format!("lopa_scenario id={id}")));
    }

    let after = get_lopa_scenario_inner(pool, org_id, id).await?;
    let fields_changed = compute_field_diff(&snapshot(&before), &snapshot(&after));
    let payload = entity_audit_payload(Some(snapshot(&before)), Some(snapshot(&after)), fields_changed);
    let _ = write_audit(
        pool,
        org_id,
        actor,
        "lopa_scenario_update",
        "lopa_scenario",
        Some(id),
        payload,
    )
    .await;

    Ok(after)
}

pub async fn delete_lopa_scenario_inner(
    pool: &sqlx::SqlitePool,
    org_id: i64,
    id: i64,
    actor: &str,
) -> AppResult<()> {
    let before = get_lopa_scenario_inner(pool, org_id, id).await?;
    let n = sqlx::query("DELETE FROM lopa_scenario WHERE org_id = ? AND id = ?")
        .bind(org_id)
        .bind(id)
        .execute(pool)
        .await?
        .rows_affected();
    if n == 0 {
        return Err(AppError::NotFound(format!("lopa_scenario id={id}")));
    }

    let payload = entity_audit_payload(Some(snapshot(&before)), None, vec!["*".to_string()]);
    let _ = write_audit(
        pool,
        org_id,
        actor,
        "lopa_scenario_delete",
        "lopa_scenario",
        Some(id),
        payload,
    )
    .await;

    Ok(())
}

// ---------------------------------------------------------------------------
// 保护层 CRUD
// ---------------------------------------------------------------------------

pub async fn list_lopa_layers_inner(
    pool: &sqlx::SqlitePool,
    org_id: i64,
    scenario_id: i64,
) -> AppResult<Vec<LopaLayer>> {
    // 先校验 scenario 同 org，不暴露存在性
    get_lopa_scenario_inner(pool, org_id, scenario_id).await?;
    let rows = sqlx::query_as::<_, LopaLayer>(&format!(
        "{SELECT_LAYER} AND scenario_id = ? ORDER BY seq, id"
    ))
    .bind(org_id)
    .bind(scenario_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn create_lopa_layer_inner(
    pool: &sqlx::SqlitePool,
    org_id: i64,
    input: &LopaLayerInput,
    actor: &str,
) -> AppResult<LopaLayer> {
    // 校验 scenario 同 org
    get_lopa_scenario_inner(pool, org_id, input.scenario_id).await?;
    if input.layer_type.trim().is_empty() {
        return Err(AppError::Validation("layer_type is required".into()));
    }

    let res = sqlx::query(
        "INSERT INTO lopa_layer (org_id, scenario_id, seq, layer_type, description, pfd, credit)
         VALUES (?,?,?,?,?,?,?)",
    )
    .bind(org_id)
    .bind(input.scenario_id)
    .bind(input.seq)
    .bind(&input.layer_type)
    .bind(&input.description)
    .bind(input.pfd)
    .bind(input.credit)
    .execute(pool)
    .await;

    let id = match res {
        Ok(r) => r.last_insert_rowid(),
        Err(e) => return Err(AppError::from(e)),
    };

    let created = sqlx::query_as::<_, LopaLayer>(&format!(
        "{SELECT_LAYER} AND id = ?"
    ))
    .bind(org_id)
    .bind(id)
    .fetch_one(pool)
    .await?;

    let payload = entity_audit_payload(None, Some(snapshot(&created)), vec!["*".to_string()]);
    let _ = write_audit(
        pool,
        org_id,
        actor,
        "lopa_layer_create",
        "lopa_layer",
        Some(id),
        payload,
    )
    .await;

    Ok(created)
}

pub async fn update_lopa_layer_inner(
    pool: &sqlx::SqlitePool,
    org_id: i64,
    id: i64,
    input: &LopaLayerInput,
    actor: &str,
) -> AppResult<LopaLayer> {
    let before = sqlx::query_as::<_, LopaLayer>(&format!("{SELECT_LAYER} AND id = ?"))
        .bind(org_id)
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("lopa_layer id={id}")))?;
    // 校验目标 scenario 同 org
    get_lopa_scenario_inner(pool, org_id, input.scenario_id).await?;
    if input.layer_type.trim().is_empty() {
        return Err(AppError::Validation("layer_type is required".into()));
    }

    let n = sqlx::query(
        "UPDATE lopa_layer SET
            scenario_id=?, seq=?, layer_type=?, description=?, pfd=?, credit=?
         WHERE org_id = ? AND id = ?",
    )
    .bind(input.scenario_id)
    .bind(input.seq)
    .bind(&input.layer_type)
    .bind(&input.description)
    .bind(input.pfd)
    .bind(input.credit)
    .bind(org_id)
    .bind(id)
    .execute(pool)
    .await?
    .rows_affected();
    if n == 0 {
        return Err(AppError::NotFound(format!("lopa_layer id={id}")));
    }

    let after = sqlx::query_as::<_, LopaLayer>(&format!("{SELECT_LAYER} AND id = ?"))
        .bind(org_id)
        .bind(id)
        .fetch_one(pool)
        .await?;

    let fields_changed = compute_field_diff(&snapshot(&before), &snapshot(&after));
    let payload = entity_audit_payload(Some(snapshot(&before)), Some(snapshot(&after)), fields_changed);
    let _ = write_audit(
        pool,
        org_id,
        actor,
        "lopa_layer_update",
        "lopa_layer",
        Some(id),
        payload,
    )
    .await;

    Ok(after)
}

pub async fn delete_lopa_layer_inner(
    pool: &sqlx::SqlitePool,
    org_id: i64,
    id: i64,
    actor: &str,
) -> AppResult<()> {
    let before = sqlx::query_as::<_, LopaLayer>(&format!("{SELECT_LAYER} AND id = ?"))
        .bind(org_id)
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("lopa_layer id={id}")))?;

    let n = sqlx::query("DELETE FROM lopa_layer WHERE org_id = ? AND id = ?")
        .bind(org_id)
        .bind(id)
        .execute(pool)
        .await?
        .rows_affected();
    if n == 0 {
        return Err(AppError::NotFound(format!("lopa_layer id={id}")));
    }

    let payload = entity_audit_payload(Some(snapshot(&before)), None, vec!["*".to_string()]);
    let _ = write_audit(
        pool,
        org_id,
        actor,
        "lopa_layer_delete",
        "lopa_layer",
        Some(id),
        payload,
    )
    .await;

    Ok(())
}
