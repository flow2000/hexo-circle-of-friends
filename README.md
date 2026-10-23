# 友链朋友圈

你是否经常烦恼于友链过多但没有时间浏览？那么友链朋友圈将解决这一痛点。你可以随时获取友链网站的更新内容，并了解友链的活跃情况。

即日起，项目恢复更新和维护，发布`v6.x.x`版本。

部署教程：[文档](https://fcircle-doc.yyyzyyyz.cn/)

## 数据存储

所有数据统一保存在仓库根目录的 `data.json` 中，不再依赖数据库：

```json
{
  "statistical_data": { "...": "统计信息" },
  "friends": [],
  "article_data": [],
  "summaries": []
}
```

- `fcircle_core`（Rust）每天爬取后直接写入 `data.json`，Actions 提交该文件
- API 直接读取 `data.json`，无需任何数据库服务

## Cloudflare 部署

仓库内置 `cloudflare/` 目录（Cloudflare Worker + 静态资源），可让 API 运行在 Cloudflare 边缘节点。

部署流程：GitHub Actions 每天爬取完成后，会提交 `data.json`，把它复制到 `cloudflare/public/data.json`，并通过 wrangler 自动部署 Worker。

需要在仓库 Secrets 中配置：

| Secret | 说明 |
| --- | --- |
| `CLOUDFLARE_API_TOKEN` | Cloudflare API Token |
| `CLOUDFLARE_ACCOUNT_ID` | Cloudflare Account ID |

`CLOUDFLARE_API_TOKEN` 所需权限：

- **Account** → `Workers Scripts` → **Edit**（上传/部署 Worker）
- **Account** → `Account Settings` → **Read**（读取账号信息）
- 如果要绑定自定义域名或路由：**Zone** → `Workers Routes` → **Edit**，以及 **Zone** → `Zone` → **Read**

不需要授予其他权限。未配置 Token 时，Actions 会跳过 Worker 部署，只更新 `data.json`。

本地手动部署：

```bash
cd cloudflare
npx wrangler deploy
```

部署前需确保 `cloudflare/public/data.json` 存在（CI 会自动复制；本地可手动执行 `cp data.json cloudflare/public/data.json`）。

## 其他部署方式

- **GitHub Actions**：每天 02:05（Asia/Shanghai）自动爬取并提交 `data.json`
- **Vercel**：`api/vercel.py` 提供 FastAPI 服务，直接读取 `data.json`
- **自托管服务器**：运行 `./fcircle_core` 生成数据，再用 `python3 -m uvicorn api.vercel:app --host 0.0.0.0 --port 8000` 启动 API

## API

`/`、`/version`、`/all`、`/friend`、`/randomfriend`、`/randompost`、`/post`、`/summary`

FastAPI 文档：`/docs`

## 更新说明

### `v6.0.6`

- 数据存储改为 `data.json`，移除 SQLite / MySQL / MongoDB 依赖
- 新增 Cloudflare Worker API（`cloudflare/`），由 Actions 自动部署
- 定时任务改为每天 02:05（上海时间）执行一次
- 仓库瘦身：移除 Rust API、`db` crate 及数据库相关代码

[![Stargazers over time](https://starchart.cc/Rock-Candy-Tea/hexo-circle-of-friends.svg)](https://starchart.cc/Rock-Candy-Tea/hexo-circle-of-friends)
