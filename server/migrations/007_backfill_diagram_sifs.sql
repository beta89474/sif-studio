-- ===========================================================================
-- 007_backfill_diagram_sifs.sql
--
-- 业务规则收口：一张联锁逻辑图 = 一个 SIF（diagram.sif_id NOT NULL 语义）。
-- 007 之前的历史图纸 sif_id 可能为 NULL（手工建图不会自动建 SIF），
-- 这会让老项目的「SIF 汇总」空白。本迁移为每张孤儿图补建一个配套 SIF 并回填。
--
-- 编号策略：项目内现有 SIF-nnn 的最大数字编号 + 孤儿图序号（ROW_NUMBER），
-- 与 create_diagram_inner 的 next_sif_code 顺延规则一致；取 MAX 而非 COUNT，
-- 手工编号不连续（如 SIF-900）时也不会撞唯一索引。
--
-- 用 TEMP TABLE 先固化分配结果，再 INSERT / UPDATE：
--   避免 INSERT...SELECT 同表时统计子查询读到语句内的新行导致跳号。
-- ===========================================================================

CREATE TEMP TABLE IF NOT EXISTS _orphan_sif (
    diagram_id INTEGER PRIMARY KEY,
    org_id     INTEGER NOT NULL,
    project_id INTEGER NOT NULL,
    new_code   TEXT    NOT NULL,
    name       TEXT    NOT NULL,
    dcode      TEXT    NOT NULL
);

INSERT INTO _orphan_sif (diagram_id, org_id, project_id, new_code, name, dcode)
SELECT d.id,
       d.org_id,
       d.project_id,
       'SIF-' || printf('%03d',
           COALESCE((
               SELECT MAX(CAST(substr(cs.code, 5) AS INTEGER))
               FROM sif cs
               WHERE cs.org_id = d.org_id
                 AND cs.project_id = d.project_id
                 AND cs.code GLOB 'SIF-[0-9][0-9][0-9]'
           ), 0)
           + ROW_NUMBER() OVER (PARTITION BY d.org_id, d.project_id ORDER BY d.id)
       ) AS new_code,
       d.name,
       d.code
FROM diagram d
WHERE d.sif_id IS NULL;

-- 补建 SIF
INSERT INTO sif (org_id, project_id, code, name, description,
                 sil_design, sil_verified, demand_mode, pfdavg_target, proof_interval)
SELECT org_id,
       project_id,
       new_code,
       name,
       '随联锁图 ' || dcode || ' 自动补建',
       'NA', 'NA', 'low', NULL, 12
FROM _orphan_sif;

-- 回填图 → SIF 关联
UPDATE diagram
SET sif_id = (
    SELECT s.id
    FROM sif s
    JOIN _orphan_sif o
      ON o.org_id = s.org_id
     AND o.project_id = s.project_id
     AND o.new_code = s.code
    WHERE o.diagram_id = diagram.id
)
WHERE sif_id IS NULL;

DROP TABLE _orphan_sif;
