/**
 * 认证端点封装（阶段 B）
 *
 * 与业务 RPC 分开：/api/auth/* 是普通 REST，不走 /api/rpc 分发器。
 * Cookie 会话（sif_session）由浏览器自动携带（credentials: same-origin）。
 */

export class AuthHttpError extends Error {
  kind: string;
  status: number;

  constructor(kind: string, message: string, status: number) {
    super(message);
    this.name = "AuthHttpError";
    this.kind = kind;
    this.status = status;
  }
}

/** /api/auth/me 的响应体（camelCase，与后端 MeResp 对应） */
export interface OrgMembership {
  orgId: number;
  orgName: string;
  role: string;
  current: boolean;
}

export interface MeInfo {
  userId: number;
  email: string;
  displayName: string;
  orgId: number;
  orgName: string;
  role: string;
  /** 用户所属的全部组织（E4 切换器） */
  orgs: OrgMembership[];
  /** F2：管理员重置过密码，必须先修改自己的密码才能继续使用 */
  mustChangePassword: boolean;
}

/** GET /api/auth/sessions 的单条会话 */
export interface SessionInfo {
  orgId: number;
  orgName: string;
  createdAt: string;
  lastSeenAt: string;
  expiresAt: string;
  userAgent: string;
  ip: string;
  current: boolean;
}

export interface RegisterInput {
  email: string;
  password: string;
  displayName?: string;
  orgName?: string;
  /** 邀请 token：带上后注册即加入邀请所属组织，不再创建新组织（D1） */
  inviteToken?: string;
}

async function errorOf(res: Response): Promise<AuthHttpError> {
  let kind = "http";
  let message = `${res.status} ${res.statusText}`;
  try {
    const body = (await res.json()) as { kind?: string; message?: string };
    if (body && typeof body === "object") {
      kind = body.kind ?? kind;
      message = body.message ?? message;
    }
  } catch {
    // 错误体非 JSON（代理 502 等）—— 保留状态码文案
  }
  return new AuthHttpError(kind, message, res.status);
}

async function jsonOrThrow<T>(res: Response): Promise<T> {
  if (!res.ok) throw await errorOf(res);
  return (await res.json()) as T;
}

export async function me(): Promise<MeInfo> {
  const res = await fetch("/api/auth/me", {
    credentials: "same-origin",
  });
  return jsonOrThrow<MeInfo>(res);
}

export async function login(email: string, password: string): Promise<MeInfo> {
  const res = await fetch("/api/auth/login", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    credentials: "same-origin",
    body: JSON.stringify({ email, password }),
  });
  return jsonOrThrow<MeInfo>(res);
}

export async function register(input: RegisterInput): Promise<MeInfo> {
  const res = await fetch("/api/auth/register", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    credentials: "same-origin",
    body: JSON.stringify({
      email: input.email,
      password: input.password,
      displayName: input.displayName ?? "",
      orgName: input.orgName ?? "",
      inviteToken: input.inviteToken ?? null,
    }),
  });
  return jsonOrThrow<MeInfo>(res);
}

export async function logout(): Promise<void> {
  const res = await fetch("/api/auth/logout", {
    method: "POST",
    credentials: "same-origin",
  });
  if (!res.ok) throw await errorOf(res);
}

// --- E3 自助账号 ----------------------------------------------------------

export async function changePassword(
  oldPassword: string,
  newPassword: string,
): Promise<void> {
  const res = await fetch("/api/auth/change-password", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    credentials: "same-origin",
    body: JSON.stringify({ oldPassword, newPassword }),
  });
  if (!res.ok) throw await errorOf(res);
}

/** 更新显示名；返回更新后的完整 MeInfo */
export async function updateProfile(displayName: string): Promise<MeInfo> {
  const res = await fetch("/api/auth/profile", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    credentials: "same-origin",
    body: JSON.stringify({ displayName }),
  });
  return jsonOrThrow<MeInfo>(res);
}

// --- E4 会话管理 / 组织切换 ------------------------------------------------

export async function listSessions(): Promise<SessionInfo[]> {
  const res = await fetch("/api/auth/sessions", { credentials: "same-origin" });
  return jsonOrThrow<SessionInfo[]>(res);
}

/** 退出其它设备；返回被撤销的会话数 */
export async function logoutOtherSessions(): Promise<number> {
  const res = await fetch("/api/auth/sessions/logout-others", {
    method: "POST",
    credentials: "same-origin",
  });
  const body = await jsonOrThrow<{ ok: boolean; revoked: number }>(res);
  return body.revoked;
}

/** 切换当前会话到目标组织；后端轮换 cookie 并返回新的 MeInfo */
export async function switchOrg(orgId: number): Promise<MeInfo> {
  const res = await fetch("/api/auth/switch-org", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    credentials: "same-origin",
    body: JSON.stringify({ orgId }),
  });
  return jsonOrThrow<MeInfo>(res);
}
