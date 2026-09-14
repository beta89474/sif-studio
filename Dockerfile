# syntax=docker/dockerfile:1
#
# SIF Studio（联锁工坊）多阶段镜像
#   1. node:20   构建前端 Vite 产物 dist/
#   2. rust:slim release 编译后端（sqlx 使用 bundled SQLite，仅需 cc）
#   3. debian-slim 运行（非 root，/data 持久卷）
#
# 构建上下文必须是仓库根目录（前端 package.json 与 server/ 同级）：
#   docker build -t sif-studio:latest .

# ---------------------------------------------------------------------------
# 阶段 1：前端
# ---------------------------------------------------------------------------
FROM node:20-bookworm-slim AS web
WORKDIR /web

# 先装依赖（利用层缓存；lockfile 不变则命中缓存）
COPY package.json package-lock.json ./
RUN npm ci --no-audit --no-fund

# 再拷源码构建。注意：src/assets/fonts/SimHei-subset.ttf 不入库但存在于
# 本地工作树，.dockerignore 不能排除它，否则 prebuild 字体会重新生成失败。
COPY . .
RUN npm run build

# ---------------------------------------------------------------------------
# 阶段 2：后端
# ---------------------------------------------------------------------------
FROM rust:1.85-slim-bookworm AS server

# bundled libsqlite3 由 cc 编译 amalgamation，需要 C 工具链
RUN apt-get update \
    && apt-get install -y --no-install-recommends build-essential \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /build
# 注意：不要把仓库根的 .cargo/config.toml（Windows MSVC 专用）拷进来——
# 这里 COPY 的是 server/ 子目录，配置文件在其父级，天然隔离。
COPY server/ ./
RUN cargo build --release --locked
# 顺手自检：二进制能起来并打印 --version 之类（无该参数则至少确认可执行）
RUN cp target/release/sif-studio-server /tmp/sif-studio-server

# ---------------------------------------------------------------------------
# 阶段 3：运行时
# ---------------------------------------------------------------------------
FROM debian:bookworm-slim AS runtime

# 非 root 运行用户 + 目录
RUN groupadd --system --gid 10001 sif \
    && useradd --system --uid 10001 --gid 10001 --home-dir /app sif \
    && mkdir -p /app/web/dist /app/web/public /data \
    && chown -R sif:sif /app /data

COPY --from=server --chown=sif:sif /tmp/sif-studio-server /app/sif-studio-server
COPY --from=web --chown=sif:sif /web/dist /app/web/dist
COPY --chown=sif:sif public/ /app/web/public/

USER sif
WORKDIR /app

# 静态资源用绝对路径，避免相对 ../dist 受工作目录影响；
# 数据库固定到持久卷 /data（mode=rwc：不存在则首启自动建库 + 跑迁移）。
ENV SIF_WEB_DIST=/app/web/dist \
    SIF_WEB_PUBLIC=/app/web/public \
    DATABASE_URL=sqlite:///data/studio.db?mode=rwc \
    PORT=8080

EXPOSE 8080
VOLUME ["/data"]

ENTRYPOINT ["/app/sif-studio-server"]
