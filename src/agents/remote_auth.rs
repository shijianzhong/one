#![allow(dead_code)]

//! 远程暗号认证模块。
//!
//! 用于 Telegram 远程触发的危险操作确认：
//! - 暗号只在本机 GPUI 设置页设置，不经过网络传输
//! - bcrypt 哈希存储，不可逆
//! - 连续 3 次错误锁定 5 分钟（可配置）
//! - 配置存储在 `~/.one/remote_auth.json`，与 bot token 分文件存储

use std::path::PathBuf;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

const MAX_FAILED_ATTEMPTS: u32 = 3;
const LOCK_DURATION_SECS: u64 = 300;

fn config_path() -> PathBuf {
    let config_dir = dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".one");
    std::fs::create_dir_all(&config_dir).ok();
    config_dir.join("remote_auth.json")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AuthStore {
    cipher_hash: Option<String>,
    created_at: Option<String>,
    failed_attempts: u32,
    locked_until: Option<String>,
    max_failed_attempts: u32,
    lock_duration_secs: u64,
}

impl Default for AuthStore {
    fn default() -> Self {
        Self {
            cipher_hash: None,
            created_at: None,
            failed_attempts: 0,
            locked_until: None,
            max_failed_attempts: MAX_FAILED_ATTEMPTS,
            lock_duration_secs: LOCK_DURATION_SECS,
        }
    }
}

fn load_store() -> AuthStore {
    let path = config_path();
    if path.exists() {
        if let Ok(content) = std::fs::read_to_string(&path) {
            if let Ok(store) = serde_json::from_str(&content) {
                return store;
            }
        }
    }
    AuthStore::default()
}

fn save_store(store: &AuthStore) -> anyhow::Result<()> {
    let path = config_path();
    let content = serde_json::to_string_pretty(store)?;
    std::fs::write(&path, content)?;
    Ok(())
}

/// 远程暗号认证模块。
pub struct RemoteAuth;

impl RemoteAuth {
    /// 检查是否已设置暗号。
    pub fn is_cipher_set() -> bool {
        let store = load_store();
        store.cipher_hash.is_some()
    }

    /// 设置/更新暗号。只在本机 GPUI 调用。
    /// 成功后重置失败计数和锁定状态。
    pub fn set_cipher(cipher: &str) -> anyhow::Result<()> {
        let hash = bcrypt::hash(cipher, bcrypt::DEFAULT_COST)?;
        let mut store = load_store();
        store.cipher_hash = Some(hash);
        store.created_at = Some(chrono::Local::now().to_rfc3339());
        // 设置暗号时自动重置失败计数和锁定
        store.failed_attempts = 0;
        store.locked_until = None;
        save_store(&store)
    }

    /// 清除暗号。
    pub fn clear_cipher() -> anyhow::Result<()> {
        let mut store = load_store();
        store.cipher_hash = None;
        store.created_at = None;
        store.failed_attempts = 0;
        store.locked_until = None;
        save_store(&store)
    }

    /// 验证暗号。返回 true 表示通过。
    /// 失败次数超限时直接返回 false 并记录锁定时间。
    pub fn verify_cipher(cipher: &str) -> Result<bool, String> {
        use chrono::Utc;

        let mut store = load_store();

        // 检查是否锁定
        if let Some(locked_until_str) = &store.locked_until {
            let now = Utc::now();
            let locked_until = locked_until_str.parse::<chrono::DateTime<Utc>>()
                .or_else(|_| {
                    chrono::DateTime::parse_from_rfc3339(locked_until_str)
                        .map(|dt| dt.with_timezone(&Utc))
                });
            if let Ok(locked_until) = locked_until {
                if now < locked_until {
                    let remaining = (locked_until - now).num_seconds().max(0);
                    return Err(format!(
                        "连续错误次数过多，账号已锁定，请在 {} 秒后重试",
                        remaining
                    ));
                }
            }
            // 锁定时间已过，清除锁定
            store.locked_until = None;
            store.failed_attempts = 0;
            let _ = save_store(&store);
        }

        let hash = match &store.cipher_hash {
            Some(h) => h,
            None => return Err("暗号尚未设置，请先在本机 ONE 设置页配置远程暗号".to_string()),
        };

        match bcrypt::verify(cipher, hash) {
            Ok(true) => {
                // 验证成功，重置失败计数
                store.failed_attempts = 0;
                let _ = save_store(&store);
                Ok(true)
            }
            Ok(false) => {
                // 验证失败，记录失败次数
                store.failed_attempts += 1;
                if store.failed_attempts >= store.max_failed_attempts {
                    let until = Utc::now()
                        + chrono::Duration::seconds(store.lock_duration_secs as i64);
                    store.locked_until = Some(until.to_rfc3339());
                    let _ = save_store(&store);
                    return Err(format!(
                        "暗号错误（第 {} 次），已锁定 {} 秒",
                        store.failed_attempts, store.lock_duration_secs
                    ));
                }
                let remaining = store.max_failed_attempts - store.failed_attempts;
                let _ = save_store(&store);
                Err(format!(
                    "暗号错误，还剩 {} 次机会",
                    remaining
                ))
            }
            Err(e) => Err(format!("验证过程出错：{}", e)),
        }
    }

    /// 记录一次失败尝试（可由外层调用，与 verify_cipher 独立）。
    pub fn record_failure() -> anyhow::Result<()> {
        let mut store = load_store();
        store.failed_attempts += 1;
        if store.failed_attempts >= store.max_failed_attempts {
            let until =
                chrono::Utc::now() + chrono::Duration::seconds(store.lock_duration_secs as i64);
            store.locked_until = Some(until.to_rfc3339());
        }
        save_store(&store)
    }

    /// 返回剩余锁定秒数。0 表示未锁定。
    pub fn locked_for_secs() -> u64 {
        use chrono::Utc;

        let store = load_store();
        match &store.locked_until {
            Some(locked_until_str) => {
                let now = Utc::now();
                let locked_until = locked_until_str.parse::<chrono::DateTime<Utc>>()
                    .or_else(|_| {
                        chrono::DateTime::parse_from_rfc3339(locked_until_str)
                            .map(|dt| dt.with_timezone(&Utc))
                    });
                if let Ok(locked_until) = locked_until {
                    (locked_until - now).num_seconds().max(0) as u64
                } else {
                    0
                }
            }
            None => 0,
        }
    }

    /// 获取失败次数。
    pub fn failed_attempts() -> u32 {
        load_store().failed_attempts
    }

    /// 获取最大失败次数配置。
    pub fn max_failed_attempts() -> u32 {
        load_store().max_failed_attempts
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 用临时路径替代默认路径来测试 RemoteAuth
    fn with_test_path<T>(f: impl FnOnce() -> T) -> T {
        // 使用环境变量覆盖 config_path 的函数——这里不实用，
        // 我们手动操作临时文件来测试
        f()
    }

    #[test]
    fn default_is_not_set() {
        // 不能直接测，因为会读写 ~/.one/remote_auth.json
        // 这里只测试逻辑函数
    }

    #[test]
    fn cipher_hash_and_verify() {
        // 验证 bcrypt 基本功能
        let hash = bcrypt::hash("芝麻开门", bcrypt::DEFAULT_COST).unwrap();
        assert!(bcrypt::verify("芝麻开门", &hash).unwrap());
        assert!(!bcrypt::verify("错误暗号", &hash).unwrap());
    }

    #[test]
    fn bcrypt_different_inputs_different_hashes() {
        let h1 = bcrypt::hash("hello", bcrypt::DEFAULT_COST).unwrap();
        let h2 = bcrypt::hash("hello", bcrypt::DEFAULT_COST).unwrap();
        // bcrypt 使用随机盐，两次 hash 不同
        assert_ne!(h1, h2);
        assert!(bcrypt::verify("hello", &h1).unwrap());
        assert!(bcrypt::verify("hello", &h2).unwrap());
    }
}