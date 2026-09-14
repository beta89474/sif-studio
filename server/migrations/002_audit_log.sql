-- ===========================================================================
-- SIF Studio / 联锁工坊 — v002 迁移：审计日志
--
-- 触发场景：
--   M2.1  仪表批量导入（导入批次 + 失败行）
--   M2.2  旁路授权（bypass 登记 + 审批流）
--   后续  仪表/SIF 修改历史（who / when / old → new）
--
-- 设计原则：
--   - 单条 append-only 写入，**不修改历史**（合规审计铁律）
--   - payload_json 装前后快照 / 导入摘要 / 任意上下文
--   - target_id 可空 = 整批操作（导入 N 条）
--   - actor 留默认 '' 兼容自动化/匿名场景
-- ===========================================================================

CREATE TABLE IF NOT EXISTS audit_log (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    ts            TEXT    NOT NULL DEFAULT (datetime('now')),
    actor         TEXT    NOT NULL DEFAULT '',
    action        TEXT    NOT NULL,                          -- create|update|delete|import|export|bypass|approve|restore
    target_table  TEXT    NOT NULL,                          -- instrument|sif|project|diagram|...
    target_id     INTEGER,                                   -- 关联行 ID；批量时为批次 ID
    payload_json  TEXT    NOT NULL DEFAULT '{}',             -- 上下文/前后快照
    note          TEXT    NOT NULL DEFAULT ''
);

CREATE INDEX IF NOT EXISTS idx_audit_ts            ON audit_log(ts DESC);
CREATE INDEX IF NOT EXISTS idx_audit_target        ON audit_log(target_table, target_id);
CREATE INDEX IF NOT EXISTS idx_audit_action        ON audit_log(action);
CREATE INDEX IF NOT EXISTS idx_audit_actor         ON audit_log(actor);
