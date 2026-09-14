# Rust 工具链在 Windows 上的安装手册

> **本 SIF Studio 项目需要 Rust 工具链才能跑后端命令**（Tauri 2 + sqlx）。本档面向
> 你将来换机器或重装系统时使用。当前 session 里我们已经装好了，照"验证当前安装"节
> 确认即可。

---

## 0. 哪些组件必须装

| 组件 | 作用 | 必装？ |
|---|---|---|
| **Rust toolchain (stable)** | `rustc` 编译器 + `cargo` 包管理器 + `rustup` 工具链管理器 | ✅ 必装 |
| **MSVC 链接工具**（Tauri 2 用）| C/C++ 编译器 + Windows SDK + linker | ✅ 必装（Tauri 2 必需 MSVC；GNU 理论上能跑但坑多） |
| **WebView2 Runtime** | Tauri 2 渲染前端网页视图（Win11 默认带；Win10 需装） | ✅ 必装 |
| **Git for Windows** | `cargo` 拉 crates.io 索引；`cargo install` 也要 | ✅ 必装 |
| **rust-analyzer**（可选）| IDE 编辑器提示用，跑 / 写代码不需要 | 可选 |

---

## 1. 装 Rust 工具链（30 秒）

### 方案 A —— `rustup-init.exe`（推荐，标准做法）

1. 浏览器打开 <https://rustup.rs/> → 点 **「rustup-init.exe」** 下载
   （或 PowerShell 里跑 `irm https://win.rustup.rs/install.ps1 | iex`）
2. 双击 `rustup-init.exe`，按提示：
   - 默认选项（回车直到 **Proceed with installation (default):** 后回车）
   - 装完会看到 `Rust is installed now. Great!`
3. **关闭再重开** PowerShell / Git Bash，让新 PATH 生效
4. 验证：

   ```bash
   rustc --version        # 应输出 rustc 1.xx.x (xxxxxx xxxx-xx-xx)
   cargo --version        # 应输出 cargo 1.xx.x (xxxxxx xxxx-xx-xx)
   rustup show            # 应显示 active toolchain: stable-x86_64-pc-windows-msvc
   ```

### 方案 B —— `winget`（Win10/11 自带）

```powershell
winget install Rustlang.Rustup
```

之后验证同上。

### 方案 C —— `chocolatey`（少见用法）

```powershell
choco install rustup.install
```

之后验证同上。

---

## 2. 装 MSVC 链接工具（Tauri 2 必需）

Tauri 2 要把 Rust 代码链接成 Windows `.exe` + `.dll`，需要 **MSVC 工具链**（不是 MinGW）。
最小集合大约 1.8 GB：

1. 浏览器打开 <https://visualstudio.microsoft.com/zh-hans/downloads/>
2. 下拉到 **「所有下载 → Tools for Visual Studio → Build Tools for Visual Studio 2022」**
   - 文件名：`vs_BuildTools.exe`（约 1 MB，是个在线安装器）
3. 双击运行，按"工作负载"勾：
   - ☑ **「C++ 桌面开发」**（默认子项全选即可）
     - 关键子项：MSVC v143、Windows 11 SDK、CMake tools for Windows
4. 装完 → **重启** → 重新打开 PowerShell 验证：

   ```bash
   where.exe link.exe        # 应输出 C:\Program Files (x86)\Microsoft Visual Studio\...
   where.exe cl.exe          # 应输出 ...\VC\Tools\MSVC\... 路径
   ```

   > 如果 `where.exe link.exe` 报"找不到"——说明 Build Tools 没装或 PATH 没刷新。
   > 试 `Import-Module "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\Common7\Tools\Microsoft.VisualStudio.DevShell.psm1"; Enter-VsDevShell -SkipAutomaticLocation` 手动激活。

---

## 3. 装 WebView2 Runtime（Win10 要；Win11 自带）

Tauri 2 用 WebView2 渲染前端。检查：

```powershell
Get-AppxPackage -Name 'Microsoft.WebView2*' | Select-Object Name, Version
```

或直接打开 Edge 浏览器 → 右上角"…" → **「关于」** → 看 Chromium 内核版本 ≥ 100 即有 WebView2。

如果没装，去 <https://developer.microsoft.com/zh-cn/microsoft-edge/webview2/> 下 **「Evergreen Standalone Installer」**。

---

## 4. 装 Git for Windows（`cargo` 拉 crates 用）

如果之前没装：

```powershell
winget install Git.Git
```

验证：`git --version`。

---

## 5. SIF Studio 项目验证（必跑）

装完 4 节前置后，进项目目录：

```bash
cd sif-studio

# 1) 拉前端依赖
npm install                                # 约 1-2 分钟

# 2) 跑 Rust 测试（验证 SQL 迁移 + 后端命令）
cd src-tauri
cargo test --tests                        # 约 2-5 分钟；首次要下 400+ crates

# 3) 跑桌面开发模式
cd ..
npm run tauri:dev                          # 首次启动会编译 Tauri 2，约 3-7 分钟
```

期望结果：
- `cargo test` 至少 7 个测试 `ok`
- `npm run tauri:dev` 弹出 **SIF Studio 窗口**（左侧栏有 "首页 / 仪表台账 / SIF 汇总 / 项目 / 联锁图"）

---

## 6. 排错速查

| 报 错 | 原 因 | 解 决 |
|---|---|---|
| `linker 'link.exe' not found` | MSVC Build Tools 没装 | 看 §2 |
| `error: linker 'link.exe' not found`, 但 §2 已装 | PATH 没刷新 | 装完后**重启**，或用 `Enter-VsDevShell` 手动激活 |
| `failed to run custom build command for openssl-sys` | 缺 OpenSSL。**Tauri 2 + sqlx 不用**，应不会出。出了说明 Cargo.lock 锁了旧版本 | `cargo update -p openssl-sys` |
| `WebView2Loader.dll not found` | WebView2 没装 | 看 §3 |
| `error: linker 'cc.exe' not found` | 错装了 GNU 工具链 (`stable-x86_64-pc-windows-gnu`) | `rustup toolchain install stable-x86_64-pc-windows-msvc --force` |
| 卡 `   Compiling` 长时间无输出 | 第一次编译正常；可观察 `.cargo\registry\cache` 大小在涨 | 耐心等（首次 3-7 分钟） |
| 端口 1420 被占用（Tauri dev） | 之前残留 | `taskkill /F /IM sif-studio.exe` |

---

## 7. 卸载

```bash
rustup self uninstall     # 卸载 Rust
# 然后手动删：
Remove-Item -Recurse "$env:USERPROFILE\.cargo"
Remove-Item -Recurse "$env:USERPROFILE\.rustup"
# Visual Studio Build Tools 用其内置卸载器
```

---

## 附录：什么时候装 rust-analyzer

只有在你用 **VS Code + rust-analyzer 扩展** 写 Rust 时才需要。它是 `rustup` 的一个 component：

```bash
rustup component add rust-analyzer
```

**跑 / 构建 SIF Studio 不需要 rust-analyzer**——`cargo` + `rustc` 就够。
