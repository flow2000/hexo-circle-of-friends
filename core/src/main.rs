use std::collections::{HashMap, HashSet};

use chrono::{NaiveDate, Utc};
use data_structures::config::Settings;
use data_structures::metadata::{ArticleSummary, BasePosts, Friends, Posts};
use data_structures::storage::DataFile;
use downloader::{BEIJING_OFFSET, download, genai::EnhancedSummaryProvider};
use tokio;
use tracing::{error, info, warn};

const DATA_FILE: &str = "data.json";

/// 检查摘要是否需要更新
fn should_update_summary(
    existing_summary: &ArticleSummary,
    current_hash: &str,
) -> (bool, &'static str) {
    let content_unchanged = existing_summary.content_hash == current_hash;
    let summary_empty = existing_summary.summary.trim().is_empty();

    if content_unchanged && !summary_empty {
        (false, "内容未变化且摘要存在")
    } else if summary_empty {
        (true, "摘要为空")
    } else {
        (true, "内容已变化")
    }
}

/// 判断文章是否超过保留天数
fn is_outdated(updated: &str, today: NaiveDate, days: usize) -> bool {
    if days == 0 {
        return false;
    }
    let date = match updated.get(..10) {
        Some(date) => date,
        None => return false,
    };
    match NaiveDate::parse_from_str(date, "%Y-%m-%d") {
        Ok(date) => date < today - chrono::Duration::days(days as i64),
        Err(_) => false,
    }
}

/// 为文章生成摘要，结果写入 summaries（按 link 索引）
async fn generate_article_summaries(
    fc_settings: &Settings,
    posts: &[Posts],
    summaries: &mut HashMap<String, ArticleSummary>,
    client: &reqwest_middleware::ClientWithMiddleware,
) -> Result<(), Box<dyn std::error::Error>> {
    if !fc_settings.generate_summary.enabled {
        return Ok(());
    }

    let summary_provider = EnhancedSummaryProvider::new(fc_settings.generate_summary.clone());
    if let Err(e) = summary_provider.validate_config() {
        error!("摘要生成配置无效: {}", e);
        return Err(e.into());
    }

    // 创建专用的LLM客户端（30秒超时）
    let llm_client = download::build_client(30, 0);

    info!(
        "开始生成文章摘要，使用提供商: {}",
        fc_settings.generate_summary.provider
    );
    info!(
        "配置参数 - 最大并发: {}, 等待限速: {}, 最大字符: {}",
        fc_settings.generate_summary.get_max_concurrent(),
        fc_settings.generate_summary.get_wait_on_rate_limit(),
        fc_settings.generate_summary.get_max_chars(),
    );

    for post in posts {
        let link = &post.meta.link;
        let existing = summaries.get(link).cloned();

        let html_content = match download::start_crawl_detailpages(link, client).await {
            Ok(v) => v,
            Err(e) => {
                error!("获取文章详情失败 {}: {}", link, e);
                continue;
            }
        };
        let current_hash = tools::calculate_content_hash(&html_content);

        if let Some(existing_summary) = &existing {
            let (should_update, reason) = should_update_summary(existing_summary, &current_hash);
            if !should_update {
                info!("文章 {} {}，跳过摘要生成", link, reason);
                continue;
            }
            info!("文章 {} {}，需要更新摘要", link, reason);
        }

        match summary_provider
            .generate_summary(&llm_client, &html_content)
            .await
        {
            Ok(summary_result) => {
                let now = tools::strptime_to_string_ymdhms(
                    Utc::now().with_timezone(&BEIJING_OFFSET.unwrap()),
                );
                let created_at = existing
                    .map(|summary| summary.created_at)
                    .unwrap_or_else(|| now.clone());
                summaries.insert(
                    link.clone(),
                    ArticleSummary::new(
                        link.clone(),
                        current_hash,
                        summary_result.summary,
                        Some(summary_result.model),
                        created_at,
                        now,
                    ),
                );
                info!("成功生成文章摘要: {}", link);
            }
            Err(e) => error!("生成文章摘要失败 {}: {}", link, e),
        }
    }

    info!("文章摘要生成完成");
    Ok(())
}

/// 将本次抓取结果与已有 JSON 数据合并
fn merge_posts(
    old_posts: Vec<Posts>,
    all_res: &[(Friends, Vec<BasePosts>)],
    max_posts_num: usize,
    created_at: String,
) -> Vec<Posts> {
    let mut posts = old_posts;
    for (friend, base_posts) in all_res {
        if base_posts.is_empty() {
            continue;
        }
        let limit = if max_posts_num > 0 {
            max_posts_num
        } else {
            base_posts.len()
        };
        let new_posts: Vec<Posts> = base_posts
            .iter()
            .take(limit)
            .map(|post| {
                Posts::new(
                    post.clone(),
                    friend.name.clone(),
                    friend.avatar.clone(),
                    created_at.clone(),
                )
            })
            .collect();
        // 与旧逻辑一致：同一友链、同一文章链接先删除再写入
        let new_links: HashSet<&str> = new_posts.iter().map(|p| p.meta.link.as_str()).collect();
        posts.retain(|p| {
            !(p.author == friend.name && new_links.contains(p.meta.link.as_str()))
        });
        posts.extend(new_posts);
    }
    posts
}

#[tokio::main]
async fn main() {
    let _guard = tools::init_tracing(
        "fcircle_core",
        Some(
            "error,fcircle_core=debug,downloader=debug,tools=debug,data_structures=debug",
        ),
    );

    let now = Utc::now().with_timezone(&BEIJING_OFFSET.unwrap());

    let css_rules: tools::Value = tools::get_yaml("./css_rules.yaml").unwrap();
    let fc_settings = tools::get_yaml_settings("./fc_settings.yaml").unwrap();

    let client = download::build_client(10, 3);

    let format_base_friends =
        download::start_crawl_linkpages(&fc_settings, &css_rules, &client).await;

    let mut tasks = vec![];
    for friend in format_base_friends {
        let fc_settings = fc_settings.clone();
        let client = client.clone();
        let css_rules = css_rules.clone();
        let task = tokio::spawn(async move {
            let format_base_posts = download::start_crawl_postpages(
                friend.link.clone(),
                &fc_settings,
                "".to_string(),
                &css_rules,
                &client,
            )
            .await
            .unwrap();
            (friend, format_base_posts)
        });
        tasks.push(task);
    }

    // 处理配置项友链
    if fc_settings.settings_friends_links.enable {
        info!("处理配置项友链...");
        let json_friends_links = if !fc_settings
            .settings_friends_links
            .json_api_or_path
            .is_empty()
        {
            if fc_settings
                .settings_friends_links
                .json_api_or_path
                .starts_with("http")
            {
                if let Ok(json_friends_links) = download::start_get_friends_links_from_json(
                    &fc_settings.settings_friends_links.json_api_or_path,
                    &client,
                )
                .await
                {
                    info!(
                        "从api:{}获取配置项友链成功",
                        fc_settings.settings_friends_links.json_api_or_path
                    );
                    json_friends_links.friends
                } else {
                    error!(
                        "从api:{}获取配置项友链失败",
                        fc_settings.settings_friends_links.json_api_or_path
                    );
                    Vec::new()
                }
            } else if let Ok(json_friends_links) = tools::get_json_friends_links(
                &fc_settings.settings_friends_links.json_api_or_path,
            ) {
                info!(
                    "从文件:{}获取配置项友链成功",
                    fc_settings.settings_friends_links.json_api_or_path
                );
                json_friends_links.friends
            } else {
                error!(
                    "从文件:{}获取配置项友链失败",
                    fc_settings.settings_friends_links.json_api_or_path
                );
                Vec::new()
            }
        } else {
            Vec::new()
        };

        let mut settings_friend_postpages = fc_settings.settings_friends_links.list.clone();
        settings_friend_postpages.extend(json_friends_links);
        for postpage_vec in settings_friend_postpages {
            let tm: chrono::DateTime<chrono::FixedOffset> = now;
            let created_at = tools::strptime_to_string_ymdhms(tm);
            let base_post = Friends::new(
                postpage_vec[0].clone(),
                postpage_vec[1].clone(),
                postpage_vec[2].clone(),
                false,
                created_at,
            );
            let fc_settings = fc_settings.clone();
            let client = client.clone();
            let css_rules = css_rules.clone();
            let task = tokio::spawn(async move {
                let format_base_posts = match download::start_crawl_postpages(
                    base_post.link.clone(),
                    &fc_settings,
                    if postpage_vec.len() == 3 {
                        String::from("")
                    } else if postpage_vec.len() == 4 {
                        postpage_vec[3].clone()
                    } else {
                        panic!("`SETTINGS_FRIENDS_LINKS-list`下的数组长度只能为3或4");
                    },
                    &css_rules,
                    &client,
                )
                .await
                {
                    Ok(v) => v,
                    Err(e) => {
                        error!("{}", e);
                        return (base_post, vec![]);
                    }
                };
                (base_post, format_base_posts)
            });
            tasks.push(task);
        }
    }

    let mut all_res = vec![];
    for task in tasks {
        all_res.push(task.await.unwrap());
    }

    // 读取已有数据（保留历史文章和摘要缓存）
    let old_data = DataFile::load(DATA_FILE);
    if old_data.is_none() {
        warn!("未找到或无法解析 {}，本次将生成全新数据", DATA_FILE);
    }
    let old_posts = old_data
        .as_ref()
        .map(|data| data.article_data.clone())
        .unwrap_or_default();
    let mut summaries: HashMap<String, ArticleSummary> = old_data
        .as_ref()
        .map(|data| {
            data.summaries
                .iter()
                .map(|summary| (summary.link.clone(), summary.clone()))
                .collect()
        })
        .unwrap_or_default();

    let mut friends = Vec::with_capacity(all_res.len());
    let mut success_num = 0;
    let mut failed_friends = vec![];
    for (friend, base_posts) in &all_res {
        let mut friend = friend.clone();
        if base_posts.is_empty() {
            friend.error = true;
            failed_friends.push(friend.clone());
        } else {
            friend.error = false;
            success_num += 1;
        }
        friends.push(friend);
    }

    let created_at = tools::strptime_to_string_ymdhms(now);
    let mut posts = merge_posts(old_posts, &all_res, fc_settings.max_posts_num, created_at);

    // 清理过期文章
    let today = now.date_naive();
    let before_clean = posts.len();
    posts.retain(|post| !is_outdated(&post.meta.updated, today, fc_settings.outdate_clean));
    let cleaned = before_clean - posts.len();

    // 生成文章摘要
    let summary_result =
        generate_article_summaries(&fc_settings, &posts, &mut summaries, &client).await;
    if let Err(e) = summary_result {
        error!("生成文章摘要失败: {}", e);
    }

    // 只保留当前文章对应的摘要
    let post_links: HashSet<&str> = posts.iter().map(|post| post.meta.link.as_str()).collect();
    let summaries: Vec<ArticleSummary> = summaries
        .into_iter()
        .filter(|(link, _)| post_links.contains(link.as_str()))
        .map(|(_, summary)| summary)
        .collect();

    let data = DataFile::new(friends, posts, summaries);
    if let Err(e) = data.save(DATA_FILE) {
        error!("写入 {} 失败: {}", DATA_FILE, e);
        return;
    }

    info!(
        "成功友链数 {}，失败友链数 {}",
        success_num,
        failed_friends.len()
    );
    info!("本次获取总文章数 {}", data.article_data.len());
    info!(
        "清理过期文章(距今超过{}天) {} 条",
        fc_settings.outdate_clean, cleaned
    );
    info!(
        "失联友链明细 {}",
        serde_json::to_string_pretty(&failed_friends).unwrap()
    );
    info!("数据已成功写入 {}", DATA_FILE);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn summary(content_hash: &str, summary: &str) -> ArticleSummary {
        ArticleSummary {
            link: "test".to_string(),
            content_hash: content_hash.to_string(),
            summary: summary.to_string(),
            ai_model: Some("test-model".to_string()),
            created_at: "2023-01-01".to_string(),
            updated_at: "2023-01-01".to_string(),
        }
    }

    #[test]
    fn test_should_update_summary_content_unchanged_summary_exists() {
        let (should_update, reason) =
            should_update_summary(&summary("hash123", "existing"), "hash123");
        assert!(!should_update);
        assert_eq!(reason, "内容未变化且摘要存在");
    }

    #[test]
    fn test_should_update_summary_content_changed() {
        let (should_update, reason) =
            should_update_summary(&summary("hash123", "existing"), "hash456");
        assert!(should_update);
        assert_eq!(reason, "内容已变化");
    }

    #[test]
    fn test_should_update_summary_empty_summary() {
        let (should_update, reason) = should_update_summary(&summary("hash123", ""), "hash123");
        assert!(should_update);
        assert_eq!(reason, "摘要为空");
    }

    #[test]
    fn test_should_update_summary_whitespace_only_summary() {
        let (should_update, reason) =
            should_update_summary(&summary("hash123", "   \n\t  "), "hash123");
        assert!(should_update);
        assert_eq!(reason, "摘要为空");
    }

    #[test]
    fn test_should_update_summary_empty_and_content_changed() {
        let (should_update, reason) = should_update_summary(&summary("hash123", ""), "hash456");
        assert!(should_update);
        assert_eq!(reason, "摘要为空");
    }

    #[test]
    fn test_is_outdated() {
        let today = NaiveDate::from_ymd_opt(2026, 10, 23).unwrap();
        assert!(is_outdated("2026-08-01", today, 60));
        assert!(!is_outdated("2026-09-01", today, 60));
        assert!(!is_outdated("unknown", today, 60));
        assert!(!is_outdated("2020-01-01", today, 0));
    }
}
