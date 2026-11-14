use super::*;


// ===== 文件差异 =====

/// git 判定二进制的启发式：前 8000 字节内出现 NUL 字节即视为二进制
/// （与 Git 的 buffer_is_binary 一致，避免把 exe/图片/zip 当文本逐行 diff 出乱码）
pub(crate) fn is_binary_bytes(bytes: &[u8]) -> bool {
    const CHECK_LEN: usize = 8000;
    let end = bytes.len().min(CHECK_LEN);
    bytes[..end].contains(&0)
}

/// 根据原始字节检测文件主要换行符类型。
/// 若包含 CRLF 则判为 "CRLF"；否则若包含 LF 则判为 "LF"；无换行符返回 None。
/// 返回值直接采用 git 语义（CRLF / LF），前端无需再次映射。
pub(crate) fn detect_eol(bytes: &[u8]) -> Option<String> {
    if bytes.windows(2).any(|w| w == b"\r\n") {
        Some("CRLF".to_string())
    } else if bytes.contains(&b'\n') {
        Some("LF".to_string())
    } else {
        None
    }
}

/// 推断 git 期望该文件在工作区「应该」使用的行尾符（即 checkout 后的形态）。
///
/// git 在 diff 时会先把工作区与索引做行尾规范化再比较，所以「内容字符一致、但版本显示有差异」
/// 的常见根因就是：工作区实际 EOL 与 git 期望 EOL 不一致（最典型是 `core.autocrlf=true`
/// 在 Windows 上期望 CRLF，而文件实际是 LF）。
///
/// 依据（与 git 同优先级）：core.autocrlf / core.eol（libgit2 已合并 local+global+system 配置）。
///   - autocrlf=true  → CRLF（与 OS 无关，git 规定 checkout 转 CRLF）
///   - autocrlf=input → LF
///   - autocrlf=false/未设 + core.eol=crlf|lf|native → 用 core.eol（native 跟 OS）
///   - 其余           → None（git 不强制，按文件实际字节）
///
/// 注：仓库级 `.gitattributes` 的 `eol`/`text` 属性此处未覆盖（多数 Windows 项目依赖 autocrlf）；
/// 若需精确到 .gitattributes 的 eol 规则，后续可调用 `repo.get_attr` 补充。
pub(crate) fn git_expected_eol(repo: &Repository) -> Option<String> {
    let cfg = repo.config().ok()?;
    let autocrlf = cfg.get_string("core.autocrlf").ok();
    let core_eol = cfg.get_string("core.eol").ok();
    match autocrlf.as_deref() {
        Some("true") => Some("CRLF".to_string()),
        Some("input") => Some("LF".to_string()),
        _ => match core_eol.as_deref() {
            Some("crlf") => Some("CRLF".to_string()),
            Some("lf") => Some("LF".to_string()),
            Some("native") => Some(if cfg!(windows) { "CRLF" } else { "LF" }.to_string()),
            _ => None,
        },
    }
}

/// 读取工作区文件内容（按行），并判定是否为二进制。
/// 返回 (行内容, 是否二进制, 行尾符类型)；二进制时行内容为空。
/// 非 UTF-8 但不含 NUL（如 GBK 文本）按 lossy 转换，保证内容可见而不是整片空白。
pub(crate) fn read_file_lines(path: &Path) -> (Vec<String>, bool, Option<String>) {
    let bytes = match fs::read(path) {
        Ok(b) => b,
        Err(_) => return (Vec::new(), false, None),
    };
    let eol = detect_eol(&bytes);
    if is_binary_bytes(&bytes) {
        return (Vec::new(), true, eol);
    }
    match String::from_utf8(bytes) {
        Ok(content) => (content.lines().map(|l| l.to_string()).collect(), false, eol),
        Err(e) => (
            String::from_utf8_lossy(e.as_bytes())
                .lines()
                .map(|l| l.to_string())
                .collect(),
            false,
            eol,
        ),
    }
}

/// Myers 差分的一步编辑操作（按旧文件行推进）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DiffOp {
    Equal,
    Delete,
    Insert,
}

/// Myers O(ND) 差分：返回从 `old` 变换到 `new` 的编辑脚本。
/// 采用经典的「贪心蛇 + 对角线搜索」实现（Eugene W. Myers 1986，`git diff` 同款核心思路）。
/// 空间复杂度 O(N+M)（只存 V 数组 + 回溯 trace），远优于旧版 O(N·M) DP 表。
pub(crate) fn myers_diff(old: &[String], new: &[String]) -> Vec<DiffOp> {
    let n = old.len();
    let m = new.len();

    // 空文件特判：避免 V 数组的 k±1 越界（n=0 时 max 很小仍会访问 k+1）
    if n == 0 && m == 0 {
        return Vec::new();
    }
    if n == 0 {
        return vec![DiffOp::Insert; m];
    }
    if m == 0 {
        return vec![DiffOp::Delete; n];
    }

    let max = n + m;

    // V[k] 记录「第 d 步」沿对角线 k = x - y 能到达的最远 x。
    // 用滚动的一维数组，避免 O(D·(N+M)) 的完整 trace 存储；但回溯需要每步的快照，
    // 因此这里存 trace：trace[d][k] = 到达该步时对角线 k 的最远 x。
    // 长度 +2 留出 k±1 边界缓冲，避免 k = ±d 时访问越界。
    let mut v = vec![0isize; 2 * max + 3];
    // offset 让负对角线也能索引：k ∈ [-max, max] → idx = k + max + 1
    let offset = (max + 1) as isize;

    // trace：每一步保存整个 V 的拷贝（只有 O(D) 步，D 通常远小于 N+M）
    let mut trace: Vec<Vec<isize>> = Vec::new();

    for d in 0..=max {
        trace.push(v.clone());
        let d = d as isize;
        let mut k = -d;
        while k <= d {
            // 决定走哪条路径：向下（删除，x+1）还是向右（插入，y+1）
            let idx = (k + offset) as usize;
            let mut x = if k == -d || (k != d && v[idx - 1] < v[idx + 1]) {
                // 向下走：从 k+1 的对角线，x = v[k+1]（删除：x 不变时 y 增）
                v[idx + 1]
            } else {
                // 向右走：从 k-1 的对角线，x = v[k-1] + 1（插入）
                v[idx - 1] + 1
            };

            let mut y = x - k;

            // 贪心蛇：沿对角线尽量吃相同行
            while (x as usize) < n && (y as usize) < m && old[x as usize] == new[y as usize] {
                x += 1;
                y += 1;
            }

            v[idx] = x;

            if (x as usize) >= n && (y as usize) >= m {
                // 到达终点 (n, m)，回溯生成编辑脚本
                return backtrack(&trace, old, new, d as usize);
            }

            k += 2;
        }
    }

    // 理论上不会到这里（max = n+m 一定可达）
    Vec::new()
}

/// 根据 trace 从终点回溯出编辑脚本。
pub(crate) fn backtrack(
    trace: &[Vec<isize>],
    old: &[String],
    new: &[String],
    final_d: usize,
) -> Vec<DiffOp> {
    let n = old.len();
    let m = new.len();
    let max = n + m;
    let offset = (max + 1) as isize; // 与 myers_diff 的 v 索引体系一致（含 k±1 缓冲）

    let mut ops: Vec<DiffOp> = Vec::new();
    let mut x = n as isize;
    let mut y = m as isize;

    for d in (0..=final_d).rev() {
        let v = &trace[d];
        let k = x - y;
        let idx = (k + offset) as usize;

        // 判断这一步是从哪个方向进来的
        let prev_k = if k == -(d as isize)
            || (k != d as isize && v[idx - 1] < v[idx + 1])
        {
            k + 1 // 从「向下删除」方向来
        } else {
            k - 1 // 从「向右插入」方向来
        };

        let prev_idx = (prev_k + offset) as usize;
        let prev_x = v[prev_idx];
        let prev_y = prev_x - prev_k;

        // 先回退贪心蛇吃掉的相同行（Equal）
        while x > prev_x && y > prev_y {
            ops.push(DiffOp::Equal);
            x -= 1;
            y -= 1;
        }

        if d == 0 {
            break;
        }

        if x == prev_x {
            // 这一步是插入（x 不变，y 增）
            ops.push(DiffOp::Insert);
            y -= 1;
        } else {
            // 这一步是删除（y 不变，x 增）
            ops.push(DiffOp::Delete);
            x -= 1;
        }
    }

    // 起点处可能还有剩余的相同行
    while x > 0 && y > 0 {
        ops.push(DiffOp::Equal);
        x -= 1;
        y -= 1;
    }

    ops.reverse();
    ops
}

pub(crate) fn lcs_diff(old: &[String], new: &[String]) -> Vec<DiffLine> {
    // Myers 差分算法（git 同款）：时间复杂度 O((N+M)·D)，D 为实际差异量；
    // 空间复杂度 O(N+M) 线性（相比旧版 O(N·M) 全量 DP 表，几万行不再 OOM）。
    // 两个文件越接近（D 越小）跑得越快；完全不相关的最坏情况 O((N+M)^2)，但
    // 由 get_file_diff 的 MAX_DIFF_LINES 硬上限兜底，不会真正卡死。
    let ops = myers_diff(old, new);

    // 回溯生成 DiffLine（old_line/new_line 用 1-based 行号）
    let mut lines: Vec<DiffLine> = Vec::with_capacity(ops.len());
    let mut old_line: usize = 1;
    let mut new_line: usize = 1;

    for op in &ops {
        match op {
            DiffOp::Equal => {
                lines.push(DiffLine {
                    line_type: "context".to_string(),
                    content: old[old_line - 1].clone(),
                    new_content: String::new(),
                    old_line: Some(old_line),
                    new_line: Some(new_line),
                    old_segments: Vec::new(),
                    new_segments: Vec::new(),
                });
                old_line += 1;
                new_line += 1;
            }
            DiffOp::Delete => {
                lines.push(DiffLine {
                    line_type: "delete".to_string(),
                    content: old[old_line - 1].clone(),
                    new_content: String::new(),
                    old_line: Some(old_line),
                    new_line: None,
                    old_segments: Vec::new(),
                    new_segments: Vec::new(),
                });
                old_line += 1;
            }
            DiffOp::Insert => {
                lines.push(DiffLine {
                    line_type: "add".to_string(),
                    content: new[new_line - 1].clone(),
                    new_content: String::new(),
                    old_line: None,
                    new_line: Some(new_line),
                    old_segments: Vec::new(),
                    new_segments: Vec::new(),
                });
                new_line += 1;
            }
        }
    }

    // 后处理：将相邻的 delete + add 合并为 modified 行，做行内 diff
    let mut merged: Vec<DiffLine> = Vec::new();
    let mut idx = 0;
    while idx < lines.len() {
        let line = &lines[idx];
        if line.line_type == "delete"
            && idx + 1 < lines.len()
            && lines[idx + 1].line_type == "add"
        {
            let (old_segments, new_segments) = diff_chars(&line.content, &lines[idx + 1].content);
            merged.push(DiffLine {
                line_type: "modified".to_string(),
                content: line.content.clone(),
                new_content: lines[idx + 1].content.clone(),
                old_line: line.old_line,
                new_line: lines[idx + 1].new_line,
                old_segments,
                new_segments,
            });
            idx += 2;
        } else {
            merged.push(line.clone());
            idx += 1;
        }
    }

    merged
}

// 对两行文本做字符级 LCS，返回旧/新两侧的分段（changed 标记不同的部分）
pub(crate) fn diff_chars(old: &str, new: &str) -> (Vec<DiffSegment>, Vec<DiffSegment>) {
    // 超长行防护：字符级 LCS 是 O(m*n)，单行超长（压缩 js / 生成产物一行几万字符）会卡死/OOM。
    // 超阈值时放弃字符级标注，两侧各返回整行一个 changed=false 段 —— 行级底色（modified 灰底）
    // 仍表达"这行有改动"，但不会误标整行几十万字符全变。与前端 DiffEditor.charDiffSegs 的
    // MAX_CHAR_DIFF_LEN 保持一致（改动需两端同步）。
    const MAX_CHAR_DIFF_LEN: usize = 2000;
    if old.chars().count() > MAX_CHAR_DIFF_LEN || new.chars().count() > MAX_CHAR_DIFF_LEN {
        return (
            vec![DiffSegment { text: old.to_string(), changed: false }],
            vec![DiffSegment { text: new.to_string(), changed: false }],
        );
    }

    let old_chars: Vec<char> = old.chars().collect();
    let new_chars: Vec<char> = new.chars().collect();
    let m = old_chars.len();
    let n = new_chars.len();

    let mut dp = vec![vec![0usize; n + 1]; m + 1];
    for i in 1..=m {
        for j in 1..=n {
            if old_chars[i - 1] == new_chars[j - 1] {
                dp[i][j] = dp[i - 1][j - 1] + 1;
            } else {
                dp[i][j] = dp[i - 1][j].max(dp[i][j - 1]);
            }
        }
    }

    #[derive(PartialEq, Clone, Copy)]
    enum Op {
        Match(char),
        Insert(char),
        Delete(char),
    }

    let mut ops: Vec<Op> = Vec::new();
    let mut i = m;
    let mut j = n;
    while i > 0 || j > 0 {
        if i > 0 && j > 0 && old_chars[i - 1] == new_chars[j - 1] {
            ops.push(Op::Match(old_chars[i - 1]));
            i -= 1;
            j -= 1;
        } else if j > 0 && (i == 0 || dp[i][j - 1] >= dp[i - 1][j]) {
            ops.push(Op::Insert(new_chars[j - 1]));
            j -= 1;
        } else {
            ops.push(Op::Delete(old_chars[i - 1]));
            i -= 1;
        }
    }
    ops.reverse();

    // 旧侧分段：Match(不变) + Delete(变化)
    let mut old_segments: Vec<DiffSegment> = Vec::new();
    for op in &ops {
        let (text, changed) = match op {
            Op::Match(c) => (c.to_string(), false),
            Op::Delete(c) => (c.to_string(), true),
            Op::Insert(_) => continue,
        };
        push_segment(&mut old_segments, text, changed);
    }

    // 新侧分段：Match(不变) + Insert(变化)
    let mut new_segments: Vec<DiffSegment> = Vec::new();
    for op in &ops {
        let (text, changed) = match op {
            Op::Match(c) => (c.to_string(), false),
            Op::Insert(c) => (c.to_string(), true),
            Op::Delete(_) => continue,
        };
        push_segment(&mut new_segments, text, changed);
    }

    (old_segments, new_segments)
}

pub(crate) fn push_segment(segments: &mut Vec<DiffSegment>, text: String, changed: bool) {
    if let Some(last) = segments.last_mut() {
        if last.changed == changed {
            last.text.push_str(&text);
            return;
        }
    }
    segments.push(DiffSegment { text, changed });
}

/// 从 git tree 中读取指定路径文件内容（按行），并判定是否为二进制。
/// 返回 (行内容, 是否二进制)；文件不存在返回 (空, false)，二进制返回 (空, true)。
pub(crate) fn file_content_from_tree(
    repo: &Repository,
    tree: &git2::Tree,
    file_path: &str,
) -> (Vec<String>, bool, Option<String>) {
    match tree.get_path(Path::new(file_path)) {
        Ok(entry) => repo
            .find_blob(entry.id())
            .map(|blob| {
                let content = blob.content();
                let eol = detect_eol(content);
                // blob.is_binary() 用的是 git 自带启发式，与 is_binary_bytes 一致
                if blob.is_binary() {
                    return (Vec::new(), true, eol);
                }
                (
                    String::from_utf8_lossy(content)
                        .lines()
                        .map(|l| l.to_string())
                        .collect(),
                    false,
                    eol,
                )
            })
            .unwrap_or((Vec::new(), false, None)),
        Err(_) => (Vec::new(), false, None),
    }
}

#[command]
pub(crate) async fn get_file_diff(repo_path: String, file_path: String, commit_id: Option<String>) -> Result<FileDiff> {
    tokio::task::spawn_blocking(move || {
        let _guard = git_read_guard();
        let repo = open_repo(&repo_path)?;

        let (old_content, new_content, is_binary, eol_actual, eol_expected) = if let Some(cid) = commit_id {
            // 提交历史模式：显示该提交相对父提交的修改对比
            let oid = git2::Oid::from_str(&cid).map_err(|e| e.to_string())?;
            let commit = repo.find_commit(oid).map_err(|e| e.to_string())?;
            let commit_tree = commit.tree().map_err(|e| e.to_string())?;
            let (new_content, new_bin, new_eol) = file_content_from_tree(&repo, &commit_tree, &file_path);
            // 父提交中的文件内容（无父提交=新增文件，旧内容为空）
            let (old_content, old_bin, old_eol) = if commit.parent_count() == 0 {
                (Vec::new(), false, None)
            } else {
                let parent = commit.parent(0).map_err(|e| e.to_string())?;
                let parent_tree = parent.tree().map_err(|e| e.to_string())?;
                file_content_from_tree(&repo, &parent_tree, &file_path)
            };
            // 提交模式：实际 = 当前提交版本，期望 = 父提交版本
            (old_content, new_content, old_bin || new_bin, new_eol, old_eol)
        } else {
            // 工作区模式：工作区文件 vs HEAD
            let full_path = PathBuf::from(&repo_path).join(&file_path);
            let (new_content, new_bin, new_eol) = read_file_lines(&full_path);
            let (old_content, old_bin, _old_eol) = {
                let head = repo.head().map_err(|e| e.to_string())?;
                let tree = head.peel_to_tree().map_err(|e| e.to_string())?;
                file_content_from_tree(&repo, &tree, &file_path)
            };
            // 工作区模式：实际 = 工作区文件 EOL；期望 = git 按 autocrlf/.gitattributes 期望的形态。
            // 这正是「内容没变、但 git status 标 M」的根因（行尾规范化）。
            let eol_expected = git_expected_eol(&repo);
            (old_content, new_content, old_bin || new_bin, new_eol, eol_expected)
        };

        // 大文件 / 二进制 跳过 diff 运算：
        // - 二进制：old/new 为空，diff 无意义
        // - 超大文本：Myers 是 O((N+M)·D) 线性空间，几万行也能算；
        //   仅当超过 10 万行（极端生成产物）才降级 oversized，前端仍可查看/编辑原文。
        const MAX_DIFF_LINES: usize = 100_000;
        let (old_content, new_content, lines, is_oversized) = if is_binary {
            (Vec::new(), Vec::new(), Vec::new(), false)
        } else if old_content.len() > MAX_DIFF_LINES || new_content.len() > MAX_DIFF_LINES {
            (old_content, new_content, Vec::new(), true)
        } else {
            let lines = lcs_diff(&old_content, &new_content);
            (old_content, new_content, lines, false)
        };

        // 归还仓库实例，供后续命令复用（避免重复 Repository::open 扫 .git）
        return_repo(&repo_path, repo);

        Ok(FileDiff {
            old_content,
            new_content,
            lines,
            is_binary,
            is_oversized,
            eol_actual,
            eol_expected,
        })
    })
    .await
    .map_err(|e| format!("加载文件差异失败: {}", e))?
}

/// 读取 git 暂存区指定 stage 的文件内容。
/// stage 1 = base, 2 = ours/HEAD, 3 = theirs/incoming
pub(crate) fn read_stage_content(repo_path: &str, stage: u8, file_path: &str) -> Option<Vec<String>> {
    let output = git_command()
        .arg("-C")
        .arg(repo_path)
        .arg("show")
        .arg(format!(":{}:{}", stage, file_path))
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let content = String::from_utf8_lossy(&output.stdout);
    Some(content.lines().map(|l| l.to_string()).collect())
}

/// 将工作区文件内容按行拆分，保留每行末尾的换行符，便于精确替换冲突块。
pub(crate) fn split_lines_keep_newline(content: &str) -> Vec<String> {
    let lines: Vec<&str> = content.split('\n').collect();
    lines
        .iter()
        .enumerate()
        .map(|(i, line)| {
            if i < lines.len() - 1 {
                format!("{}\n", line)
            } else {
                line.to_string()
            }
        })
        .collect()
}

/// 从工作区内容中解析 Git 冲突块（含可选的 diff3 base 段）—— **容错版**。
///
/// ⚠️ 旧实现「遇到 `>>>>>>> ` 就无条件闭合」，在**畸形冲突文件**上会灾难性失败：
/// 例如 AI/人工半途编辑过、或多次 merge 叠加后，文件里会出现
///   `<<<<<<< HEAD`
///   `>>>>>>> master`      ← 提前出现的结束标记（ours 段为空）
///   `=======`             ← 真正的分隔符被甩到后面
///   ... 上百行真实差异内容 ...
///   `>>>>>>> master`      ← 真正的结束标记成了孤儿
/// 旧实现会在第 2 行就闭合，产出「1 个空块」，后续真实冲突内容一行都标不出来。
///
/// 新规则（与前端 `ConflictSolver.parseConflictBlocks` 保持一致）：
///  1. `>>>>>>> ` 只有在**已经见过 `=======`**（section==2）时才作为块结束；
///     否则记为「畸形标记行」stray（不计入 ours 内容）。
///  2. 扫描中若遇到下一个 `<<<<<<< ` 仍未闭合 → 回退，把当前块作为未闭合块收尾。
///  3. `=======` / `||||||| ` 只有「第一次出现」才当分隔符；重复出现记为 stray。
pub(crate) fn parse_conflict_blocks(content: &str) -> Vec<ConflictBlock> {
    let lines = split_lines_keep_newline(content);
    let mut result = Vec::new();
    let mut i = 0;

    while i < lines.len() {
        if !lines[i].starts_with("<<<<<<< ") {
            i += 1;
            continue;
        }

        let start_line = i;
        let marker_ours = lines[i][8..].trim_end().to_string();
        i += 1;

        let mut ours: Vec<String> = Vec::new();
        let mut base: Option<Vec<String>> = None;
        let mut theirs: Vec<String> = Vec::new();
        let mut stray: Vec<usize> = Vec::new();
        // 占位：真正遇到 `=======` 时更新；未闭合则停留在扫描终止处
        let mut separator_line = start_line + 1;
        // 结束标记（`>>>>>>> ` 后的分支名）：仅闭合块会填充，未闭合块保持空串
        let mut marker_theirs = String::new();
        let mut section = 0; // 0 = ours, 1 = base (diff3), 2 = theirs
        let mut closed = false;

        while i < lines.len() {
            let cur = lines[i].clone();

            // ① 下一个冲突块开始 → 当前块未闭合，回退到该行，交给外层重新解析
            if cur.starts_with("<<<<<<< ") {
                break;
            }

            if cur.starts_with("||||||| ") {
                if section == 0 {
                    base = Some(Vec::new());
                    section = 1;
                    i += 1;
                    continue;
                }
                // 畸形：分隔符之后又冒出 ||||||| → 记为畸形标记行
                stray.push(i);
                i += 1;
                continue;
            } else if cur.starts_with("=======") {
                if section != 2 {
                    separator_line = i;
                    section = 2;
                    i += 1;
                    continue;
                }
                // 畸形：theirs 段里又出现分隔符
                stray.push(i);
                i += 1;
                continue;
            } else if cur.starts_with(">>>>>>> ") {
                // ② 关键修复：只有「已经见过 =======」才认作结束标记
                if section == 2 {
                    marker_theirs = cur[8..].trim_end().to_string();
                    i += 1;
                    closed = true;
                    break;
                }
                // 提前出现的结束标记 → 畸形标记行（不计入 ours 内容）
                stray.push(i);
                i += 1;
                continue;
            }

            match section {
                0 => ours.push(cur),
                1 => {
                    if let Some(ref mut b) = base {
                        b.push(cur);
                    }
                }
                2 => theirs.push(cur),
                _ => {}
            }
            i += 1;
        }

        // ③ 统一出口：闭合块用 end_line=i-1（i 已指向结束标记之后）；
        //    未闭合块（撞到下一个 <<<<<<< 或文件尾）用扫描终止处，至少把范围标出来 ——
        //    绝不静默丢弃，丢弃 = 用户看到的「有差异但没标记」。
        let end_line = if closed {
            i - 1
        } else if i > start_line {
            i - 1
        } else {
            start_line
        };
        result.push(ConflictBlock {
            start_line,
            separator_line,
            end_line,
            ours,
            theirs,
            base,
            marker_ours,
            marker_theirs,
            stray_marker_lines: stray,
        });
    }

    result
}

#[command]
pub(crate) async fn get_conflict_file(repo_path: String, file_path: String) -> Result<ConflictFile> {
    tokio::task::spawn_blocking(move || {
        let _guard = git_read_guard();

        let full_path = PathBuf::from(&repo_path).join(&file_path);
        let working_raw = fs::read_to_string(&full_path).map_err(|e| e.to_string())?;
        let working_content = working_raw.lines().map(|l| l.to_string()).collect();

        let blocks = parse_conflict_blocks(&working_raw);

        // ours = stage 2 (HEAD/current branch), theirs = stage 3 (incoming branch)
        let mut ours_content = read_stage_content(&repo_path, 2, &file_path).unwrap_or_default();
        let mut theirs_content = read_stage_content(&repo_path, 3, &file_path).unwrap_or_default();
        let base_content = read_stage_content(&repo_path, 1, &file_path);

        // stash-pop 产生的冲突不填充 index 阶段（stage 1/2/3 为空），此时从冲突标记解析
        // ours/theirs 兜底，保证冲突解决器左右两栏仍显示「本地改动」与「远端改动」。
        if ours_content.is_empty() && theirs_content.is_empty() && !blocks.is_empty() {
            ours_content = blocks.iter().flat_map(|b| b.ours.iter().cloned()).collect();
            theirs_content = blocks.iter().flat_map(|b| b.theirs.iter().cloned()).collect();
        }

        Ok(ConflictFile {
            path: file_path,
            ours_content,
            theirs_content,
            base_content,
            working_content,
            blocks,
        })
    })
    .await
    .map_err(|e| format!("加载冲突文件失败: {}", e))?
}

#[command]
pub(crate) async fn read_working_file(repo_path: String, file_path: String) -> Result<String> {
    tokio::task::spawn_blocking(move || {
        let _guard = git_read_guard();
        let full_path = PathBuf::from(&repo_path).join(&file_path);
        fs::read_to_string(&full_path).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("读取文件失败: {}", e))?
}

#[command]
pub(crate) async fn write_file_content(repo_path: String, file_path: String, content: String) -> Result<()> {
    tokio::task::spawn_blocking(move || {
        let _guard = git_write_guard();
        let full_path = PathBuf::from(&repo_path).join(&file_path);
        if let Some(parent) = full_path.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        // 按【原文件的行尾形态】写回。前端两个编辑入口（编辑差异 / 解决冲突）传回的
        // 都是 LF 分隔的文本，直接落盘会把 CRLF 文件（Windows autocrlf 仓库的常态）
        // 整体改成 LF —— 用户只动一行，git 里却是全文件每行都有差异。
        // 文件末尾「有无换行」同理：读入时 `str::lines()` 丢掉了这个信息，只有这里能补回，
        // 否则保存后必然多出 "\ No newline at end of file"。
        let original = fs::read(&full_path).ok();
        let ends_with_newline = original
            .as_deref()
            .map(|b| b.last() == Some(&b'\n'))
            .unwrap_or(false);
        let use_crlf = original
            .as_deref()
            .map(|b| detect_eol(b).as_deref() == Some("CRLF"))
            .unwrap_or(false);

        // 只归一 CRLF→LF（孤立 \r 属正文，不动），再按原形态重建；冲突页原文自带 \r\n 也走同一条路
        let mut out = content.replace("\r\n", "\n");
        if ends_with_newline && !out.ends_with('\n') {
            out.push('\n');
        }
        if use_crlf {
            out = out.replace('\n', "\r\n");
        }

        let mut file = File::create(&full_path).map_err(|e| e.to_string())?;
        file.write_all(out.as_bytes()).map_err(|e| e.to_string())?;
        Ok(())
    })
    .await
    .map_err(|e| format!("写入文件失败: {}", e))?
}
