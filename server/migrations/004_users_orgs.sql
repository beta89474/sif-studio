-- ===========================================================================
-- SIF Studio / 联锁工坊 — v004 迁移：在线多租户（组织 / 用户 / 会话）
--
-- 设计要点（与 003 同一工程纪律）：
--   - 历史迁移 001-003 冻结不改，本文件只做增量。
--   - org_id 列用 ALTER ADD（SQLite 无法在 ADD COLUMN 上直接 NOT NULL），
--     存量行回填默认组织后，应用层保证新写入一律非空并强制 WHERE 过滤。
--   - 存量回填：建 id=1 默认组织，8 张业务表全部挂到该组织；
--     第一个注册用户在注册逻辑里被认领为该组织 owner（见 http/auth.rs），
--     全新部署（无存量数据）时该组织会被注册流程直接改名使用。
--   - 审计合规：audit_log 直接挂 org_id（审计中心按组织过滤，不经 project 派生）。
-- ===========================================================================

-- ─── 1. 组织 / 用户 / 成员 / 会话 ──────────────────────────────────────────

CREATE TABLE IF NOT EXISTS org (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    name        TEXT    NOT NULL,
    created_at  TEXT    NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS user (
    id             INTEGER PRIMARY KEY AUTOINCREMENT,
    email          TEXT    NOT NULL,                        -- 登录邮箱（全局唯一，小写比较）
    password_hash  TEXT    NOT NULL,                        -- argon2 PHC 字符串
    display_name   TEXT    NOT NULL DEFAULT '',
    created_at     TEXT    NOT NULL DEFAULT (datetime('now'))
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_user_email ON user(lower(email));

CREATE TABLE IF NOT EXISTS org_member (
    org_id     INTEGER NOT NULL REFERENCES org(id)  ON DELETE CASCADE,
    user_id    INTEGER NOT NULL REFERENCES user(id) ON DELETE CASCADE,
    role       TEXT    NOT NULL DEFAULT 'engineer',          -- owner|engineer|viewer
    created_at TEXT    NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (org_id, user_id),
    CHECK (role IN ('owner', 'engineer', 'viewer'))
);
CREATE INDEX IF NOT EXISTS idx_member_user ON org_member(user_id);

-- 服务端会话表（Cookie 里只存不透明随机 token；HttpOnly + SameSite=Lax）
CREATE TABLE IF NOT EXISTS session (
    token        TEXT    PRIMARY KEY,                        -- 32 字节随机数 hex
    user_id      INTEGER NOT NULL REFERENCES user(id) ON DELETE CASCADE,
    org_id       INTEGER NOT NULL REFERENCES org(id)  ON DELETE CASCADE,
    created_at   TEXT    NOT NULL DEFAULT (datetime('now')),
    last_seen_at TEXT    NOT NULL DEFAULT (datetime('now')),
    expires_at   TEXT    NOT NULL,                           -- datetime('now','+30 days') 风格 UTC
    user_agent   TEXT    NOT NULL DEFAULT '',
    ip           TEXT    NOT NULL DEFAULT ''
);
CREATE INDEX IF NOT EXISTS idx_session_user ON session(user_id);

-- ─── 2. 默认组织（存量数据归属；首个注册用户认领） ─────────────────────────
INSERT INTO org (id, name)
SELECT 1, '默认组织（存量数据）'
WHERE NOT EXISTS (SELECT 1 FROM org WHERE id = 1);
-- 显式插入 id=1 后 sqlite_sequence 记录为 1，后续注册的新组织自增从 2 开始。

-- ─── 3. 业务表加 org_id ────────────────────────────────────────────────────
-- project
ALTER TABLE project ADD COLUMN org_id INTEGER REFERENCES org(id) ON DELETE CASCADE;
UPDATE project SET org_id = 1 WHERE org_id IS NULL;
DROP INDEX IF EXISTS idx_project_code;
CREATE UNIQUE INDEX IF NOT EXISTS idx_project_org_code ON project(org_id, code);
CREATE INDEX IF NOT EXISTS idx_project_org ON project(org_id);

-- instrument
ALTER TABLE instrument ADD COLUMN org_id INTEGER REFERENCES org(id) ON DELETE CASCADE;
UPDATE instrument SET org_id = 1 WHERE org_id IS NULL;
CREATE INDEX IF NOT EXISTS idx_instrument_org ON instrument(org_id);

-- diagram（同时加乐观锁版本号）
ALTER TABLE diagram ADD COLUMN org_id INTEGER REFERENCES org(id) ON DELETE CASCADE;
UPDATE diagram SET org_id = 1 WHERE org_id IS NULL;
ALTER TABLE diagram ADD COLUMN version INTEGER NOT NULL DEFAULT 0;
CREATE INDEX IF NOT EXISTS idx_diagram_org ON diagram(org_id);

-- sif
ALTER TABLE sif ADD COLUMN org_id INTEGER REFERENCES org(id) ON DELETE CASCADE;
UPDATE sif SET org_id = 1 WHERE org_id IS NULL;
CREATE INDEX IF NOT EXISTS idx_sif_org ON sif(org_id);

-- sif_instrument
ALTER TABLE sif_instrument ADD COLUMN org_id INTEGER REFERENCES org(id) ON DELETE CASCADE;
UPDATE sif_instrument SET org_id = 1 WHERE org_id IS NULL;
CREATE INDEX IF NOT EXISTS idx_link_org ON sif_instrument(org_id);

-- bypass_record
ALTER TABLE bypass_record ADD COLUMN org_id INTEGER REFERENCES org(id) ON DELETE CASCADE;
UPDATE bypass_record SET org_id = 1 WHERE org_id IS NULL;
CREATE INDEX IF NOT EXISTS idx_bypass_org ON bypass_record(org_id);

-- service_ticket
ALTER TABLE service_ticket ADD COLUMN org_id INTEGER REFERENCES org(id) ON DELETE CASCADE;
UPDATE service_ticket SET org_id = 1 WHERE org_id IS NULL;
CREATE INDEX IF NOT EXISTS idx_ticket_org ON service_ticket(org_id);

-- audit_log（审计中心直接按 org 过滤）
ALTER TABLE audit_log ADD COLUMN org_id INTEGER REFERENCES org(id) ON DELETE CASCADE;
UPDATE audit_log SET org_id = 1 WHERE org_id IS NULL;
CREATE INDEX IF NOT EXISTS idx_audit_org ON audit_log(org_id);
