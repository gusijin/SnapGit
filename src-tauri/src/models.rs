
// 挂在某个提交上的引用（分支/远程分支/标签），供日志面板像 SourceTree 那样内联显示
#[derive(Debug, serde::Serialize)]
pub(crate) struct CommitRef {
    pub(crate) name: String,
    #[serde(rename = "type")]
    pub(crate) ref_type: String,
}

#[derive(Debug, serde::Serialize)]
pub(crate) struct Commit {
    pub(crate) id: String,
    pub(crate) message: String,
    pub(crate) author: String,
    pub(crate) date: String,
    // 父提交 id 列表（合并提交有多个），前端据此绘制提交图谱
    pub(crate) parents: Vec<String>,
    pub(crate) refs: Vec<CommitRef>,
}

#[derive(Debug, serde::Serialize)]
pub(crate) struct Branch {
    pub(crate) name: String,
    pub(crate) is_current: bool,
    pub(crate) is_remote: bool,
    pub(crate) ahead: usize,
    pub(crate) behind: usize,
}

#[derive(Debug, serde::Serialize)]
pub(crate) struct UpstreamInfo {
    pub(crate) remote: String,
    pub(crate) remote_branch: String,
}

/// 检测远程分支是否可被当前用户删除。
/// 通过两步判断：
/// 1. 协议快速检查：`git://` 协议只读，直接判不可写。
/// 2. `git push --dry-run <remote> :<remote_branch>` 试运行：捕获服务端拒绝原因，
///    若错误包含"protected / denied / permission / forbidden"等关键词则不可删；
///    其他错误（如认证失败、网络错误）视为"未知"，让用户实际执行时再试。
#[derive(Debug, serde::Serialize)]
pub(crate) struct RemoteDeleteCheck {
    pub(crate) can_delete: bool,
    /// 当 can_delete=false 时，填写中文简短原因，前端用作 tooltip。
    pub(crate) reason: Option<String>,
}

#[derive(Debug, serde::Serialize, Clone)]
pub(crate) struct FileStatus {
    pub(crate) path: String,
    pub(crate) status: String,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
pub(crate) struct ScannedProject {
    pub(crate) path: String,
    pub(crate) name: String,
    pub(crate) vcs_type: String,
}

#[derive(Debug, serde::Serialize)]
pub(crate) struct FileTreeNode {
    pub(crate) name: String,
    pub(crate) path: String,
    pub(crate) is_dir: bool,
    pub(crate) children: Vec<FileTreeNode>,
}

#[derive(Debug, serde::Serialize, Clone)]
pub(crate) struct DiffSegment {
    pub(crate) text: String,
    pub(crate) changed: bool,
}

#[derive(Debug, serde::Serialize, Clone)]
pub(crate) struct DiffLine {
    pub(crate) line_type: String,
    pub(crate) content: String,
    pub(crate) new_content: String,
    pub(crate) old_line: Option<usize>,
    pub(crate) new_line: Option<usize>,
    pub(crate) old_segments: Vec<DiffSegment>,
    pub(crate) new_segments: Vec<DiffSegment>,
}

#[derive(Debug, serde::Serialize)]
pub(crate) struct FileDiff {
    pub(crate) old_content: Vec<String>,
    pub(crate) new_content: Vec<String>,
    pub(crate) lines: Vec<DiffLine>,
    /// 二进制文件（exe / 图片 / 压缩包等）：不做逐行 diff，前端展示「无法预览」提示
    pub(crate) is_binary: bool,
    /// 超大文本文件：行数超过 MAX_DIFF_LINES，跳过 O(m*n) 的 LCS 计算（否则会卡死/OOM），
    /// 前端降级为「仅展示可编辑、无差异高亮」并提示用户
    pub(crate) is_oversized: bool,
    /// 文件「实际」行尾符：工作区模式 = 工作区文件本身；提交模式 = 当前提交版本。
    /// 取值 "CRLF" / "LF" / None（无换行符）。
    pub(crate) eol_actual: Option<String>,
    /// 文件「期望」行尾符：工作区模式 = git 按 core.autocrlf / core.eol / .gitattributes 期望的形态；
    /// 提交模式 = 父提交版本。`eol_actual != eol_expected` 即「内容字符一致、但 git 认为有差异」的根因
    ///（行尾规范化：如 autocrlf=true 期望 CRLF，而文件实际是 LF）。
    pub(crate) eol_expected: Option<String>,
}

#[derive(Debug, serde::Serialize, Clone)]
pub(crate) struct ConflictBlock {
    pub(crate) start_line: usize,
    pub(crate) separator_line: usize,
    pub(crate) end_line: usize,
    pub(crate) ours: Vec<String>,
    pub(crate) theirs: Vec<String>,
    pub(crate) base: Option<Vec<String>>,
    pub(crate) marker_ours: String,
    pub(crate) marker_theirs: String,
    /// 畸形冲突文件专用：本块内「位置不对的标记行」行号（0-based，相对 working 文档）。
    /// 例如 `>>>>>>> ` 出现在 `=======` 之前 —— 这些行不属于任何一侧内容，前端渲染时标灰。
    /// 标准冲突文件此字段为空数组。前端 ConflictSolver 会自行重算，这里保持字段对齐即可。
    #[serde(default)]
    pub(crate) stray_marker_lines: Vec<usize>,
}

#[derive(Debug, serde::Serialize)]
pub(crate) struct ConflictFile {
    pub(crate) path: String,
    pub(crate) ours_content: Vec<String>,
    pub(crate) theirs_content: Vec<String>,
    pub(crate) base_content: Option<Vec<String>>,
    pub(crate) working_content: Vec<String>,
    pub(crate) blocks: Vec<ConflictBlock>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
pub(crate) struct RepositoryInfo {
    pub(crate) path: String,
    pub(crate) name: String,
    pub(crate) current_branch: String,
}

#[derive(Debug, serde::Serialize, Clone)]
pub(crate) struct SubmoduleInfo {
    /// 子模块在父仓库中的相对路径
    pub(crate) path: String,
    /// 展示名（取 path 最后一段）
    pub(crate) name: String,
    /// 远程地址（来自 .gitmodules）
    pub(crate) url: String,
    /// 跟踪分支（来自 .gitmodules，可空）
    pub(crate) branch: Option<String>,
    /// 当前已检出的提交完整 SHA；未初始化为 None
    pub(crate) head_commit: Option<String>,
    /// 当前已检出的提交短 SHA（前 7 位），用于界面展示；未初始化为 None
    pub(crate) head_commit_short: Option<String>,
    /// 父仓库记录的 gitlink 提交 SHA（来自索引）；未记录为 None
    pub(crate) recorded_commit: Option<String>,
    /// 是否已初始化（工作区已克隆内容）
    pub(crate) initialized: bool,
    /// 指针是否改变：已初始化且当前 HEAD ≠ 记录值
    pub(crate) modified: bool,
    /// 子模块工作树是否有未提交改动
    pub(crate) dirty: bool,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct RecentRepositories {
    pub(crate) repos: Vec<RepositoryInfo>,
}

