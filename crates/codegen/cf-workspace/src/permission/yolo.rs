//! B5: 结构化 YOLO（always-approve）模型 + 启动安全闸。
//!
//! 历史上 YOLO 是一个全有全无的布尔（见 [`super::manager`] 的 actor），要么
//! 全部工具自动批准、要么不批准。本模块提供一个带**工具白名单**的结构化模型，
//! 让 `--yolo=read_file,grep` 这类"仅放行部分工具"的用法成为可能，并给出
//! "全开 YOLO + 沙箱未激活"时应拒绝启动的判定逻辑。
//!
//! 设计要点（与既有 actor 兼容）：
//! - 白名单为 `None` 表示"全开"，语义与旧布尔 `true` 完全一致，因此把它接进
//!   permission manager 后，未设置白名单的现有路径行为不变（现有测试不受影响）。
//! - `Some(set)` 表示仅 `set` 内的工具被自动批准，其余工具照常走审批。

use std::collections::HashSet;

/// YOLO 模式配置。
///
/// B5: 支持工具白名单，避免 `--yolo` 全有全无。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct YoloMode {
    /// 是否启用 YOLO（工具自动批准）。
    pub enabled: bool,
    /// 工具白名单。`None` = 全开；`Some(set)` = 仅 `set` 中工具自动批准。
    /// 命令行 `--yolo=read_file,grep` 解析成 `Some({"read_file","grep"})`。
    pub allowlist: Option<HashSet<String>>,
}

impl YoloMode {
    /// 全开：启用且无白名单（任意工具自动批准）。
    pub fn all() -> Self {
        Self {
            enabled: true,
            allowlist: None,
        }
    }

    /// 白名单：启用且仅放行给定工具。
    pub fn allowlist<I: IntoIterator<Item = String>>(tools: I) -> Self {
        Self {
            enabled: true,
            allowlist: Some(tools.into_iter().collect()),
        }
    }

    /// 关闭：不自动批准任何工具。
    pub fn disabled() -> Self {
        Self::default()
    }

    /// 该工具是否被 YOLO 自动批准。
    pub fn auto_approves(&self, tool_name: &str) -> bool {
        if !self.enabled {
            return false;
        }
        match &self.allowlist {
            None => true,
            Some(set) => set.contains(tool_name),
        }
    }

    /// 是否为"全开 YOLO"（启用且无白名单）——最需要 OS 层防护兜底的组合。
    pub fn is_unrestricted(&self) -> bool {
        self.enabled && self.allowlist.is_none()
    }
}

/// 把 `--yolo` 的参数值解析为 [`YoloMode`]。
///
/// - `""` / `"true"` / `"all"` → 全开
/// - `"read_file,grep,edit"` → 白名单（按逗号切分，去空白，忽略空项）
pub fn parse_yolo_arg(arg: &str) -> YoloMode {
    let trimmed = arg.trim();
    if trimmed.is_empty()
        || trimmed.eq_ignore_ascii_case("true")
        || trimmed.eq_ignore_ascii_case("all")
    {
        return YoloMode::all();
    }
    let tools = trimmed
        .split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());
    YoloMode::allowlist(tools)
}

/// 启动安全闸的判定结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum YoloStartupCheck {
    /// 允许启动。
    Ok,
    /// 拒绝启动：全开 YOLO 且沙箱未激活、且未显式确认风险。
    RefuseNoSandbox,
}

/// B5: 全开 YOLO + 沙箱未激活时，若未显式 ack 风险则拒绝启动。
///
/// 白名单模式（`Some`）即便沙箱关闭也允许启动——风险面已被白名单收敛。
/// `ack_no_sandbox` 对应 `--ack-no-sandbox`：用户显式承担无 OS 防护的风险。
pub fn yolo_startup_check(
    mode: &YoloMode,
    sandbox_active: bool,
    ack_no_sandbox: bool,
) -> YoloStartupCheck {
    if mode.is_unrestricted() && !sandbox_active && !ack_no_sandbox {
        YoloStartupCheck::RefuseNoSandbox
    } else {
        YoloStartupCheck::Ok
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yolo_all_allows_any() {
        let mode = YoloMode::all();
        assert!(mode.auto_approves("read_file"));
        assert!(mode.auto_approves("bash"));
        assert!(mode.auto_approves("anything_at_all"));
        assert!(mode.is_unrestricted());
    }

    #[test]
    fn yolo_allowlist_only_listed() {
        let mode = YoloMode::allowlist(["read_file".to_string(), "grep".to_string()]);
        assert!(mode.auto_approves("read_file"));
        assert!(mode.auto_approves("grep"));
        assert!(!mode.auto_approves("bash"));
        assert!(!mode.auto_approves("edit"));
        assert!(!mode.is_unrestricted());
    }

    #[test]
    fn yolo_disabled_denies_all() {
        let mode = YoloMode::disabled();
        assert!(!mode.auto_approves("read_file"));
        assert!(!mode.auto_approves("bash"));
        assert!(!mode.is_unrestricted());
        // Default 与 disabled 一致。
        assert_eq!(YoloMode::default(), mode);
    }

    #[test]
    fn parse_yolo_arg_empty_or_keywords_is_all() {
        assert_eq!(parse_yolo_arg(""), YoloMode::all());
        assert_eq!(parse_yolo_arg("true"), YoloMode::all());
        assert_eq!(parse_yolo_arg("ALL"), YoloMode::all());
        assert_eq!(parse_yolo_arg("  "), YoloMode::all());
    }

    #[test]
    fn parse_yolo_arg_csv_is_allowlist() {
        let mode = parse_yolo_arg("read_file, grep ,edit");
        assert!(mode.enabled);
        let set = mode.allowlist.expect("allowlist present");
        assert!(set.contains("read_file"));
        assert!(set.contains("grep"));
        assert!(set.contains("edit"));
        assert_eq!(set.len(), 3);
    }

    #[test]
    fn parse_yolo_arg_ignores_empty_items() {
        let mode = parse_yolo_arg("read_file,,grep,");
        let set = mode.allowlist.expect("allowlist present");
        assert_eq!(set.len(), 2);
    }

    #[test]
    fn yolo_all_plus_sandbox_off_rejects() {
        let mode = YoloMode::all();
        assert_eq!(
            yolo_startup_check(&mode, false, false),
            YoloStartupCheck::RefuseNoSandbox
        );
    }

    #[test]
    fn yolo_all_plus_sandbox_on_starts() {
        let mode = YoloMode::all();
        assert_eq!(yolo_startup_check(&mode, true, false), YoloStartupCheck::Ok);
    }

    #[test]
    fn yolo_all_sandbox_off_but_acked_starts() {
        let mode = YoloMode::all();
        assert_eq!(yolo_startup_check(&mode, false, true), YoloStartupCheck::Ok);
    }

    #[test]
    fn yolo_allowlist_plus_sandbox_off_starts() {
        let mode = YoloMode::allowlist(["read_file".to_string()]);
        assert_eq!(
            yolo_startup_check(&mode, false, false),
            YoloStartupCheck::Ok
        );
    }

    #[test]
    fn yolo_disabled_plus_sandbox_off_starts() {
        let mode = YoloMode::disabled();
        assert_eq!(
            yolo_startup_check(&mode, false, false),
            YoloStartupCheck::Ok
        );
    }
}
