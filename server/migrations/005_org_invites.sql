-- ===========================================================================
-- SIF Studio / 联锁工坊 — v005 迁移：组织邀请（阶段 D1）
--
-- 背景：004 之后每个新注册都开独立组织，同事无法加入同一个组织。
-- 本表存 owner 签发的邀请链接：
--   - token 为 32 字节随机 hex（URL 路径里传递，不存邮箱等个人信息）；
--   - 邀请在过期/吊销前**可多次使用**（团队链接，类似 Slack 邀请）；
--   - 每次接受都写 audit_log，不在这里维护使用计数；
--   - 注册接受邀请走公开端点 /api/auth/register（带 inviteToken），
--     已登录用户接受走 POST /api/org/invites/accept。
-- ===========================================================================

CREATE TABLE IF NOT EXISTS org_invite (
    token       TEXT    PRIMARY KEY,
    org_id      INTEGER NOT NULL REFERENCES org(id)  ON DELETE CASCADE,
    role        TEXT    NOT NULL DEFAULT 'engineer',  -- owner|engineer|viewer
    invited_by  INTEGER NOT NULL REFERENCES user(id) ON DELETE CASCADE,
    created_at  TEXT    NOT NULL DEFAULT (datetime('now')),
    expires_at  TEXT    NOT NULL,                     -- datetime('now','+N days')
    revoked_at  TEXT,                                 -- 非 NULL = 已吊销
    CHECK (role IN ('owner', 'engineer', 'viewer'))
);

CREATE INDEX IF NOT EXISTS idx_invite_org ON org_invite(org_id);
