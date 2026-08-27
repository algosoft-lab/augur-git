//! M2：配置数据层——仓库/视图设置，config.json 持久化
//!
//! 存储位置：%APPDATA%\augur-git\config.json（Store 沙箱兼容）

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// 用户选择的语言偏好（镜像 augur-pdf/augur-term config.rs）。
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub enum LanguagePreference {
    /// 跟随操作系统语言。
    #[serde(rename = "system")]
    System,
    #[serde(rename = "en-US", alias = "en")]
    English,
    #[serde(rename = "zh-CN", alias = "zh")]
    SimplifiedChinese,
}

impl Default for LanguagePreference {
    fn default() -> Self {
        Self::System
    }
}

/// 仓库参数（侧栏配置面板，单一来源）
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct RepoParams {
    /// 仓库路径（空 = 未选择）
    pub path: String,
}

impl Default for RepoParams {
    fn default() -> Self {
        Self {
            path: String::new(),
        }
    }
}

/// 视图设置
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ViewSettings {
    /// 显示未跟踪文件
    pub show_untracked: bool,
    /// 日志视图自动跟随（M3 提交历史引入）
    pub auto_follow: bool,
}

impl Default for ViewSettings {
    fn default() -> Self {
        Self {
            show_untracked: true,
            auto_follow: true,
        }
    }
}

/// 应用配置（单一事实源，Workspace 持有；任一变更即存盘）
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct AppConfig {
    /// 界面语言；`system` 跟随操作系统。字段缺失时回落「跟随系统」。
    #[serde(default)]
    pub language: LanguagePreference,
    #[serde(default)]
    pub repo: RepoParams,
    #[serde(default)]
    pub view: ViewSettings,
    /// 最近打开的仓库（MRU，最多 8 个，侧栏快速切换）
    #[serde(default)]
    pub recent_repos: Vec<String>,
}

impl AppConfig {
    /// 记录一个最近仓库（去重、置顶、截断）
    pub fn push_recent(&mut self, path: &str) {
        self.recent_repos.retain(|p| p != path);
        self.recent_repos.insert(0, path.to_string());
        self.recent_repos.truncate(8);
    }
}

/// 存储文件路径
pub fn store_path() -> PathBuf {
    let base = std::env::var("APPDATA").unwrap_or_else(|_| ".".into());
    let dir = PathBuf::from(base).join("augur-git");
    let _ = std::fs::create_dir_all(&dir);
    dir.join("config.json")
}

/// 加载配置（文件不存在或解析失败时返回默认值）
pub fn load() -> AppConfig {
    let path = store_path();
    match std::fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str(&text).unwrap_or_else(|e| {
            log::warn!("[config] failed to parse config; using defaults: {e}");
            AppConfig::default()
        }),
        Err(_) => AppConfig::default(),
    }
}

/// 保存配置
pub fn save(config: &AppConfig) -> anyhow::Result<()> {
    let text = serde_json::to_string_pretty(config)?;
    std::fs::write(store_path(), text)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_roundtrip() {
        let mut c = AppConfig::default();
        c.repo.path = r"D:\dev\gitee\augur-git".into();
        c.view.show_untracked = false;
        let text = serde_json::to_string(&c).unwrap();
        let back: AppConfig = serde_json::from_str(&text).unwrap();
        assert_eq!(back.repo.path, r"D:\dev\gitee\augur-git");
        assert!(!back.view.show_untracked);
    }

    #[test]
    fn defaults_are_sane() {
        let c = AppConfig::default();
        assert!(c.view.show_untracked);
        assert!(c.recent_repos.is_empty());
    }

    #[test]
    fn recent_repos_dedup_and_truncate() {
        let mut c = AppConfig::default();
        for i in 0..10 {
            c.push_recent(&format!("repo{i}"));
        }
        assert_eq!(c.recent_repos.len(), 8);
        c.push_recent("repo3");
        assert_eq!(c.recent_repos.first().map(String::as_str), Some("repo3"));
        assert_eq!(c.recent_repos.iter().filter(|p| *p == "repo3").count(), 1);
    }

    #[test]
    fn language_preference_round_trips() {
        let json = r#"{"language":"zh-CN"}"#;
        let config: AppConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.language, LanguagePreference::SimplifiedChinese);

        let serialized = serde_json::to_string(&AppConfig::default()).unwrap();
        assert!(serialized.contains(r#""system""#));
    }

    #[test]
    fn accepts_language_alias() {
        let json = r#"{"language":"zh"}"#;
        let config: AppConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.language, LanguagePreference::SimplifiedChinese);
    }

    #[test]
    fn missing_language_field_defaults_to_system() {
        let json = r#"{"repo":{"path":""}}"#;
        let config: AppConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.language, LanguagePreference::System);
    }
}
