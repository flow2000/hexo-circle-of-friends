/**
 * Hexo Circle of Friends — Cloudflare Worker API
 * 直接读取随 Worker 一起部署的 data.json，无需任何数据库。
 */

const VERSION = "6.0.6";
const DATA_CACHE_TTL = 60 * 1000;

const CORS_HEADERS = {
  "Access-Control-Allow-Origin": "*",
  "Access-Control-Allow-Methods": "GET,OPTIONS",
  "Access-Control-Allow-Headers": "*",
};

let dataCache = { data: null, loadedAt: 0 };

function json(body, status = 200) {
  return new Response(JSON.stringify(body), {
    status,
    headers: {
      "Content-Type": "application/json; charset=utf-8",
      ...CORS_HEADERS,
    },
  });
}

function notFound() {
  return json({ message: "not found" }, 404);
}

function badRequest(message) {
  return json({ message }, 400);
}

function getDomain(link) {
  try {
    return new URL(link).host;
  } catch {
    return "";
  }
}

function readInt(url, name, fallback) {
  const raw = url.searchParams.get(name);
  if (raw === null || raw === "") return fallback;
  const value = Number(raw);
  return Number.isInteger(value) ? value : NaN;
}

function readNum(url) {
  const num = readInt(url, "num", 1);
  if (!Number.isInteger(num) || num < 1 || num > 100) return null;
  return num;
}

async function loadData(request, env) {
  const now = Date.now();
  if (dataCache.data && now - dataCache.loadedAt < DATA_CACHE_TTL) {
    return dataCache.data;
  }
  const response = await env.ASSETS.fetch(new URL("/data.json", request.url));
  if (!response.ok) {
    throw new Error(`data.json not found (${response.status})`);
  }
  dataCache = { data: await response.json(), loadedAt: now };
  return dataCache.data;
}

function summaryMap(data) {
  return new Map((data.summaries || []).map((summary) => [summary.link, summary]));
}

function summaryFields(summary) {
  return {
    summary: summary ? summary.summary : null,
    ai_model: summary ? summary.ai_model : null,
    summary_created_at: summary ? summary.createdAt : null,
    summary_updated_at: summary ? summary.updatedAt : null,
  };
}

function sortPosts(posts, rule) {
  return [...posts].sort((a, b) =>
    String(b[rule] || "").localeCompare(String(a[rule] || ""))
  );
}

async function handleAll(url, data) {
  const rule = url.searchParams.get("rule") || "updated";
  if (rule !== "created" && rule !== "updated") {
    return badRequest("rule error, please use 'created'/'updated'");
  }
  const start = readInt(url, "start", 0);
  const end = readInt(url, "end", -1);
  if (Number.isNaN(start) || start < 0 || Number.isNaN(end)) {
    return badRequest("param error");
  }

  const posts = sortPosts(data.article_data || [], rule);
  const window = end > 0 && end > start ? posts.slice(start, end) : posts.slice(start);
  const summaries = summaryMap(data);
  const articleData = window.map((post, index) => ({
    floor: start + index + 1,
    title: post.title,
    created: post.created,
    updated: post.updated,
    link: post.link,
    author: post.author,
    avatar: post.avatar,
    ...summaryFields(summaries.get(post.link)),
  }));

  const statisticalData = { ...(data.statistical_data || {}) };
  statisticalData.article_num = posts.length;

  return json({ statistical_data: statisticalData, article_data: articleData });
}

function handleFriend(data) {
  if (!data.friends || data.friends.length === 0) return notFound();
  return json(data.friends);
}

function handleRandomFriend(url, data) {
  const num = readNum(url);
  if (num === null) return badRequest("param 'num' error");
  const friends = [...(data.friends || [])];
  if (friends.length === 0) return notFound();
  for (let i = friends.length - 1; i > 0; i -= 1) {
    const j = Math.floor(Math.random() * (i + 1));
    [friends[i], friends[j]] = [friends[j], friends[i]];
  }
  return json(friends.slice(0, Math.min(num, friends.length)));
}

function handleRandomPost(url, data) {
  const num = readNum(url);
  if (num === null) return badRequest("param 'num' error");
  const posts = [...(data.article_data || [])];
  if (posts.length === 0) return notFound();
  for (let i = posts.length - 1; i > 0; i -= 1) {
    const j = Math.floor(Math.random() * (i + 1));
    [posts[i], posts[j]] = [posts[j], posts[i]];
  }
  return json(
    posts.slice(0, Math.min(num, posts.length)).map((post) => ({
      title: post.title,
      created: post.created,
      updated: post.updated,
      link: post.link,
      rule: post.rule || "feed",
      author: post.author,
      avatar: post.avatar,
      createdAt: post.createdAt || null,
    }))
  );
}

function handlePost(url, data) {
  const link = url.searchParams.get("link");
  const rule = url.searchParams.get("rule") || "created";
  const num = readInt(url, "num", -1);
  if (rule !== "created" && rule !== "updated") {
    return badRequest("rule error, please use 'created'/'updated'");
  }
  if (Number.isNaN(num) || num === 0) return badRequest("param 'num' error");

  const friends = data.friends || [];
  const allPosts = data.article_data || [];
  let domain = "";
  let friend = null;

  if (link === null) {
    const activeFriends = friends.filter((item) => !item.error);
    const candidates = activeFriends.length > 0 ? activeFriends : friends;
    if (candidates.length === 0) return notFound();
    friend = candidates[Math.floor(Math.random() * candidates.length)];
    domain = getDomain(friend.link);
  } else {
    domain = getDomain(link);
    if (domain) {
      friend = friends.find((item) => (item.link || "").includes(domain)) || null;
      // 友链可能换域名，退回用文章列表反查作者
      if (!friend) {
        const sample = allPosts.find((post) => (post.link || "").includes(domain));
        if (sample) {
          friend = friends.find((item) => item.name === sample.author) || null;
        }
      }
    }
  }

  if (!friend) return notFound();

  let posts = domain ? allPosts.filter((post) => (post.link || "").includes(domain)) : [];
  posts = sortPosts(posts, rule);
  if (num > 0) posts = posts.slice(0, num);

  return json({
    statistical_data: {
      name: friend.name,
      link: friend.link,
      avatar: friend.avatar,
      article_num: posts.length,
    },
    article_data: posts.map((post, index) => ({
      title: post.title,
      link: post.link,
      created: post.created,
      updated: post.updated,
      floor: index + 1,
      author: post.author,
      avatar: post.avatar,
    })),
  });
}

function handleSummary(url, data) {
  const link = url.searchParams.get("link");
  if (!link) return badRequest("param 'link' is required");
  const summary = (data.summaries || []).find((item) => item.link === link);
  if (!summary) return notFound();
  return json({
    link: summary.link,
    summary: summary.summary,
    ai_model: summary.ai_model,
    content_hash: summary.content_hash,
    created_at: summary.createdAt,
    updated_at: summary.updatedAt,
  });
}

export default {
  async fetch(request, env) {
    if (request.method === "OPTIONS") {
      return new Response(null, { status: 204, headers: CORS_HEADERS });
    }
    if (request.method !== "GET") {
      return json({ message: "method not allowed" }, 405);
    }

    const url = new URL(request.url);
    const path = url.pathname.replace(/\/+$/, "") || "/";
    const version = env.VERSION || VERSION;

    if (path === "/") {
      return json({
        message: "服务运行正常",
        version,
        docs: "/",
        database: "json",
      });
    }
    if (path === "/version") {
      return json({ version });
    }

    let data;
    try {
      data = await loadData(request, env);
    } catch (error) {
      return json({ message: `Internal server error: ${error.message}` }, 500);
    }

    switch (path) {
      case "/all":
        return handleAll(url, data);
      case "/friend":
        return handleFriend(data);
      case "/randomfriend":
        return handleRandomFriend(url, data);
      case "/randompost":
        return handleRandomPost(url, data);
      case "/post":
        return handlePost(url, data);
      case "/summary":
        return handleSummary(url, data);
      default:
        return notFound();
    }
  },
};
