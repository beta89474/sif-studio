/**
 * 组织成员 / 邀请端点封装（阶段 D1）
 *
 * - 邀请链接是团队链接：一个 token 可被多人使用，直到过期或被吊销。
 * - org_id 从不出现在请求里：一律由会话（或邀请记录）决定。
 * - 错误形态与 auth.ts 同款：{ kind, message } + HTTP 状态码。
 */

export class OrgHttpError extends Error {
  kind: string;
  status: number;

  constructor(kind: string, message: string, status: number) {
    super(message);
    this.name = "OrgHttpError";
    this.kind = kind;
    this.status = status;
  }
}

/** GET /api/auth/invite 的公开预览体 */
export interface InvitePreview {
  orgName: string;
  role: string;
  /** UTC 时间字符串 */
  expiresAt: string;
}

/** 邀请记录（owner 视角） */
export interface Invite {
  token: string;
  orgId: number;
  role: string;
  invitedBy: number;
  createdAt: string;
  expiresAt: string;
  revokedAt: string | null;
}

/** 成员行 */
export interface Member {
  userId: number;
  email: string;
  displayName: string;
  role: string;
  createdAt: string;
}

export type OrgRole = "owner" | "engineer" | "viewer";

async function errorOf(res: Response): Promise<OrgHttpError> {
  let kind = "http";
  let message = `${res.status} ${res.statusText}`;
  try {
    const body = (await res.json()) as { kind?: string; message?: string };
    if (body && typeof body === "object") {
      kind = body.kind ?? kind;
      message = body.message ?? message;
    }
  } catch {
    // 非 JSON 错误体 —— 保留状态码文案
  }
  return new OrgHttpError(kind, message, res.status);
}

async function jsonOrThrow<T>(res: Response): Promise<T> {
  if (!res.ok) throw await errorOf(res);
  return (await res.json()) as T;
}

/** 公开：注册页凭 token 预览邀请（404 = 不存在/已吊销/已过期） */
export async function invitePreview(token: string): Promise<InvitePreview> {
  const res = await fetch(
    `/api/auth/invite?token=${encodeURIComponent(token)}`,
    { credentials: "same-origin" },
  );
  return jsonOrThrow<InvitePreview>(res);
}

/** owner：签发团队邀请链接（默认 engineer / 7 天有效） */
export async function createInvite(
  role: OrgRole = "engineer",
  ttlDays?: number,
): Promise<Invite> {
  const res = await fetch("/api/org/invites", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    credentials: "same-origin",
    body: JSON.stringify({ role, ttlDays }),
  });
  return jsonOrThrow<Invite>(res);
}

export async function listInvites(): Promise<Invite[]> {
  const res = await fetch("/api/org/invites", { credentials: "same-origin" });
  return jsonOrThrow<Invite[]>(res);
}

export async function revokeInvite(token: string): Promise<void> {
  const res = await fetch(
    `/api/org/invites/${encodeURIComponent(token)}`,
    { method: "DELETE", credentials: "same-origin" },
  );
  if (!res.ok) throw await errorOf(res);
}

/** 已登录用户凭邀请加入新组织（加入后需重新登录进入新组织） */
export async function acceptInvite(token: string): Promise<{ orgName: string }> {
  const res = await fetch("/api/org/invite/accept", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    credentials: "same-origin",
    body: JSON.stringify({ token }),
  });
  return jsonOrThrow<{ orgName: string }>(res);
}

export async function listMembers(): Promise<Member[]> {
  const res = await fetch("/api/org/members", { credentials: "same-origin" });
  return jsonOrThrow<Member[]>(res);
}

export async function setMemberRole(
  userId: number,
  role: OrgRole,
): Promise<void> {
  const res = await fetch(`/api/org/members/${userId}/role`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    credentials: "same-origin",
    body: JSON.stringify({ role }),
  });
  if (!res.ok) throw await errorOf(res);
}

export async function removeMember(userId: number): Promise<void> {
  const res = await fetch(`/api/org/members/${userId}`, {
    method: "DELETE",
    credentials: "same-origin",
  });
  if (!res.ok) throw await errorOf(res);
}

/** E3：owner 为成员重置密码（重置后该成员所有设备需重新登录） */
export async function resetMemberPassword(
  userId: number,
  newPassword: string,
): Promise<void> {
  const res = await fetch(`/api/org/members/${userId}/reset-password`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    credentials: "same-origin",
    body: JSON.stringify({ newPassword }),
  });
  if (!res.ok) throw await errorOf(res);
}

/** 拼邀请注册链接（hash 路由：#/register?invite=<token>） */
export function inviteUrl(token: string): string {
  const { origin, pathname, search } = window.location;
  return `${origin}${pathname}${search}#/register?invite=${encodeURIComponent(token)}`;
}

/** 角色中文展示 */
export function roleLabel(role: string): string {
  if (role === "owner") return "所有者 OWNER";
  if (role === "engineer") return "工程师 ENGINEER";
  if (role === "viewer") return "只读 VIEWER";
  return role;
}
