-- ===========================================================================
-- SIF Studio / 联锁工坊 — 初始 schema（v001）
-- 标准依据：IEC 61511-1:2016 / GB/T 21109.1-2007 / ISA-5.1/5.2 / GB/T 50770-2013
-- 说明：单文件嵌入式迁移，sqlx::migrate!() 启动时自动跑。
--       表名按业务域聚类，索引覆盖高频查询路径，CHECK 约束保语义。
-- ===========================================================================

-- ---------------------------------------------------------------------------
-- 1. 仪表主数据（instrument）—— 来自真实台账，P&ID / 出厂资料的「位号字典」
--    角色枚举与 ISA-5.1 / IEC 61511-1 §11.4 / 11.5 一致：
--      detector  检测元件（PT / LT / PSV / TT / FLSL …）
--      final     最终元件（XV / SOV / SDV / FCV / 切断阀 …）
--      logic     逻辑求解器（继电器 / 表决 / 锁存 / 计时）
--      aux       旁路/允许设备（BYP / 复位按钮 / 试验按钮）
--    sil_target  对该仪表的目标 SIL（A/B/C/D/NA）
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS instrument (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    tag             TEXT    NOT NULL,                           -- 位号（如 PT-201）
    service         TEXT    NOT NULL DEFAULT '',                -- 服务描述
    kind            TEXT    NOT NULL,                           -- 仪表类型（PT/TT/LT…）
    role            TEXT    NOT NULL,                           -- detector|final|logic|aux
    psv_id          TEXT    NOT NULL DEFAULT '',                -- 厂内 PSV 编号
    manufacturer    TEXT    NOT NULL DEFAULT '',
    model           TEXT    NOT NULL DEFAULT '',
    range_min       REAL,                                       -- 量程下限
    range_max       REAL,                                       -- 量程上限
    unit            TEXT    NOT NULL DEFAULT '',
    setpoint        REAL,                                       -- 设定值
    sil_target      TEXT    NOT NULL DEFAULT 'NA',              -- A|B|C|D|NA
    proof_interval  INTEGER NOT NULL DEFAULT 0,                 -- 检验周期（月）
    installed_at    TEXT    NOT NULL DEFAULT '',                -- ISO 8601
    notes           TEXT    NOT NULL DEFAULT '',
    created_at      TEXT    NOT NULL DEFAULT (datetime('now')),
    updated_at      TEXT    NOT NULL DEFAULT (datetime('now')),
    CHECK (role IN ('detector', 'final', 'logic', 'aux')),
    CHECK (sil_target IN ('NA', 'A', 'B', 'C', 'D'))
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_instrument_tag ON instrument(tag);
CREATE INDEX IF NOT EXISTS idx_instrument_role ON instrument(role);
CREATE INDEX IF NOT EXISTS idx_instrument_kind ON instrument(kind);


-- ---------------------------------------------------------------------------
-- 2. 项目（project）—— 一个工厂 / 装置 / 子系统级别
--    名称+编号复合唯一，便于以后从外部 CSV/PDF 导入时去重
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS project (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    code        TEXT    NOT NULL,                              -- 项目编号（PRJ-001）
    name        TEXT    NOT NULL,                              -- 项目名称（"120 万吨/年催化裂化"）
    client      TEXT    NOT NULL DEFAULT '',                   -- 业主
    location    TEXT    NOT NULL DEFAULT '',                   -- 厂区/装置
    phase       TEXT    NOT NULL DEFAULT 'design',             -- design|construction|commissioning|operation
    started_at  TEXT    NOT NULL DEFAULT (datetime('now')),
    finished_at TEXT    NOT NULL DEFAULT '',
    notes       TEXT    NOT NULL DEFAULT '',
    created_at  TEXT    NOT NULL DEFAULT (datetime('now')),
    updated_at  TEXT    NOT NULL DEFAULT (datetime('now')),
    CHECK (phase IN ('design', 'construction', 'commissioning', 'operation', 'closed'))
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_project_code ON project(code);


-- ---------------------------------------------------------------------------
-- 3. 联锁逻辑图（diagram）—— 一个 SIF 对应一张图，project 下多张
--    data 字段是图本身（M0 编辑器导出的 JSON 快照）
--    sheet_size 是 A 系列横式图幅（A0/A1/A2/A3/A4）
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS diagram (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id   INTEGER NOT NULL,
    code         TEXT    NOT NULL,
    name         TEXT    NOT NULL,
    sif_id       INTEGER,                                       -- 软链 SIF（NULL=未指派）
    sheet_size   TEXT    NOT NULL DEFAULT 'A1',
    revision     TEXT    NOT NULL DEFAULT 'A0',
    data         TEXT    NOT NULL DEFAULT '{}',                 -- M0 编辑器 JSON 快照
    updated_at   TEXT    NOT NULL DEFAULT (datetime('now')),
    created_at   TEXT    NOT NULL DEFAULT (datetime('now')),
    FOREIGN KEY (project_id) REFERENCES project(id) ON DELETE CASCADE,
    FOREIGN KEY (sif_id)     REFERENCES sif(id)     ON DELETE SET NULL,
    CHECK (sheet_size IN ('A0','A1','A2','A3','A4'))
);

CREATE INDEX IF NOT EXISTS idx_diagram_project  ON diagram(project_id);
CREATE INDEX IF NOT EXISTS idx_diagram_sif      ON diagram(sif_id);
CREATE UNIQUE INDEX IF NOT EXISTS idx_diagram_project_code ON diagram(project_id, code);


-- ---------------------------------------------------------------------------
-- 4. 安全仪表功能（SIF）—— 业务核心，一张图可以有 0/1 个 SIF
--    sil_verified = 工程已验算的 SIL（来自 SIL 验算流程）
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS sif (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id   INTEGER NOT NULL,
    code         TEXT    NOT NULL,                              -- SIF-201 / SIF-202 …
    name         TEXT    NOT NULL,
    description  TEXT    NOT NULL DEFAULT '',
    sil_design   TEXT    NOT NULL DEFAULT 'NA',                -- 设计 SIL（来自因果表/PHA）
    sil_verified TEXT    NOT NULL DEFAULT 'NA',                -- 工程验算结果
    demand_mode  TEXT    NOT NULL DEFAULT 'low',               -- low|high（IEC 61511-1 §5.2.6.1.6）
    pfdavg_target REAL,                                         -- 目标 PFDavg
    proof_interval INTEGER NOT NULL DEFAULT 12,                -- 检验周期（月）
    created_at   TEXT    NOT NULL DEFAULT (datetime('now')),
    updated_at   TEXT    NOT NULL DEFAULT (datetime('now')),
    FOREIGN KEY (project_id) REFERENCES project(id) ON DELETE CASCADE,
    CHECK (sil_design   IN ('NA','A','B','C','D')),
    CHECK (sil_verified IN ('NA','A','B','C','D')),
    CHECK (demand_mode  IN ('low','high'))
);

CREATE INDEX IF NOT EXISTS idx_sif_project        ON sif(project_id);
CREATE UNIQUE INDEX IF NOT EXISTS idx_sif_project_code ON sif(project_id, code);


-- ---------------------------------------------------------------------------
-- 5. SIF ↔ 仪表 关联（sif_instrument）
--    同一仪表在多个 SIF 中扮演不同角色（如 LSL 既是主 SIF 的检测又是
--    旁路 SIF 的旁通触发），所以没做 (sif_id, instrument_id) 唯一。
--    role 决定它在 SIF 中的功能位置。
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS sif_instrument (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    sif_id        INTEGER NOT NULL,
    instrument_id INTEGER NOT NULL,
    role          TEXT    NOT NULL,                            -- detector|final|logic|aux
    port_index    INTEGER NOT NULL DEFAULT 0,                 -- 在 SIF 图上的端口号
    diagram_id    INTEGER,                                     -- 跨图汇总时按 diagram 聚合
    note          TEXT    NOT NULL DEFAULT '',
    created_at    TEXT    NOT NULL DEFAULT (datetime('now')),
    FOREIGN KEY (sif_id)        REFERENCES sif(id)        ON DELETE CASCADE,
    FOREIGN KEY (instrument_id) REFERENCES instrument(id) ON DELETE CASCADE,
    FOREIGN KEY (diagram_id)    REFERENCES diagram(id)    ON DELETE SET NULL,
    CHECK (role IN ('detector', 'final', 'logic', 'aux'))
);

CREATE INDEX IF NOT EXISTS idx_link_sif        ON sif_instrument(sif_id);
CREATE INDEX IF NOT EXISTS idx_link_instrument ON sif_instrument(instrument_id);
CREATE INDEX IF NOT EXISTS idx_link_diagram    ON sif_instrument(diagram_id);


-- ---------------------------------------------------------------------------
-- 6. 旁路登记（bypass_record）—— IEC 61511-1 §11.5.2 要求
--    "工程级旁路必须限时（典型 ≤ 短期维护），到期必须报警"。
--    这里只存合规登记（who / when / why / expired_at）。
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS bypass_record (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id      INTEGER NOT NULL,
    sif_id          INTEGER NOT NULL,
    bypassed_by     TEXT    NOT NULL DEFAULT '',              -- 旁路操作人
    bypassed_at     TEXT    NOT NULL DEFAULT (datetime('now')),
    reason          TEXT    NOT NULL DEFAULT '',
    planned_restore TEXT    NOT NULL DEFAULT '',              -- 计划复役时间
    restored_at     TEXT    NOT NULL DEFAULT '',
    permit_no       TEXT    NOT NULL DEFAULT '',              -- 工作票号
    approved_by     TEXT    NOT NULL DEFAULT '',              -- 批准人
    FOREIGN KEY (project_id) REFERENCES project(id) ON DELETE CASCADE,
    FOREIGN KEY (sif_id)     REFERENCES sif(id)     ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_bypass_sif     ON bypass_record(sif_id);
CREATE INDEX IF NOT EXISTS idx_bypass_project ON bypass_record(project_id);
CREATE INDEX IF NOT EXISTS idx_bypass_active  ON bypass_record(restored_at);


-- ---------------------------------------------------------------------------
-- 7. 服务工单（service_ticket）—— M2 起真正激活。
--    至少预留结构以利迁移：故障 / 检验 / 维护单 共享一张表，按 kind 区分。
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS service_ticket (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id  INTEGER NOT NULL,
    sif_id      INTEGER,
    instrument_id INTEGER,
    kind        TEXT    NOT NULL DEFAULT 'maintenance',       -- maintenance|inspection|failure|audit
    title       TEXT    NOT NULL DEFAULT '',
    detail      TEXT    NOT NULL DEFAULT '',
    opened_by   TEXT    NOT NULL DEFAULT '',
    opened_at   TEXT    NOT NULL DEFAULT (datetime('now')),
    closed_at   TEXT    NOT NULL DEFAULT '',
    severity    TEXT    NOT NULL DEFAULT 'normal',
    FOREIGN KEY (project_id)    REFERENCES project(id)    ON DELETE CASCADE,
    FOREIGN KEY (sif_id)        REFERENCES sif(id)        ON DELETE SET NULL,
    FOREIGN KEY (instrument_id) REFERENCES instrument(id) ON DELETE SET NULL,
    CHECK (kind IN ('maintenance', 'inspection', 'failure', 'audit')),
    CHECK (severity IN ('normal', 'warning', 'critical'))
);

CREATE INDEX IF NOT EXISTS idx_ticket_project    ON service_ticket(project_id);
CREATE INDEX IF NOT EXISTS idx_ticket_kind       ON service_ticket(kind);
CREATE INDEX IF NOT EXISTS idx_ticket_instrument ON service_ticket(instrument_id);
