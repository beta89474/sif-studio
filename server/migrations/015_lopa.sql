-- ===========================================================================
-- 015_lopa.sql
--
-- LOPA（Layer of Protection Analysis）定级依据（IEC 61511-1 Annex E / GB/T 21109）：
--   保护层分析用于确定 SIL 等级。一个 hazard scenario（危险场景）含初始事件
--   频率、后果严重度与容许风险频率，场景下挂多个独立保护层（IPL）。
--   场景推导出的 SIL claim 与 SIF.sil_design/sil_verified 形成溯源闭环。
--
--   结构（规范化）：
--     lopa_scenario  场景主表（org + project 作用域，sif 软关联）
--     lopa_layer     保护层子表（scenario_id 外键，CASCADE 删除）
--
--   全表 org_id 作用域，与既有 project/sif/bypass 一致。
-- ===========================================================================

-- ---------------------------------------------------------------------------
-- LOPA 场景（hazard scenario）
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS lopa_scenario (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    org_id        INTEGER NOT NULL,
    project_id    INTEGER NOT NULL,
    sif_id        INTEGER,                          -- 关联 SIF（NULL=定级前场景）
    code          TEXT    NOT NULL,                 -- SC-001
    title         TEXT    NOT NULL,
    hazard        TEXT    NOT NULL DEFAULT '',      -- 危险描述
    cause         TEXT    NOT NULL DEFAULT '',      -- 初始事件 / 原因
    consequence   TEXT    NOT NULL DEFAULT '',      -- 后果
    severity      TEXT    NOT NULL DEFAULT 'medium', -- minor|medium|major|catastrophic
    init_freq     REAL,                             -- 初始事件频率（/年）
    risk_tol      REAL,                             -- 容许风险频率（/年）
    sil_claim     TEXT    NOT NULL DEFAULT 'NA',    -- LOPA 推导 SIL（A|B|C|D|NA）
    notes         TEXT    NOT NULL DEFAULT '',
    created_at    TEXT    NOT NULL DEFAULT (datetime('now')),
    updated_at    TEXT    NOT NULL DEFAULT (datetime('now')),
    FOREIGN KEY (project_id) REFERENCES project(id) ON DELETE CASCADE,
    FOREIGN KEY (sif_id)     REFERENCES sif(id)     ON DELETE SET NULL,
    CHECK (severity  IN ('minor','medium','major','catastrophic')),
    CHECK (sil_claim IN ('NA','A','B','C','D'))
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_lopa_scenario_code ON lopa_scenario(project_id, code);
CREATE INDEX IF NOT EXISTS idx_lopa_scenario_sif  ON lopa_scenario(sif_id);
CREATE INDEX IF NOT EXISTS idx_lopa_scenario_org  ON lopa_scenario(org_id);

-- ---------------------------------------------------------------------------
-- 独立保护层（IPL / Independent Protection Layer）
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS lopa_layer (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    org_id        INTEGER NOT NULL,
    scenario_id   INTEGER NOT NULL,
    seq           INTEGER NOT NULL DEFAULT 0,       -- 层级顺序
    layer_type    TEXT    NOT NULL,                 -- ipl|bypass|alarm|procedural
    description   TEXT    NOT NULL DEFAULT '',
    pfd           REAL,                             -- 保护层 PFD（1/要求）
    credit        REAL    NOT NULL DEFAULT 0,        -- 信用因子
    created_at     TEXT    NOT NULL DEFAULT (datetime('now')),
    FOREIGN KEY (scenario_id) REFERENCES lopa_scenario(id) ON DELETE CASCADE,
    CHECK (layer_type IN ('ipl','bypass','alarm','procedural'))
);

CREATE INDEX IF NOT EXISTS idx_lopa_layer_scenario ON lopa_layer(scenario_id);
CREATE INDEX IF NOT EXISTS idx_lopa_layer_org      ON lopa_layer(org_id);
