/**
 * 在线版 RPC 适配层
 *
 * 与 Tauri 桌面端 `invoke(cmd, args?)` 保持同签名，业务代码（stores / views）
 * 只需把 import 来源从 `@tauri-apps/api/core` 改到这里，其余零改动。
 *
 * 协议：
 *   POST /api/rpc  body: { cmd: string, args: object }
 *   成功：200 + 任意 JSON（后端已把 snake_case 键递归转为 camelCase）
 *   失败：非 2xx + { kind, message }（与桌面端 reject 的错误形状一致）
 *
 * 阶段 B 会在此层注入会话凭证与 401 跳登录。
 */

/** 后端错误体形状（AppError → {kind,message}），与桌面版历史协议一致 */
export class RpcError extends Error {
  kind: string;
  status: number;

  constructor(kind: string, message: string, status: number) {
    super(message);
    this.name = "RpcError";
    this.kind = kind;
    this.status = status;
  }
}

export async function invoke<T>(
  cmd: string,
  args?: Record<string, unknown>,
): Promise<T> {
  let res: Response;
  try {
    res = await fetch("/api/rpc", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      // 同域自动带 Cookie；阶段 B 的会话 Cookie 依赖此项
      credentials: "same-origin",
      body: JSON.stringify({ cmd, args: args ?? {} }),
    });
  } catch (networkErr) {
    // fetch 拒绝（服务未启动 / 断网）—— 归一为 RpcError，UI 分支不变
    throw new RpcError(
      "network",
      networkErr instanceof Error ? networkErr.message : String(networkErr),
      0,
    );
  }

  if (res.status === 401) {
    // 未登录 / 会话过期：广播事件（App.vue 监听后清登录态并跳登录页），
    // 再抛 unauthorized 交给调用点的既有错误分支。
    window.dispatchEvent(new CustomEvent("sif:unauthorized"));
    throw new RpcError("unauthorized", "未登录或会话已过期", res.status);
  }

  if (!res.ok) {
    let kind = "http";
    let message = `${res.status} ${res.statusText}`;
    try {
      const body = (await res.json()) as { kind?: string; message?: string };
      if (body && typeof body === "object") {
        kind = body.kind ?? kind;
        message = body.message ?? message;
      }
    } catch {
      // 错误体非 JSON（反向代理 502 等）—— 保留 HTTP 状态文案
    }
    throw new RpcError(kind, message, res.status);
  }

  return (await res.json()) as T;
}
