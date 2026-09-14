# M1 数据库架构 & 跨图 SIF 汇总

> 写给后端 / 性能 / 数据校对 review，以及未来可能的 DB schema 迁移。

---

## 1. 设计目标

| 目标 | 落地做法 |
|---|---|
| 用户能"一屏看完整 SIF" | `list_sifs` 单 SQL 聚合返回 detector/final/logic/aux 计数 + 位号 CSV + 涉及图号 |
| 真实仪表台账可查询 | `instrument` 表 + role/sil_target CHECK + 唯一 index on tag |
| 多图多项目下不让"按图堆叠" | `project` 顶层 + `diagram.sif_id` 软链 + `sif_instrument.diagram_id` 标记录入位置 |
| 删除数据不留尾 | FK 全配 CASCADE / SET NULL，on delete 触发 DB 而非应用代码 |
| 跨平台数据迁移 | SQLite 单文件 `studio.db`，复制即迁移 |

---

## 2. v001 7 表

```
project ─┬─ diagram ─┐
         │           │
         └─ sif ─┐    │ (diagram.sif_id 软链 sif)
                │
                └─ sif_instrument ── instrument
                │
                └─ bypass_record   (IEC 61511 §11.5)
                │
                └─ service_ticket  (M2 激活)

CHECK:
- instrument.role         IN ('detector', 'final', 'logic', 'aux')
- instrument.sil_target   IN ('NA', 'A', 'B', 'C', 'D')
- sif.sil_design/verified IN (...)
- sif.demand_mode         IN ('low', 'high')
- diagram.sheet_size      IN ('A0','A1','A2','A3','A4')
- sif_instrument.role     IN (...)
- service_ticket.kind     IN ('maintenance','inspection','failure','audit')
- service_ticket.severity IN ('normal','warning','critical')

UNIQUE:
- instrument(tag)
- project(code)
- diagram(project_id, code)
- sif(project_id, code)
- sif_instrument(sif_id, instrument_id, role, port_index, diagram_id)
```

**索引**（频次由高到低）：
- `instrument.tag / role / kind` —— 台账搜索 + 按角色筛选
- `diagram.project_id / sif_id / (project_id, code)`
- `sif.project_id / (project_id, code)`
- `sif_instrument.sif_id / instrument_id / diagram_id` —— 反查"哪个仪表用了"
- `bypass_record.sif_id / project_id / restored_at` —— "哪些还在旁路中"
- `service_ticket.project_id / kind / instrument_id`

---

## 3. 跨图汇总 SQL（核心）

`list_sifs(project_id?: number)` 一次拿完：

```sql
SELECT
  s.id, s.project_id, p.code AS project_code,
  s.code, s.name, s.description,
  s.sil_design, s.sil_verified, s.demand_mode, s.pfdavg_target, s.proof_interval,

  COUNT(DISTINCT CASE WHEN si.role='detector' THEN si.instrument_id END) AS detector_count,
  COUNT(DISTINCT CASE WHEN si.role='final'    THEN si.instrument_id END) AS final_count,
  COUNT(DISTINCT CASE WHEN si.role='logic'    THEN si.instrument_id END) AS logic_count,
  COUNT(DISTINCT CASE WHEN si.role='aux'      THEN si.instrument_id END) AS aux_count,

  COUNT(DISTINCT si.diagram_id) AS diagram_count,

  GROUP_CONCAT(DISTINCT CASE WHEN si.role='detector' THEN i.tag END) AS detectors_csv,
  GROUP_CONCAT(DISTINCT CASE WHEN si.role='final'    THEN i.tag END) AS finals_csv,
  GROUP_CONCAT(DISTINCT d.code) AS diagrams_csv

FROM sif s
JOIN project p             ON p.id = s.project_id
LEFT JOIN sif_instrument si ON si.sif_id = s.id
LEFT JOIN instrument i      ON i.id = si.instrument_id
LEFT JOIN diagram d         ON d.id = si.diagram_id
[WHERE s.project_id = ?]
GROUP BY s.id
ORDER BY s.code
```

**为什么这么写**：
1. **单条 SQL 拿全部** → 避免 N+1（每个 SIF 再去查仪表、再去查图）
2. **LEFT JOIN** → 没关联的 SIF 也会出现（仪表零的 SIF 仍展示，给用户提醒）
3. **CASE WHEN 配 COUNT(DISTINCT)** → 角色区分的同时保证一个仪表在多图里不重复计数
4. **GROUP_CONCAT 配 DISTINCT** → 去重位号 / 图号拼接；前端拿到 "PT-101,PT-102,LSL-203" 直接 split 使用
5. **GROUP BY s.id** → SQLite 允许非聚合列放宽，这是已知的 GROUP BY 简化，但仍然按 s.id 唯一分组，正确

**为什么 project_id 是可选的**：
- Home 页要看"全部 SIF"
- SifDashboard 顶部筛选允许按项目过滤
- 在 SQL 里只接 `WHERE s.project_id = ?` 一行

---

## 4. CASCADE 拓扑

```
project (删)  ──┬──> diagram       (ON DELETE CASCADE)
                ├──> sif           (ON DELETE CASCADE)
                │       │
                │       ├──> sif_instrument     (ON DELETE CASCADE)
                │       │       │
                │       │       └──> instrument  (??)
                │       ├──> bypass_record      (ON DELETE CASCADE)
                │       └──> service_ticket     (ON DELETE SET NULL)
                │
                └──> bypass_record (ON DELETE CASCADE)
                └──> service_ticket (ON DELETE CASCADE)

instrument (删) ──> sif_instrument (ON DELETE CASCADE)
diagram (删)    ──> diagram.sif_id → NULL (已经在 sif 上没有 FK)
                 [sif_instrument.diagram_id → NULL SET NULL，避免被引]

sif (删)        ──> diagram.sif_id → NULL (ON DELETE SET NULL 让图保留)
              ──> sif_instrument (ON DELETE CASCADE 自动清关联)
              ──> bypass_record  (ON DELETE CASCADE)
              ──> service_ticket (ON DELETE SET NULL)
```

**取舍**：
- 删 diagram → `sif_instrument.diagram_id = NULL`，**关联不能丢**（仪表在该 SIF 还是有用）
- 删 sif → `diagram.sif_id = NULL`，**图保留但显示「未挂 SIF」**
- 删 instrument → **关联 link 一并清**（业务上改仪表 = 重做 SIF）
- 删 project → **一切全清**（项目归档 = 完整地删除）

---

## 5. 性能与限制（v001）

- **journal_mode = WAL**：并发读 + 写不互斥
- **busy_timeout = 5s**：Tauri 命令若同时多个触发不会立刻失败
- **外键全开**：DB 一致性靠 FK，不靠应用码
- 单库预估：仪表 1k 条 + SIF 200 条 + 图 200 张 + link 5k 条 —— 完全单机 SQL < 50 ms

## 6. 已知 v001 不做的事

- 没软删除：删除 = 真删除（v002 再加 `deleted_at`）
- 没 audit log：增删改无历史（v003 加 `trigger` / 单独 audit 表）
- 没版本号：diagram 只存最新一帧 data（M1 没 upsert semantics；v002 加 `revision_no`）
- 没多用户：单文件 SQLite，无并发写（M2 改中央 Postgres）

---

## 7. 写新迁移要注意

1. 文件名 `vXXX_xxx.sql`，`XXX` 单调递增
2. **不要修改历史迁移**——schema 演变走新迁移
3. 用 `CREATE TABLE IF NOT EXISTS` / `CREATE INDEX IF NOT EXISTS` 等幂等 DDL（防止 partial migration 重跑）
4. ALTER TABLE 加列能用；删列 SQLite 3.35+ 支持但建议新表替旧表
5. 大批初始数据用临时 seed migration，标 `seed_only` 注释
