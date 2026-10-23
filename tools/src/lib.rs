use chrono::{DateTime, Datelike, NaiveDate, NaiveDateTime, TimeZone};
use data_structures::config;
use logroller::{Compression, LogRollerBuilder, Rotation, RotationAge};
pub use serde_yaml::Value;
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{self, BufReader};
use tracing::info;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{
    EnvFilter,
    fmt::{self, format},
    prelude::*,
};

#[allow(deprecated)]
pub fn init_tracing(logger_name: &str, filter_str: Option<&str>) -> WorkerGuard {
    // stdout和file同时输出，并设置不同的fmt
    // 输出划分为http和core两个文件，通过filter来实现 https://docs.rs/tracing-subscriber/latest/tracing_subscriber/layer/index.html
    let formmater_string = "%Y-%m-%d %H:%M:%S (%Z)".to_string();
    let timer = tracing_subscriber::fmt::time::ChronoLocal::new(formmater_string);
    // about timezone,see:https://github.com/tokio-rs/tracing/issues/3102
    let appender = LogRollerBuilder::new(
        &format!("./logs/{logger_name}"),
        &format!("{logger_name}.log"),
    )
    .rotation(Rotation::AgeBased(RotationAge::Daily)) // Rotate daily
    .max_keep_files(7) // Keep a week's worth of logs
    .time_zone(logroller::TimeZone::Local) // Use local timezone
    .compression(Compression::Gzip) // Compress old logs
    .build()
    .unwrap();
    let stdout_layer = fmt::layer()
        .with_target(true)
        .with_level(true)
        .with_timer(timer.clone())
        .with_thread_ids(true)
        .with_file(true)
        .with_line_number(true)
        .with_ansi(true)
        .compact();
    let (non_blocking, _guard) = tracing_appender::non_blocking(appender);

    let layer = fmt::layer()
        .with_target(true)
        .with_level(true)
        .with_thread_ids(true)
        .with_timer(timer.clone())
        .with_file(true)
        .with_line_number(true)
        .with_ansi(false)
        .fmt_fields(format::PrettyFields::new().with_ansi(false))
        .with_writer(non_blocking)
        .compact();
    let global_filter = if let Some(filter_str) = filter_str {
        EnvFilter::new(filter_str)
    } else {
        EnvFilter::new("trace")
    };
    tracing_subscriber::registry()
        .with(global_filter)
        .with(stdout_layer)
        .with(layer)
        .init();
    info!("Setup tracing success for {}", logger_name);
    _guard
}

/// 将时间结构转换为统一格式的字符串`%Y-%m-%d %H:%M:%S`，带时分秒
pub fn strptime_to_string_ymdhms<Tz: TimeZone>(strptime: DateTime<Tz>) -> String {
    strptime
        .fixed_offset()
        .format("%Y-%m-%d %H:%M:%S")
        .to_string()
}

/// 将时间结构转换为统一格式的字符串`%Y-%m-%d`，不带时分秒
pub fn strptime_to_string_ymd<Tz: TimeZone>(strptime: DateTime<Tz>) -> String {
    strptime.fixed_offset().format("%Y-%m-%d").to_string()
}

/// 将可能不标准的时间字符串转换为统一格式的字符串`%Y-%m-%d`，不带时分秒
pub fn strftime_to_string_ymd(strftime: &str) -> Result<String, Box<dyn std::error::Error>> {
    // 首先尝试两位年份格式（需要放在前面避免被四位年份格式误匹配）
    let two_digit_year_fmts = [
        "%d/%m/%y",          // 25/12/23
        "%d-%m-%y",          // 25-12-23
        "%m/%d/%y",          // 12/25/23
        "%m-%d-%y",          // 12-25-23
        "%d/%m/%y %H:%M:%S", // 25/12/23 15:30:45
        "%d-%m-%y %H:%M:%S", // 25-12-23 15:30:45
        "%m/%d/%y %H:%M:%S", // 12/25/23 15:30:45
        "%m-%d-%y %H:%M:%S", // 12-25-23 15:30:45
        "%d/%m/%y %H:%M",    // 25/12/23 15:30
        "%d-%m-%y %H:%M",    // 25-12-23 15:30
        "%m/%d/%y %H:%M",    // 12/25/23 15:30
        "%m-%d-%y %H:%M",    // 12-25-23 15:30
    ];

    for fmt in two_digit_year_fmts {
        if fmt.contains("%H") {
            // 带时间的两位年份格式
            if let Ok(mut v) = NaiveDateTime::parse_from_str(strftime, fmt) {
                // 如果年份小于50，假设是20xx，否则假设是19xx
                let year = v.year();
                if year < 50 {
                    v = v.with_year(year + 2000).unwrap();
                } else if year < 100 {
                    v = v.with_year(year + 1900).unwrap();
                }
                return Ok(v.date().format("%Y-%m-%d").to_string());
            }
        } else {
            // 仅日期的两位年份格式
            if let Ok(mut v) = NaiveDate::parse_from_str(strftime, fmt) {
                // 如果年份小于50，假设是20xx，否则假设是19xx
                let year = v.year();
                if year < 50 {
                    v = v.with_year(year + 2000).unwrap();
                } else if year < 100 {
                    v = v.with_year(year + 1900).unwrap();
                }
                return Ok(v.format("%Y-%m-%d").to_string());
            }
        }
    }

    // 仅日期格式（四位年份）
    let date_only_fmts = [
        "%Y-%m-%d",     // 2023-12-25
        "%Y/%m/%d",     // 2023/12/25
        "%Y.%m.%d",     // 2023.12.25
        "%Y %m %d",     // 2023 12 25
        "%Y年%m月%d日", // 2023年12月25日
        "%Y年%m月%d",   // 2023年12月25
        "%Y%m%d",       // 20231225
        "%d/%m/%Y",     // 25/12/2023
        "%d-%m-%Y",     // 25-12-2023
        "%d.%m.%Y",     // 25.12.2023
        "%m/%d/%Y",     // 12/25/2023
        "%m-%d-%Y",     // 12-25-2023
        "%m.%d.%Y",     // 12.25.2023
        // 英文月份格式
        "%d %b %Y",  // 25 Dec 2023
        "%b %d, %Y", // Dec 25, 2023
        "%b %d %Y",  // Dec 25 2023
        "%d %B %Y",  // 25 December 2023
        "%B %d, %Y", // December 25, 2023
        "%B %d %Y",  // December 25 2023
    ];

    for fmt in date_only_fmts {
        if let Ok(v) = NaiveDate::parse_from_str(strftime, fmt) {
            return Ok(v.format("%Y-%m-%d").to_string());
        }
    }

    // 带时间的格式（四位年份）
    let datetime_fmts = [
        "%Y-%m-%d %H:%M:%S",      // 2023-12-25 15:30:45
        "%Y-%m-%d %H:%M",         // 2023-12-25 15:30
        "%Y-%m-%dT%H:%M:%S",      // 2023-12-25T15:30:45
        "%Y-%m-%dT%H:%M",         // 2023-12-25T15:30
        "%Y-%m-%dT%H:%M:%S.%3fZ", // 2023-12-25T15:30:45.123Z
        "%Y-%m-%dT%H:%M:%SZ",     // 2023-12-25T15:30:45Z
        "%Y-%m-%dT%H:%M:%S.%6f",  // 2023-12-25T15:30:45.123456
        "%Y-%m-%d %H:%M:%S%.3f",  // 2023-12-25 15:30:45.123
        "%Y-%m-%d %H:%M:%S%.6f",  // 2023-12-25 15:30:45.123456
        "%Y/%m/%d %H:%M:%S",      // 2023/12/25 15:30:45
        "%Y/%m/%d %H:%M",         // 2023/12/25 15:30
        "%Y.%m.%d %H:%M:%S",      // 2023.12.25 15:30:45
        "%Y.%m.%d %H:%M",         // 2023.12.25 15:30
        "%Y-%m-%d%H:%M:%S",       // 2023-12-2515:30:45 (无空格)
        "%Y-%m-%d%H:%M",          // 2023-12-2515:30 (无空格)
        "%Y年%m月%d日 %H:%M:%S",  // 2023年12月25日 15:30:45
        "%Y年%m月%d日 %H:%M",     // 2023年12月25日 15:30
        "%Y年%m月%d日%H:%M:%S",   // 2023年12月25日15:30:45
        "%Y年%m月%d日%H:%M",      // 2023年12月25日15:30
        "%d/%m/%Y %H:%M:%S",      // 25/12/2023 15:30:45
        "%d/%m/%Y %H:%M",         // 25/12/2023 15:30
        "%d-%m-%Y %H:%M:%S",      // 25-12-2023 15:30:45
        "%d-%m-%Y %H:%M",         // 25-12-2023 15:30
        "%m/%d/%Y %H:%M:%S",      // 12/25/2023 15:30:45
        "%m/%d/%Y %H:%M",         // 12/25/2023 15:30
        "%m-%d-%Y %H:%M:%S",      // 12-25-2023 15:30:45
        "%m-%d-%Y %H:%M",         // 12-25-2023 15:30
        // RFC2822 格式
        "%a, %d %b %Y %H:%M:%S %z", // Mon, 25 Dec 2023 15:30:45 +0800
        "%a, %d %b %Y %H:%M:%S",    // Mon, 25 Dec 2023 15:30:45
        "%d %b %Y %H:%M:%S",        // 25 Dec 2023 15:30:45
        // 英文月份带时间格式
        "%B %d, %Y %H:%M:%S", // December 25, 2023 15:30:45
        "%B %d %Y %H:%M:%S",  // December 25 2023 15:30:45
        "%d %B %Y %H:%M:%S",  // 25 December 2023 15:30:45
        "%b %d, %Y %H:%M:%S", // Dec 25, 2023 15:30:45
        "%b %d %Y %H:%M:%S",  // Dec 25 2023 15:30:45
        "%d %b %Y %H:%M:%S",  // 25 Dec 2023 15:30:45
    ];

    for fmt in datetime_fmts {
        if let Ok(v) = NaiveDateTime::parse_from_str(strftime, fmt) {
            return Ok(v.date().format("%Y-%m-%d").to_string());
        }
    }

    Err(Box::new(std::io::Error::other(format!(
        "{strftime} is not a valid date",
    ))))
}

pub fn get_yaml(path: &str) -> io::Result<Value> {
    let config_file = File::open(path)?;
    match serde_yaml::from_reader(config_file) {
        Ok(config) => Ok(config),
        Err(err) => panic!("{}", err),
    }
}

pub fn get_yaml_settings(path: &str) -> io::Result<config::Settings> {
    let config_file = File::open(path)?;
    match serde_yaml::from_reader(config_file) {
        Ok(config) => Ok(config),
        Err(err) => panic!("{}", err),
    }
}

/// 获取环境变量，如果为空则返回错误
pub fn get_env_var(var_name: &str) -> Result<String, Box<dyn std::error::Error>> {
    dotenvy::dotenv_override()?;
    match dotenvy::var(var_name) {
        Ok(var) => {
            if var.is_empty() {
                Err(Box::new(std::io::Error::other(format!(
                    "{var_name} is not set",
                ))))
            } else {
                Ok(var)
            }
        }
        Err(_) => Err(Box::new(std::io::Error::other(format!(
            "{var_name} is not set",
        )))),
    }
}

/// 解析JSON文件为SettingsFriendsLinksJsonMeta结构
pub fn get_json_friends_links(
    path: &str,
) -> Result<config::SettingsFriendsLinksJsonMeta, Box<dyn std::error::Error>> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let data: config::SettingsFriendsLinksJsonMeta = serde_json::from_reader(reader)?;
    Ok(data)
}

/// 计算HTML内容的SHA256哈希值
pub fn calculate_content_hash(html_content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(html_content.as_bytes());
    format!("{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{FixedOffset, TimeZone, Utc};

    #[test]
    fn test_strptime_to_string_ymdhms() {
        // 测试UTC时间
        let utc_time = Utc.with_ymd_and_hms(2023, 12, 25, 15, 30, 45).unwrap();
        let result = strptime_to_string_ymdhms(utc_time);
        assert_eq!(result, "2023-12-25 15:30:45");

        // 测试带时区的时间
        let offset = FixedOffset::east_opt(8 * 3600).unwrap(); // +8时区
        let offset_time = offset.with_ymd_and_hms(2023, 1, 1, 12, 0, 0).unwrap();
        let result = strptime_to_string_ymdhms(offset_time);
        assert_eq!(result, "2023-01-01 12:00:00");
    }

    #[test]
    fn test_strptime_to_string_ymd() {
        // 测试UTC时间
        let utc_time = Utc.with_ymd_and_hms(2023, 12, 25, 15, 30, 45).unwrap();
        let result = strptime_to_string_ymd(utc_time);
        assert_eq!(result, "2023-12-25");

        // 测试带时区的时间
        let offset = FixedOffset::east_opt(8 * 3600).unwrap();
        let offset_time = offset.with_ymd_and_hms(2023, 1, 1, 12, 0, 0).unwrap();
        let result = strptime_to_string_ymd(offset_time);
        assert_eq!(result, "2023-01-01");
    }

    #[test]
    fn test_strftime_to_string_ymd() {
        // 测试标准日期格式
        assert_eq!(strftime_to_string_ymd("2025-04-19").unwrap(), "2025-04-19");
        assert_eq!(strftime_to_string_ymd("2023-12-25").unwrap(), "2023-12-25");

        // 测试斜杠分隔符格式
        assert_eq!(strftime_to_string_ymd("2023/12/25").unwrap(), "2023-12-25");
        assert_eq!(strftime_to_string_ymd("25/12/2023").unwrap(), "2023-12-25");
        assert_eq!(strftime_to_string_ymd("12/25/2023").unwrap(), "2023-12-25");

        // 测试点分隔符格式
        assert_eq!(strftime_to_string_ymd("2023.12.25").unwrap(), "2023-12-25");
        assert_eq!(strftime_to_string_ymd("25.12.2023").unwrap(), "2023-12-25");
        assert_eq!(strftime_to_string_ymd("12.25.2023").unwrap(), "2023-12-25");

        // 测试空格分隔符格式
        assert_eq!(strftime_to_string_ymd("2023 12 25").unwrap(), "2023-12-25");

        // 测试中文格式
        assert_eq!(
            strftime_to_string_ymd("2023年12月25日").unwrap(),
            "2023-12-25"
        );
        assert_eq!(
            strftime_to_string_ymd("2023年12月25").unwrap(),
            "2023-12-25"
        );

        // 测试紧凑格式
        assert_eq!(strftime_to_string_ymd("20231225").unwrap(), "2023-12-25");

        // 测试两位年份格式（应该被解释为20xx年）
        assert_eq!(strftime_to_string_ymd("25/12/23").unwrap(), "2023-12-25");
        assert_eq!(strftime_to_string_ymd("12/25/23").unwrap(), "2023-12-25");
        assert_eq!(strftime_to_string_ymd("25-12-23").unwrap(), "2023-12-25");
        assert_eq!(strftime_to_string_ymd("12-25-23").unwrap(), "2023-12-25");

        // 测试边界情况
        assert_eq!(strftime_to_string_ymd("2000-01-01").unwrap(), "2000-01-01");
        assert_eq!(strftime_to_string_ymd("9999-12-31").unwrap(), "9999-12-31");
    }

    #[test]
    fn test_strftime_to_string_ymd_with_time() {
        // 测试带时分秒的格式
        assert_eq!(
            strftime_to_string_ymd("2023-12-25 15:30:45").unwrap(),
            "2023-12-25"
        );
        assert_eq!(
            strftime_to_string_ymd("2023-12-25 15:30").unwrap(),
            "2023-12-25"
        );

        // 测试ISO格式
        assert_eq!(
            strftime_to_string_ymd("2023-12-25T15:30:45").unwrap(),
            "2023-12-25"
        );
        assert_eq!(
            strftime_to_string_ymd("2023-12-25T15:30").unwrap(),
            "2023-12-25"
        );
        assert_eq!(
            strftime_to_string_ymd("2023-12-25T15:30:45Z").unwrap(),
            "2023-12-25"
        );
        assert_eq!(
            strftime_to_string_ymd("2023-12-25T15:30:45.123Z").unwrap(),
            "2023-12-25"
        );
        assert_eq!(
            strftime_to_string_ymd("2023-12-25T15:30:45.123456").unwrap(),
            "2023-12-25"
        );

        // 测试不带空格的时间格式
        assert_eq!(
            strftime_to_string_ymd("2023-12-2515:30:45").unwrap(),
            "2023-12-25"
        );
        assert_eq!(
            strftime_to_string_ymd("2023-12-2515:30").unwrap(),
            "2023-12-25"
        );

        // 测试斜杠分隔符带时间
        assert_eq!(
            strftime_to_string_ymd("2023/12/25 15:30:45").unwrap(),
            "2023-12-25"
        );
        assert_eq!(
            strftime_to_string_ymd("2023/12/25 15:30").unwrap(),
            "2023-12-25"
        );
        assert_eq!(
            strftime_to_string_ymd("25/12/2023 15:30:45").unwrap(),
            "2023-12-25"
        );
        assert_eq!(
            strftime_to_string_ymd("12/25/2023 15:30:45").unwrap(),
            "2023-12-25"
        );

        // 测试点分隔符带时间
        assert_eq!(
            strftime_to_string_ymd("2023.12.25 15:30:45").unwrap(),
            "2023-12-25"
        );
        assert_eq!(
            strftime_to_string_ymd("2023.12.25 15:30").unwrap(),
            "2023-12-25"
        );

        // 测试中文格式带时间
        assert_eq!(
            strftime_to_string_ymd("2023年12月25日 15:30:45").unwrap(),
            "2023-12-25"
        );
        assert_eq!(
            strftime_to_string_ymd("2023年12月25日 15:30").unwrap(),
            "2023-12-25"
        );
        assert_eq!(
            strftime_to_string_ymd("2023年12月25日15:30:45").unwrap(),
            "2023-12-25"
        );
        assert_eq!(
            strftime_to_string_ymd("2023年12月25日15:30").unwrap(),
            "2023-12-25"
        );

        // 测试两位年份带时间
        assert_eq!(
            strftime_to_string_ymd("25/12/23 15:30:45").unwrap(),
            "2023-12-25"
        );
        assert_eq!(
            strftime_to_string_ymd("12/25/23 15:30:45").unwrap(),
            "2023-12-25"
        );

        // 测试毫秒和微秒
        assert_eq!(
            strftime_to_string_ymd("2023-12-25 15:30:45.123").unwrap(),
            "2023-12-25"
        );
        assert_eq!(
            strftime_to_string_ymd("2023-12-25 15:30:45.123456").unwrap(),
            "2023-12-25"
        );
    }

    #[test]
    fn test_strftime_to_string_ymd_english_formats() {
        // 测试英文月份格式
        assert_eq!(strftime_to_string_ymd("25 Dec 2023").unwrap(), "2023-12-25");
        assert_eq!(
            strftime_to_string_ymd("Dec 25, 2023").unwrap(),
            "2023-12-25"
        );
        assert_eq!(strftime_to_string_ymd("Dec 25 2023").unwrap(), "2023-12-25");
        assert_eq!(
            strftime_to_string_ymd("25 December 2023").unwrap(),
            "2023-12-25"
        );
        assert_eq!(
            strftime_to_string_ymd("December 25, 2023").unwrap(),
            "2023-12-25"
        );
        assert_eq!(
            strftime_to_string_ymd("December 25 2023").unwrap(),
            "2023-12-25"
        );

        // 测试英文格式带时间
        assert_eq!(
            strftime_to_string_ymd("25 Dec 2023 15:30:45").unwrap(),
            "2023-12-25"
        );

        // 测试RFC2822格式（如果支持的话，这个可能需要特殊处理）
        // assert_eq!(strftime_to_string_ymd("Mon, 25 Dec 2023 15:30:45 +0800").unwrap(), "2023-12-25");
        // assert_eq!(strftime_to_string_ymd("Mon, 25 Dec 2023 15:30:45").unwrap(), "2023-12-25");
    }

    #[test]
    fn test_get_yaml_file_not_found() {
        let result = get_yaml("non_existent_file.yaml");
        assert!(result.is_err());
    }

    #[test]
    fn test_get_yaml_settings_file_not_found() {
        let result = get_yaml_settings("non_existent_settings.yaml");
        assert!(result.is_err());
    }

    #[test]
    fn test_strftime_to_string_ymd_error_cases() {
        // 测试各种无效的日期格式
        assert!(strftime_to_string_ymd("").is_err());
        assert!(strftime_to_string_ymd("not a date").is_err());
        assert!(strftime_to_string_ymd("2023-13-01").is_err()); // 无效月份
        assert!(strftime_to_string_ymd("2023-02-30").is_err()); // 无效日期
        assert!(strftime_to_string_ymd("2023-00-01").is_err()); // 月份为0
        assert!(strftime_to_string_ymd("2023-01-00").is_err()); // 日期为0
        assert!(strftime_to_string_ymd("2023-01-32").is_err()); // 日期超出范围
        assert!(strftime_to_string_ymd("13/13/2023").is_err()); // 无效月份和日期
        assert!(strftime_to_string_ymd("32/12/2023").is_err()); // 无效日期
        assert!(strftime_to_string_ymd("abc/def/ghi").is_err()); // 非数字
        assert!(strftime_to_string_ymd("2023-ab-cd").is_err()); // 包含字母
        assert!(strftime_to_string_ymd("just text").is_err()); // 纯文本
        assert!(strftime_to_string_ymd("123").is_err()); // 太短
        assert!(strftime_to_string_ymd("2023").is_err()); // 只有年份
        assert!(strftime_to_string_ymd("2023-12").is_err()); // 缺少日期
        assert!(strftime_to_string_ymd("12/2023").is_err()); // 缺少日期
        assert!(strftime_to_string_ymd("2023--25").is_err()); // 双破折号
        assert!(strftime_to_string_ymd("2023//25").is_err()); // 双斜杠
        assert!(strftime_to_string_ymd("2023..25").is_err()); // 双点
        assert!(strftime_to_string_ymd("25:30:45").is_err()); // 只有时间没有日期
        assert!(strftime_to_string_ymd("InvalidMonth 25, 2023").is_err()); // 无效英文月份

        // 验证错误消息
        let result = strftime_to_string_ymd("invalid");
        assert!(result.is_err());
        let error_msg = result.unwrap_err().to_string();
        assert!(error_msg.contains("invalid"));
        assert!(error_msg.contains("is not a valid date"));

        // 测试空白字符
        assert!(strftime_to_string_ymd("   ").is_err());
        assert!(strftime_to_string_ymd("\t").is_err());
        assert!(strftime_to_string_ymd("\n").is_err());
    }

    #[test]
    fn test_strftime_to_string_ymd_valid_edge_cases() {
        // 测试一些边缘但有效的情况

        // 测试无效格式应该返回错误，之前的测试有错误
        let result = strftime_to_string_ymd("invalid date");
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("is not a valid date")
        );

        // 测试闰年2月29日
        assert_eq!(strftime_to_string_ymd("2024-02-29").unwrap(), "2024-02-29");
        assert_eq!(strftime_to_string_ymd("29/02/2024").unwrap(), "2024-02-29");
        assert_eq!(strftime_to_string_ymd("02/29/2024").unwrap(), "2024-02-29");

        // 测试非闰年2月29日应该失败
        assert!(strftime_to_string_ymd("2023-02-29").is_err());
        assert!(strftime_to_string_ymd("29/02/2023").is_err());

        // 测试1月1日
        assert_eq!(strftime_to_string_ymd("2023-01-01").unwrap(), "2023-01-01");
        assert_eq!(strftime_to_string_ymd("01/01/2023").unwrap(), "2023-01-01");
        assert_eq!(strftime_to_string_ymd("01/01/23").unwrap(), "2023-01-01");

        // 测试12月31日
        assert_eq!(strftime_to_string_ymd("2023-12-31").unwrap(), "2023-12-31");
        assert_eq!(strftime_to_string_ymd("31/12/2023").unwrap(), "2023-12-31");
        assert_eq!(strftime_to_string_ymd("12/31/2023").unwrap(), "2023-12-31");

        // 测试30天的月份
        assert_eq!(strftime_to_string_ymd("2023-04-30").unwrap(), "2023-04-30");
        assert_eq!(strftime_to_string_ymd("2023-06-30").unwrap(), "2023-06-30");
        assert_eq!(strftime_to_string_ymd("2023-09-30").unwrap(), "2023-09-30");
        assert_eq!(strftime_to_string_ymd("2023-11-30").unwrap(), "2023-11-30");

        // 测试31天的月份不能有31日应该失败
        assert!(strftime_to_string_ymd("2023-04-31").is_err());
        assert!(strftime_to_string_ymd("2023-06-31").is_err());
        assert!(strftime_to_string_ymd("2023-09-31").is_err());
        assert!(strftime_to_string_ymd("2023-11-31").is_err());
    }

    #[test]
    fn test_get_json_friends_links() {
        use crate::config::SettingsFriendsLinksJsonMeta;

        // 测试正常解析test_api.json文件
        let result: Result<SettingsFriendsLinksJsonMeta, _> =
            get_json_friends_links("../tests/test_api.json");
        assert!(result.is_ok());

        let data = result.unwrap();
        assert_eq!(data.friends.len(), 2);

        // 验证第一个朋友的数据
        assert_eq!(data.friends[0].len(), 4);
        assert_eq!(data.friends[0][0], "elizen");
        assert_eq!(data.friends[0][1], "https://elizen.me/");
        assert_eq!(
            data.friends[0][2],
            "https://akilar.top/images/headimage.png"
        );
        assert_eq!(data.friends[0][3], "hello.xml");

        // 验证第二个朋友的数据
        assert_eq!(data.friends[1].len(), 3);
        assert_eq!(data.friends[1][0], "Akilarの糖果屋");
        assert_eq!(data.friends[1][1], "https://akilar.top/");
        assert_eq!(
            data.friends[1][2],
            "https://akilar.top/images/headimage.png"
        );
    }

    #[test]
    fn test_get_json_friends_links_structure() {
        use crate::config::SettingsFriendsLinksJsonMeta;

        // 测试函数返回正确的结构类型
        let result: Result<SettingsFriendsLinksJsonMeta, _> =
            get_json_friends_links("../tests/test_api.json");
        assert!(result.is_ok());

        let data = result.unwrap();
        assert_eq!(data.friends.len(), 2);
        assert_eq!(data.friends[0][0], "elizen");
        assert_eq!(data.friends[1][0], "Akilarの糖果屋");

        // 验证返回的确实是SettingsFriendsLinksJsonMeta类型
        let _: SettingsFriendsLinksJsonMeta = data;
    }

    #[test]
    fn test_get_json_file_not_found() {
        use crate::config::SettingsFriendsLinksJsonMeta;

        // 测试文件不存在的情况
        let result: Result<SettingsFriendsLinksJsonMeta, _> =
            get_json_friends_links("non_existent_file.json");
        assert!(result.is_err());

        // 验证错误类型
        let error = result.unwrap_err();
        assert!(
            error.to_string().contains("No such file or directory")
                || error.to_string().contains("cannot find the file")
                || error.to_string().contains("系统找不到指定的文件")
        );
    }

    #[test]
    fn test_get_json_invalid_json() {
        use crate::config::SettingsFriendsLinksJsonMeta;
        use std::fs;
        use std::io::Write;

        // 创建一个临时的无效JSON文件
        let temp_file = "temp_invalid.json";
        let mut file = fs::File::create(temp_file).unwrap();
        writeln!(file, "{{invalid json content").unwrap();

        // 测试解析无效JSON
        let result: Result<SettingsFriendsLinksJsonMeta, _> = get_json_friends_links(temp_file);
        assert!(result.is_err());

        // 验证错误消息包含JSON解析错误信息
        let error = result.unwrap_err();
        let error_msg = error.to_string();
        assert!(
            error_msg.contains("EOF")
                || error_msg.contains("expected")
                || error_msg.contains("invalid")
                || error_msg.contains("parse")
                || error_msg.contains("missing")
                || error_msg.contains("unexpected")
                || error_msg.contains("key must be a string")
                || error_msg.contains("column")
        );

        // 清理临时文件
        fs::remove_file(temp_file).unwrap();
    }

    #[test]
    fn test_get_json_wrong_structure() {
        use crate::config::SettingsFriendsLinksJsonMeta;
        use std::fs;
        use std::io::Write;

        // 创建一个结构不匹配的JSON文件
        let temp_file = "temp_wrong_structure.json";
        let mut file = fs::File::create(temp_file).unwrap();
        writeln!(file, r#"{{"wrong_field": "value"}}"#).unwrap();

        // 测试解析结构不匹配的JSON
        let result: Result<SettingsFriendsLinksJsonMeta, _> = get_json_friends_links(temp_file);
        assert!(result.is_err());

        // 验证错误消息
        let error = result.unwrap_err();
        let error_msg = error.to_string();
        assert!(
            error_msg.contains("missing field")
                || error_msg.contains("friends")
                || error_msg.contains("expected")
        );

        // 清理临时文件
        fs::remove_file(temp_file).unwrap();
    }

    #[test]
    fn test_get_json_empty_file() {
        use crate::config::SettingsFriendsLinksJsonMeta;
        use std::fs;

        // 创建一个空文件
        let temp_file = "temp_empty.json";
        fs::File::create(temp_file).unwrap();

        // 测试解析空文件
        let result: Result<SettingsFriendsLinksJsonMeta, _> = get_json_friends_links(temp_file);
        assert!(result.is_err());

        // 验证错误消息
        let error = result.unwrap_err();
        assert!(error.to_string().contains("EOF") || error.to_string().contains("unexpected end"));

        // 清理临时文件
        fs::remove_file(temp_file).unwrap();
    }

    #[test]
    fn test_get_json_complex_structure() {
        use crate::config::SettingsFriendsLinksJsonMeta;
        use std::fs;
        use std::io::Write;

        // 创建一个更复杂的JSON文件
        let temp_file = "temp_complex.json";
        let mut file = fs::File::create(temp_file).unwrap();
        writeln!(
            file,
            r#"{{
            "friends": [
                ["name1", "https://example1.com", "avatar1.png"],
                ["name2", "https://example2.com", "avatar2.png", "feed.xml"],
                ["name3", "https://example3.com", "avatar3.png", "rss.xml", "extra_field"]
            ]
        }}"#
        )
        .unwrap();

        // 测试解析复杂结构
        let result: Result<SettingsFriendsLinksJsonMeta, _> = get_json_friends_links(temp_file);
        assert!(result.is_ok());

        let data = result.unwrap();
        assert_eq!(data.friends.len(), 3);

        // 验证数据内容
        assert_eq!(data.friends[0].len(), 3);
        assert_eq!(data.friends[1].len(), 4);
        assert_eq!(data.friends[2].len(), 5);

        assert_eq!(data.friends[0][0], "name1");
        assert_eq!(data.friends[1][3], "feed.xml");
        assert_eq!(data.friends[2][4], "extra_field");

        // 清理临时文件
        fs::remove_file(temp_file).unwrap();
    }

    #[test]
    fn test_get_json_unicode_content() {
        use crate::config::SettingsFriendsLinksJsonMeta;
        use std::fs;
        use std::io::Write;

        // 创建包含Unicode字符的JSON文件
        let temp_file = "temp_unicode.json";
        let mut file = fs::File::create(temp_file).unwrap();
        writeln!(
            file,
            r#"{{
            "friends": [
                ["用户名", "https://测试.com", "头像.png"],
                ["🚀博客", "https://example.com", "😊.jpg"],
                ["Español", "https://español.com", "niño.png"]
            ]
        }}"#
        )
        .unwrap();

        // 测试解析包含Unicode的JSON
        let result: Result<SettingsFriendsLinksJsonMeta, _> = get_json_friends_links(temp_file);
        assert!(result.is_ok());

        let data = result.unwrap();
        assert_eq!(data.friends.len(), 3);

        // 验证Unicode内容
        assert_eq!(data.friends[0][0], "用户名");
        assert_eq!(data.friends[0][1], "https://测试.com");
        assert_eq!(data.friends[1][0], "🚀博客");
        assert_eq!(data.friends[1][2], "😊.jpg");
        assert_eq!(data.friends[2][0], "Español");

        // 清理临时文件
        fs::remove_file(temp_file).unwrap();
    }

    #[test]
    fn test_calculate_content_hash() {
        // 测试基本哈希计算
        let content1 = "Hello, World!";
        let hash1 = calculate_content_hash(content1);
        assert_eq!(hash1.len(), 64); // SHA256哈希长度为64个十六进制字符

        // 测试相同内容产生相同哈希
        let content2 = "Hello, World!";
        let hash2 = calculate_content_hash(content2);
        assert_eq!(hash1, hash2);

        // 测试不同内容产生不同哈希
        let content3 = "Hello, Rust!";
        let hash3 = calculate_content_hash(content3);
        assert_ne!(hash1, hash3);

        // 测试空字符串
        let empty_content = "";
        let empty_hash = calculate_content_hash(empty_content);
        assert_eq!(empty_hash.len(), 64);
        assert_eq!(
            empty_hash,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );

        // 测试HTML内容
        let html_content = r#"
        <html>
            <head><title>Test</title></head>
            <body>
                <h1>Hello World</h1>
                <p>This is a test article.</p>
            </body>
        </html>
        "#;
        let html_hash = calculate_content_hash(html_content);
        assert_eq!(html_hash.len(), 64);

        // 测试中文内容
        let chinese_content = "你好，世界！这是一个测试文章。";
        let chinese_hash = calculate_content_hash(chinese_content);
        assert_eq!(chinese_hash.len(), 64);

        // 验证哈希格式（只包含十六进制字符）
        assert!(hash1.chars().all(|c| c.is_ascii_hexdigit()));
        assert!(html_hash.chars().all(|c| c.is_ascii_hexdigit()));
        assert!(chinese_hash.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_calculate_content_hash_stability() {
        // 测试哈希计算的稳定性（多次计算同一内容应该得到相同结果）
        let content = "Stable hash test content with special chars: !@#$%^&*()";
        let hashes: Vec<String> = (0..10).map(|_| calculate_content_hash(content)).collect();

        // 所有哈希都应该相同
        let first_hash = &hashes[0];
        for hash in &hashes {
            assert_eq!(hash, first_hash);
        }
    }

    #[test]
    fn test_calculate_content_hash_sensitivity() {
        // 测试哈希计算对微小变化的敏感性
        let base_content = "This is a test content for hash sensitivity.";
        let base_hash = calculate_content_hash(base_content);

        // 添加一个空格
        let space_content = "This is a test content for hash sensitivity. ";
        let space_hash = calculate_content_hash(space_content);
        assert_ne!(base_hash, space_hash);

        // 改变大小写
        let case_content = "This is a test content for hash Sensitivity.";
        let case_hash = calculate_content_hash(case_content);
        assert_ne!(base_hash, case_hash);

        // 添加换行符
        let newline_content = "This is a test content for hash sensitivity.\n";
        let newline_hash = calculate_content_hash(newline_content);
        assert_ne!(base_hash, newline_hash);
    }
}

pub mod html_extractor;

// 版本管理模块 - 整合自 version_manager.rs
use data_structures::version::VersionResponse;
use std::env;

/// 获取当前版本信息
///
/// 从 workspace 的统一版本配置中获取版本号，
/// 所有二进制文件（core、api）都使用相同的版本
pub fn get_version() -> VersionResponse {
    // 优先从编译时的包版本获取（来自 workspace 配置）
    // 在编译时将版本号硬编码到二进制文件中
    let version = env::var("VERSION") // 支持环境变量覆盖
        .unwrap_or_else(|_| env!("CARGO_PKG_VERSION").to_string()); // 编译时版本

    VersionResponse::new(version)
}

#[cfg(test)]
mod config_test;
