-- ===========================================================================
-- 008_alarm_ledger.sql
--
-- 报警台账（alarm_ledger）—— 过程报警主数据，依据 ISA-18.2 / EEMUA-191 报警管理。
--
-- 业务边界：
--   - 报警台账与联锁逻辑图 / SIF 不关联（报警是 DCS/BPCS 层的运行告警，
--     不参与安全仪表功能回路）；
--   - 但报警通常挂在某台仪表上（如 PT-201 的高高报 PAHH-201），
--     所以保留可空 instrument_id 软关联：仪表删除时报警保留（SET NULL），
--     不做级联，避免删仪表丢报警记录；
--   - 强项目隔离：project_id NOT NULL，项目内 tag 唯一（与仪表台账一致）。
--
-- alarm_type 报警类型：
--   HH 高高报 / H 高报 / LL 低低报 / L 低报
--   DEV 偏差报警 / RATE 变化率报警 / DISC 断线/故障 / OTHER 其他
-- priority 优先级（ISA-18.2）：critical | high | medium | low
-- category 类别：process 工艺 | equipment 设备 | safety 安全
-- status 当前状态：normal 正常 | active 报警中 | bypassed 旁路 | shelved 搁置
-- ===========================================================================

CREATE TABLE IF NOT EXISTS alarm_ledger (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    org_id          INTEGER NOT NULL REFERENCES org(id) ON DELETE CASCADE,
    project_id      INTEGER NOT NULL REFERENCES project(id) ON DELETE CASCADE,
    tag             TEXT    NOT NULL,                           -- 报警位号（PAHH-201）
    instrument_id   INTEGER REFERENCES instrument(id) ON DELETE SET NULL, -- 可选关联仪表
    description     TEXT    NOT NULL DEFAULT '',                -- 报警说明
    alarm_type      TEXT    NOT NULL DEFAULT 'H',
    priority        TEXT    NOT NULL DEFAULT 'medium',
    category        TEXT    NOT NULL DEFAULT 'process',
    setpoint        REAL,                                       -- 报警设定值
    unit            TEXT    NOT NULL DEFAULT '',
    deadband        REAL,                                       -- 报警死区/回差
    delay_seconds   INTEGER NOT NULL DEFAULT 0,                 -- 报警延时（秒）
    status          TEXT    NOT NULL DEFAULT 'normal',
    response_action TEXT    NOT NULL DEFAULT '',                -- 操作员响应动作
    notes           TEXT    NOT NULL DEFAULT '',
    created_at      TEXT    NOT NULL DEFAULT (datetime('now')),
    updated_at      TEXT    NOT NULL DEFAULT (datetime('now')),
    CHECK (alarm_type IN ('HH','H','LL','L','DEV','RATE','DISC','OTHER')),
    CHECK (priority   IN ('critical','high','medium','low')),
    CHECK (category   IN ('process','equipment','safety')),
    CHECK (status     IN ('normal','active','bypassed','shelved'))
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_alarm_project_tag
    ON alarm_ledger(project_id, tag);
CREATE INDEX IF NOT EXISTS idx_alarm_org        ON alarm_ledger(org_id);
CREATE INDEX IF NOT EXISTS idx_alarm_project    ON alarm_ledger(project_id);
CREATE INDEX IF NOT EXISTS idx_alarm_instrument ON alarm_ledger(instrument_id);
CREATE INDEX IF NOT EXISTS idx_alarm_priority   ON alarm_ledger(priority);
