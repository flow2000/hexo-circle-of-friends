# -*- coding:utf-8 -*-
"""直接读取 data.json 的 API 数据访问层"""
import json
import os
import random
from urllib import parse

from tools.utils import get_base_path

_cache = {"mtime": None, "data": None}


def _data_path():
    return os.environ.get("FC_DATA_PATH") or os.path.join(get_base_path(), "data.json")


def _load_data():
    """读取 data.json，文件更新后自动重新加载"""
    path = _data_path()
    mtime = os.path.getmtime(path)
    if _cache["mtime"] != mtime or _cache["data"] is None:
        with open(path, "r", encoding="utf-8") as f:
            _cache["data"] = json.load(f)
        _cache["mtime"] = mtime
    return _cache["data"]


def _summaries_map(data):
    return {s.get("link"): s for s in data.get("summaries", [])}


def _summary_fields(summary):
    if not summary:
        return {
            "summary": None,
            "ai_model": None,
            "summary_created_at": None,
            "summary_updated_at": None,
        }
    return {
        "summary": summary.get("summary"),
        "ai_model": summary.get("ai_model"),
        "summary_created_at": summary.get("createdAt"),
        "summary_updated_at": summary.get("updatedAt"),
    }


def query_all(li, start: int = 0, end: int = 0, rule: str = "updated"):
    data = _load_data()

    # 检查rule的合法性
    if rule != "created" and rule != "updated":
        return {"message": "rule error, please use 'created'/'updated'"}

    posts = sorted(
        data.get("article_data", []),
        key=lambda p: p.get(rule) or "",
        reverse=True,
    )
    statistical_data = dict(data.get("statistical_data", {}))
    statistical_data["article_num"] = len(posts)
    if not statistical_data.get("last_updated_time"):
        statistical_data["last_updated_time"] = max(
            (p.get("createdAt") or "" for p in posts), default="1970-01-01 00:00:00"
        )

    if end <= 0 or end <= start:
        window = posts[start:]
    else:
        window = posts[start:end]

    summaries = _summaries_map(data)
    article_data = []
    for k, post in enumerate(window):
        item = {"floor": start + k + 1}
        for elem in li:
            item[elem] = post.get(elem)
        item.update(_summary_fields(summaries.get(post.get("link"))))
        article_data.append(item)

    return {"statistical_data": statistical_data, "article_data": article_data}


def query_friend():
    friends = _load_data().get("friends", [])
    if not friends:
        return {"message": "not found"}
    return friends


def query_random_friend(num):
    if num < 1:
        return {"message": "param 'num' error"}
    friends = _load_data().get("friends", [])
    if not friends:
        return {"message": "not found"}
    return random.sample(friends, min(num, len(friends)))


def query_random_post(num):
    if num < 1:
        return {"message": "param 'num' error"}
    posts = _load_data().get("article_data", [])
    if not posts:
        return {"message": "not found"}
    result = []
    for post in random.sample(posts, min(num, len(posts))):
        result.append(
            {
                "title": post.get("title"),
                "created": post.get("created"),
                "updated": post.get("updated"),
                "link": post.get("link"),
                "rule": post.get("rule") or "feed",
                "author": post.get("author"),
                "avatar": post.get("avatar"),
                "createdAt": post.get("createdAt") or None,
            }
        )
    return result


def query_post(link, num, rule):
    data = _load_data()
    if rule != "created" and rule != "updated":
        return {"message": "rule error, please use 'created'/'updated'"}

    friends = data.get("friends", [])
    posts = data.get("article_data", [])

    if link is None:
        active_friends = [f for f in friends if not f.get("error")]
        candidates = active_friends or friends
        if not candidates:
            return {"message": "not found"}
        user = random.choice(candidates)
        domain = parse.urlsplit(user.get("link", "")).netloc
    else:
        domain = parse.urlsplit(link).netloc
        user = next((f for f in friends if domain and domain in f.get("link", "")), None)
        # 如果 friends 中找不到（可能是换域名了），尝试通过 posts 反查
        if user is None and domain:
            sample_post = next((p for p in posts if domain in p.get("link", "")), None)
            if sample_post is not None:
                user = next(
                    (f for f in friends if f.get("name") == sample_post.get("author")),
                    None,
                )

    if user is None:
        return {"message": "not found"}

    matched = [p for p in posts if domain and domain in p.get("link", "")]
    matched.sort(key=lambda p: p.get(rule) or "", reverse=True)
    if num > 0:
        matched = matched[:num]

    article_data = [
        {
            "title": post.get("title"),
            "link": post.get("link"),
            "created": post.get("created"),
            "updated": post.get("updated"),
            "floor": floor + 1,
            "author": post.get("author"),
            "avatar": post.get("avatar"),
        }
        for floor, post in enumerate(matched)
    ]

    return {
        "statistical_data": {
            "name": user.get("name"),
            "link": user.get("link"),
            "avatar": user.get("avatar"),
            "article_num": len(article_data),
        },
        "article_data": article_data,
    }


def query_summary(link):
    """查询指定链接的文章摘要"""
    for summary in _load_data().get("summaries", []):
        if summary.get("link") == link:
            return {
                "link": summary.get("link"),
                "summary": summary.get("summary"),
                "ai_model": summary.get("ai_model"),
                "content_hash": summary.get("content_hash"),
                "created_at": summary.get("createdAt"),
                "updated_at": summary.get("updatedAt"),
            }
    return {"message": "not found"}
