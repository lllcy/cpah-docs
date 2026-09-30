use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::LazyLock;

static ENGLISH: LazyLock<HashMap<String, String>> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../../src/locales/en.json"))
        .expect("the checked-in English catalog must be valid JSON")
});

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Language {
    #[default]
    #[serde(rename = "en")]
    English,
    #[serde(rename = "zh-CN")]
    Chinese,
}

impl Language {
    pub fn translate_health_title(self, title: &str) -> String {
        if let Some((name, label)) = title.rsplit_once(" · ")
            && matches!(label, "监控目录" | "输出目录" | "目录关系")
        {
            return format!("{name} · {}", self.translate(label));
        }
        self.translate(title)
    }

    pub fn translate(self, message: &str) -> String {
        if self == Self::English {
            ENGLISH
                .get(message)
                .cloned()
                .unwrap_or_else(|| message.to_string())
        } else {
            message.to_string()
        }
    }

    pub fn text<'a>(self, chinese: &'a str, english: &'a str) -> &'a str {
        match self {
            Self::Chinese => chinese,
            Self::English => english,
        }
    }
}

pub struct TrayMenu {
    pub show: tauri::menu::MenuItem<tauri::Wry>,
    pub quit: tauri::menu::MenuItem<tauri::Wry>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_protocol_rejects_unsupported_values_and_preserves_unknown_text() {
        assert_eq!(
            serde_json::from_str::<Language>("\"zh-CN\"").unwrap(),
            Language::Chinese
        );
        assert_eq!(
            serde_json::from_str::<Language>("\"en\"").unwrap(),
            Language::English
        );
        assert!(serde_json::from_str::<Language>("\"unknown\"").is_err());
        assert_eq!(Language::English.translate("任务数据库"), "Task database");
        assert_eq!(Language::Chinese.translate("任务数据库"), "任务数据库");
        assert_eq!(Language::English.translate("审计资料.pdf"), "审计资料.pdf");
        assert_eq!(
            Language::English.translate_health_title("项目 · 监控目录 · 输出目录"),
            "项目 · 监控目录 · Output folder"
        );
        assert_eq!(
            Language::Chinese.translate_health_title("项目 · 监控目录 · 输出目录"),
            "项目 · 监控目录 · 输出目录"
        );
    }
}
