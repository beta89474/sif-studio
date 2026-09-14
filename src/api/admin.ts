/**
 * 管理端点封装（阶段 C2/C3）
 *
 * - backup/restore 是**服务器操作员**能力（跨所有租户），用静态
 *   X-Admin-Token 头（对应服务端环境变量 SIF_ADMIN_TOKEN）；
 *   服务端未配置令牌时这两个端点返回 404。
 * - import-legacy 是**组织 owner** 能力，只带会话 cookie，不需要令牌。
 *
 * 令牌只活在设置页组件内存里，不写 localStorage。
 */

export class AdminHttpError extends Error {
  kind: string;
  status: number;

  constructor(kind: string, message: string, status: number) {
    super(message);
    this.name = "AdminHttpError";
    this.kind = kind;
    this.status = status;
  }
}

/** POST /api/admin/restore 响应体（与后端 RestoreReport 对应） */
export interface RestoreReport {
  restored: boolean;
  migrationVersion: number;
  tablesCopied: number;
  rowsCopied: Record<string, number>;
}

/** 单表导入计数（与后端 LegacyTableReport 对应） */
export interface LegacyTableReport {
  inserted: number;
  skipped: number;
}

/** POST /api/admin/import-legacy 响应体 */
export interface LegacyReport {
  projects: LegacyTableReport;
  instruments: LegacyTableReport;
  sifs: LegacyTableReport;
  diagrams: LegacyTableReport;
  links: LegacyTableReport;
  bypasses: LegacyTableReport;
  tickets: LegacyTableReport;
  auditLogs: LegacyTableReport;
  /** 找不到归属项目而整体丢弃的行数 */
  orphanRows: number;
}

async function errorOf(res: Response): Promise<AdminHttpError> {
  let kind = "http";
  let message = `${res.status} ${res.statusText}`;
  try {
    const body = (await res.json()) as { kind?: string; message?: string };
    if (body && typeof body === "object") {
      kind = body.kind ?? kind;
      message = body.message ?? message;
    }
  } catch {
    // 错误体非 JSON
  }
  return new AdminHttpError(kind, message, res.status);
}

function multipart(file: File): FormData {
  const form = new FormData();
  form.append("file", file, file.name);
  return form;
}

/** 从 Content-Disposition 里取文件名，取不到用兜底名。 */
function filenameFromDisposition(res: Response, fallback: string): string {
  const m = /filename="?([^"]+)"?/.exec(res.headers.get("content-disposition") ?? "");
  return m && m[1] ? m[1] : fallback;
}

/** 触发浏览器下载一个 Blob。 */
function saveBlob(blob: Blob, filename: string): void {
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = filename;
  document.body.appendChild(a);
  a.click();
  a.remove();
  // 释放推迟到下一轮事件循环，避免下载尚未开始
  setTimeout(() => URL.revokeObjectURL(url), 0);
}

/**
 * 下载整库备份（含所有组织）。令牌错误 → 401；服务端未配置令牌 → 404。
 */
export async function downloadBackup(token: string): Promise<void> {
  const res = await fetch("/api/admin/backup", {
    method: "GET",
    credentials: "same-origin",
    headers: { "x-admin-token": token },
  });
  if (!res.ok) throw await errorOf(res);
  const blob = await res.blob();
  const fallback = `sif-studio-backup-${new Date().toISOString().slice(0, 19).replace(/[:T]/g, "-")}.db`;
  saveBlob(blob, filenameFromDisposition(res, fallback));
}

/**
 * 整库恢复（会覆盖服务端**所有组织**的数据）。
 * 非 SQLite / 版本不一致会返回 4xx，错误文案可直接展示。
 */
export async function restoreBackup(token: string, file: File): Promise<RestoreReport> {
  const res = await fetch("/api/admin/restore", {
    method: "POST",
    credentials: "same-origin",
    headers: { "x-admin-token": token },
    body: multipart(file),
  });
  if (!res.ok) throw await errorOf(res);
  return (await res.json()) as RestoreReport;
}

/**
 * 导入旧 Tauri 桌面版 studio.db 到当前组织（会话身份即 owner 校验）。
 * 可重复执行：按业务唯一键去重，重复数据计入 skipped。
 */
export async function importLegacy(file: File): Promise<LegacyReport> {
  const res = await fetch("/api/admin/import-legacy", {
    method: "POST",
    credentials: "same-origin",
    body: multipart(file),
  });
  if (!res.ok) throw await errorOf(res);
  return (await res.json()) as LegacyReport;
}
