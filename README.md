# ZakoAI Proxy

简体中文 · [English](README.en.md) · [安全说明](SECURITY.md) · [API 契约](API_CONTRACT.md)

基于 Rust 的自托管 Responses / Anthropic Messages API 中转，提供适配 PC 和移动端的管理控制台、SQLite 持久化和 Docker Compose 部署。侧边栏包含仪表盘、分组管理、账号管理、密钥管理和系统设置。

## ⚠️ 免责声明 · Disclaimer

### 简体中文

> [!WARNING]
> **严禁将本项目用于任何违法违纪活动。**

本项目仅供合法、合规的学习、研究和在你本人有权操作的环境里使用。包括但不限于：未经授权的入侵、攻击、渗透、窃取数据；诈骗、洗钱、传播恶意程序；生成或传播违法有害内容；侵犯他人隐私、知识产权和其他合法权益；违反所在地法律法规或第三方平台、模型服务的使用条款。

使用者须自行遵守所在国家和地区的法律法规，以及相关软件和服务的使用条款。使用本项目产生的一切行为和后果，由使用者自行承担，与项目作者及贡献者无关。如果你不同意以上内容，请不要下载、安装或使用本项目。

### English

> [!WARNING]
> **This project must NOT be used for any illegal or unlawful activity.**

It is provided only for lawful and compliant learning, research, and use in environments you are authorized to operate. Prohibited uses include, without limitation: unauthorized intrusion, attacks, penetration, or data theft; fraud, money laundering, or distributing malware; generating or spreading illegal or harmful content; infringing others' privacy, intellectual property, or other legal rights; and violating applicable laws or the terms of use of third-party platforms and model services.

You are solely responsible for complying with the laws and regulations of your jurisdiction and with the terms of the software and services you use. All actions and consequences arising from the use of this project are borne entirely by the user, and the authors and contributors accept no liability. If you do not agree, do not download, install, or use this project.

---

# AI中转站推荐 -- 杂鱼API

[点我跳转](https://www.zayuapi.com/)

![image.png](https://www.mizaawa.com/usr/uploads/2026/10/2898908578.png)

*稳定不掺水，已注册活跃用户高达 3000+，自有私有原创反代协议号池，支持 GPT、Claude、Gemini、Grok 外加大部分国模，价格始终处于国内第一低价梯队。*

## 小白向教程
博客文档：https://www.mizaawa.com/index.php/archives/95/

## Docker Compose 部署

需要 Docker Engine / Docker Desktop 和 Docker Compose v2：

```sh
git clone https://github.com/mizaawa/zakoai-proxy-cli.git
cd zakoai-proxy-cli
docker compose pull
docker compose build --pull
docker compose up -d --no-build
docker compose ps
```

访问 **http://localhost:1990**。全新数据卷首次进入会创建管理员账号，密码至少 12 个字符；之后使用同一账密登录。项目没有默认管理员账号或密码，请先完成初始化再对外开放服务。

`pull` 下载已发布镜像；`build --pull` 更新 Node、Rust、Debian 基础镜像并从当前源码重新编译项目；`up -d` 启动服务。镜像名称为 `ghcr.io/mizaawa/zakoai-proxy-cli:latest`，默认分支 CI 成功后会发布该镜像及对应提交 SHA 标签。

Compose 默认仅将宿主机 **127.0.0.1:1990** 映射到容器。需要对外开放端口时设置 `ZAKO_BIND_IP=0.0.0.0`，`ZAKO_PORT` 可修改宿主机端口。可在不提交 Git 的 `.env` 中配置：

```dotenv
ZAKO_BIND_IP=127.0.0.1
ZAKO_PORT=1990
TRUST_PROXY=0
```

公网部署请通过可信反向代理提供 HTTPS，后端端口保持私有。仅在此代理之后设置 `TRUST_PROXY=1`，代理须保留原始 `Host` 并传入 `X-Forwarded-Proto: https`，从而启用会话 Cookie 的 `Secure` 属性。SSE 代理需关闭响应缓冲，读取超时应大于系统设置中的请求超时。

```sh
docker compose logs --tail=100 -f
curl --fail http://localhost:1990/health
docker compose down
```

`down` 保留数据卷，`down -v` 会删除管理员、配置、密钥和统计数据。

### 在控制台更新

v0.3.0 起可在 **系统设置 → GitHub 源码更新** 中检查 GitHub 最新正式 Release。选择更新后需再次输入当前管理员密码；更新器从官方仓库检出对应标签，在后台从源码构建镜像，并显示构建、部署和健康检查日志。构建期间旧版本继续服务，切换版本时会短暂中断连接；页面恢复后可继续查看结果。

Compose 同时启动隔离的 `updater` 服务，不对宿主机发布端口。主程序通过内部 `UPDATER_URL=http://updater:9000/` 及只读 `/control/token` 访问它。Docker socket 仅挂载给更新器，不挂载给公开的代理服务。更新器持有近似宿主机 root 的 Docker 控制能力，请仅在受控服务器上部署，不要对外暴露内部接口或共享令牌。

源码构建成功后，更新器先停止代理并备份完整数据，再启动新镜像；部署或健康检查失败时恢复旧版本及同一次数据备份。更新状态和备份保留在 `update-state` 数据卷中，不能代替异地备份。账号、API key、分组、统计和 `master.key` 随数据卷保留。更新只接受官方仓库的正式版本标签，不支持输入任意地址或执行命令。

旧版部署需先拉取本版本源码并执行一次上述 Compose 构建和启动命令，以安装更新器。使用自定义 Compose 项目名时，保持 `COMPOSE_PROJECT_NAME` 一致；不使用 Compose 或未配置更新器的本地开发环境只展示版本信息，不能在网页执行更新。更新代理镜像不会自行替换更新器或修改宿主机 Compose 文件；发布说明要求调整部署配置时仍需由管理员执行。

## 配置中转

1. **分组管理：** 创建分组，在“破甲策略”中通过 `profile_ids` 独立勾选 `gpt-5.6-sol`、`gpt-6.1-sol`、`gpt-6-astra`。分组除名称外只配置策略选择，不再配置自定义模型列表或指令。按账号映射后的实际模型精确匹配已选策略：只使用该模型自己的配置，未勾选则不注入，同时勾选多个模型不会拼接不同策略。旧版未修改过的内置模板分组会自动识别。删除分组前需先解除账号和密钥引用。
2. **账号管理：** 输入上游端点及 API key，“支持协议”默认勾选 `responses` / `compact` / `anthropic`，可按需取消，再拉取并勾选上游实际可用模型、选择所属分组。Anthropic 对应原生 `/v1/messages` 链路，获取模型时也使用 Anthropic 鉴权。可通过 `model_mappings` 设置“请求模型 → 实际模型”，映射目标必须已勾选；映射仅执行一次，不递归串联，未配置映射的请求使用同名模型。端点可带或不带 `/v1`。RPM 和并发限制设为 `0` 表示不限制；编辑时密钥留空会保留原密钥。
3. **密钥管理：** 为指定分组创建命名的客户端 API key，可在创建后及之后登录控制台时复制。修改所属分组无需更换 key；停用或删除后阻止新的请求。API 基础地址为当前站点来源加 `/v1`。
4. **系统设置：** 设置安全入口、最大重试数、失败阈值、冷却时间、请求超时及统计保留天数。修改密码需验证原密码并再次确认新密码，成功后撤销所有会话。此处二次确认不是 TOTP 等独立第二认证因素。

例如，账号配置片段 `{"models":["gpt-6-astra"],"model_mappings":{"chat-main":"gpt-6-astra"}}` 允许客户端请求 `chat-main`，向上游发送的 `model` 为 `gpt-6-astra`；分组勾选 `gpt-6-astra` 策略后，注入该模型的策略，而不是按别名匹配。未设置映射的 `gpt-6-astra` 请求仍使用同名实际模型。`responses` 和 `compact` 使用相同规则。

调度仅选择匹配当前分组、协议且映射后实际模型已勾选的可用账号，未勾选任何可用模型的账号不接收请求。**优先级数字越小越先尝试**，相同优先级按账号 ID 稳定排序。单请求最多尝试 `最大重试数 + 1` 个不同账号。每次尝试都从原始客户端请求重新应用当前账号的单跳映射，再按实际模型匹配分组策略；故障转移到其他账号时独立计算，不继承或叠加前一账号的策略。连接失败、HTTP 401、403、408、429 和 HTTP 5xx 可在响应开始前切换到下一账号；连续可重试失败达到阈值时进入冷却，冷却时间结束后重新参与调度。其余 4xx 原样返回，不触发重试。流式响应已开始后中断不会重放请求。RPM 窗口、并发及冷却状态只保存在当前进程，重启后重置；同一个数据卷只运行一个实例。

安全入口将控制台从 `/` 移到 `/<入口>`，`/health` 和 `/v1/*` 不受影响。它是额外的入口隐藏措施，不代替登录认证。保存设置后请记录新的登录地址。

## 客户端 API

提供 `GET /v1/models`、`POST /v1/responses`、`POST /v1/responses/compact` 和 `POST /v1/messages`。`/v1/models` 包含当前分组账号已勾选的实际模型及有效映射的请求模型名。鉴权使用控制台生成的中转密钥，通过 `Authorization: Bearer` 或 `x-api-key` 传入；项目没有 Chat Completions 兼容路由。

Anthropic 客户端使用当前站点作为服务地址，选择已启用 `anthropic` 协议的账号模型。`/v1/messages` 保留原生消息、工具调用与 SSE，转发 `anthropic-version`（默认 `2023-06-01`）及 `anthropic-beta`。匹配分组策略时前置到 `system`，保留原有字符串或文本块数组。模型映射、故障切换、限流及统计沿用相同调度规则；输入 token 包含 Anthropic 缓存读取及写入，缓存命中 token 统计读取量。

```sh
export ZAKO_API_KEY='YOUR_GENERATED_PROXY_KEY'
curl http://localhost:1990/v1/responses \
  -H "Authorization: Bearer $ZAKO_API_KEY" \
  -H 'Content-Type: application/json' \
  -d '{"model":"YOUR_REQUEST_MODEL_ID","input":"Hello"}'

curl -N http://localhost:1990/v1/responses \
  -H "Authorization: Bearer $ZAKO_API_KEY" \
  -H 'Content-Type: application/json' \
  -d '{"model":"YOUR_REQUEST_MODEL_ID","input":"Hello","stream":true}'
```

`YOUR_REQUEST_MODEL_ID` 可以是已勾选的实际模型名，也可以是账号配置的请求模型别名。上下文压缩请求使用 `/v1/responses/compact`，请求格式及实际支持情况由上游决定。两种协议均先将请求的 `model` 替换为当前账号解析出的实际模型；若分组为该实际模型勾选了策略，再向 `instructions` 字段前置该策略文本。原有客户端指令和其他字段予以保留；未匹配已选策略时不注入指令。SSE 事件转发同时读取用量和完成状态，反射出的上游凭证会被脱敏。上游的“破甲”配置在本项目中属于可选指令文本，不保证改变模型行为或消除拒答，实际结果取决于上游服务。

## 统计与存储

- 仪表盘展示输入、输出、缓存及未命中 token、缓存命中率、RPM、TPM、总请求、成功率、平均耗时、每日图表、历史区间和 Cyber 标记次数。
- 缓存 token 已包含在输入 token 内，总 token 为输入加输出，不重复计数。用量来自上游 JSON 或完成后的 SSE 用量字段，缺失时计为零，不估算。
- RPM / TPM 为最近 60 秒已结束并记录的请求；每日统计使用 UTC。总量和单密钥用量基于保留中的请求元数据，并非永久计费账本。默认保留 90 天，新请求写入时清理过期记录。
- Cyber 计数表示响应中出现不区分大小写的 `cyber` 文本，每个响应最多计一次。它是文本启发式统计，不等同于特定弹窗、拒答或模型行为的检测结果。
- `/data/zako.db` 保存配置、会话摘要和请求元数据。上游密钥及可再次复制的客户端密钥使用 `/data/master.key` 配合 AES-256-GCM 加密；管理员密码使用 Argon2 摘要。请求日志不持久化提示词或响应正文；SQLite 文件和其他元数据未进行整盘加密。

### 备份与恢复

请将**整个 `/data`，特别是 `master.key`** 备份到受控存储。停机时可得到一致备份：

```sh
docker compose stop
docker compose cp proxy:/data ./zako-backup
docker compose start
```

恢复时先停止服务，将备份内容放回数据卷 `/data` 根目录并保留 `10001:10001` 所有权。数据库、SQLite 附属文件和 `master.key` 应来自同一次备份；缺少匹配主密钥的数据库不能解密存储的 API key。恢复后检查 `/health`、管理员登录和一次小型上游请求。请勿将备份提交到 Git。

## 本地开发与 CI

需要 Rust **1.99**、Node.js **22** 及 npm，SQLite 在 Rust 构建中自带：

```sh
cd web
npm ci
npm run build
cd ..
cargo run --locked
```

本地默认监听 `0.0.0.0:1990`，使用 `DATA_DIR=data`、`WEB_DIR=web/dist`。仅本机访问时设置 `BIND_ADDR=127.0.0.1:1990`。`RUST_LOG` 控制日志级别。

GitHub Actions 包含四项检查：**Rust Quality**（格式和严格 Clippy）、**Backend Tests**（单元测试及模拟上游集成测试）、**Frontend**（全新依赖安装、TypeScript 及生产构建）、**Docker**（Compose 验证、镜像构建、非 root HTTP 冒烟和重启后的初始化持久化）。Docker 在前三项通过后执行，再于默认分支推送时发布镜像。CI 使用临时生成的测试账密，不使用真实上游密钥。测试通过表示已覆盖行为验证完成，不是对所有潜在缺陷的零风险承诺。

```sh
cargo fmt --all -- --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-targets --all-features
```

## 开源协议与第三方声明

前端构建会自动收集锁定依赖的许可声明，包含 Inter 字体的 SIL Open Font License。生成的 `assets/third-party-licenses.txt` 由站点提供，并随镜像保存至 `/app/licenses/frontend-notices.txt`。第三方组件保留各自的许可条款。

本项目以 [MIT 协议](LICENSE) 开源。

### 上游项目与许可遵循

内置指令配置借鉴并改编自 [MDX-Tom/gpt-instruct](https://github.com/MDX-Tom/gpt-instruct)，遵循其 [MIT 许可条款](https://github.com/MDX-Tom/gpt-instruct/blob/main/LICENSE)。感谢上游作者与贡献者的开源工作。

- **上游版权：** Copyright (c) 2026 li lingbo；Copyright (c) 2026 yynxxxxx。
- **许可保留：** 完整版权与许可声明见 [third_party/gpt-instruct.LICENSE](third_party/gpt-instruct.LICENSE)，分发源码或容器时须保留本项目与上游的 MIT 声明。
- **借鉴范围：** 这些指令是导入的配置数据，并非上游应用代码或模型实现。
