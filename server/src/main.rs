//! HTTP 服务入口（在线版；原 Tauri 桌面外壳已移除）
//!
//! 启动：`cargo run`，默认监听 0.0.0.0:8080（可用 PORT 覆盖）。
//! 数据库：`DATABASE_URL`（默认 sqlite://data/studio.db?mode=rwc）。
//!
//! F1 运维破窗子命令（owner 忘记密码时，在服务器本机执行）：
//!   sif-studio-server reset-password --email you@company.com [--password 'newpw123']
//! 不传 --password 时从标准输入读取（本地终端会回显）。
//! 重置后密码为临时密码，用户首次登录会被强制修改。

use std::io::{BufRead, Write};

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    let result = match args.get(1).map(String::as_str) {
        Some("reset-password") => run_reset_password(&args[2..]).await,
        Some("-h") | Some("--help") | Some("help") => {
            print_usage();
            Ok(())
        }
        Some(other) => {
            eprintln!("[sif-studio-server] 未知子命令：{other}\n");
            print_usage();
            std::process::exit(2);
        }
        None => sif_studio_lib::serve().await.map_err(Into::into),
    };
    if let Err(e) = result {
        eprintln!("[sif-studio-server] {e}");
        std::process::exit(1);
    }
}

fn print_usage() {
    eprintln!(
        "SIF Studio / 联锁工坊 服务器\n\
         \n\
         用法：\n\
          sif-studio-server                         启动 HTTP 服务（默认 0.0.0.0:8080）\n\
         服务时可用环境变量：PORT / DATABASE_URL / SIF_TRUST_PROXY /\n\
                         SIF_INVITE_ONLY / SIF_COOKIE_SECURE / SIF_ADMIN_TOKEN\n\
                         SIF_SESSION_TTL_DAYS / SIF_RATE_LIMIT_LOGIN /\n\
                         SIF_RATE_LIMIT_REGISTER / SIF_INVITE_TTL_DEFAULT /\n\
                         SIF_INVITE_TTL_MAX\n\
         \n\
          sif-studio-server reset-password --email <邮箱> [--password <新密码>]\n\
                                           本地重置用户密码（破窗通道，\n\
                                           重置后首次登录须修改密码）"
    );
}

/// 解析 reset-password 参数并执行。错误一律字符串化交给 main 统一打印。
async fn run_reset_password(rest: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let mut email: Option<String> = None;
    let mut password: Option<String> = None;

    let mut iter = rest.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--email" => {
                email = Some(iter.next().ok_or("缺少 --email 的值")?.trim().to_string());
            }
            "--password" => {
                password = Some(iter.next().ok_or("缺少 --password 的值")?.clone());
            }
            other => return Err(format!("未知参数：{other}").into()),
        }
    }

    let email = email.filter(|s| !s.is_empty()).ok_or(
        "必须指定 --email <登录邮箱>，示例：\n  \
         sif-studio-server reset-password --email owner@company.com",
    )?;
    let password = match password.filter(|s| !s.is_empty()) {
        Some(pw) => pw,
        None => {
            // 本地终端会回显；提示运维不要在共享终端执行，并避免把密码放进 shell 历史
            eprint!("请输入 {email} 的新密码（至少 8 个字符，本地终端会回显）：");
            std::io::stderr().flush()?;
            let mut line = String::new();
            std::io::stdin().lock().read_line(&mut line)?;
            line.trim_end_matches(['\r', '\n']).to_string()
        }
    };

    // db::open 会确保目录存在并跑齐迁移（含 006 加列），再执行重置。
    let pool = sif_studio_lib::db::open().await?;
    sif_studio_lib::http::auth::admin_reset_password_cli(&pool, &email, &password).await?;

    println!(
        "已重置 {email} 的密码。\n\
         - 该账号在所有设备上的会话已失效；\n\
         - 下次登录后必须先修改密码才能正常使用。"
    );
    Ok(())
}
