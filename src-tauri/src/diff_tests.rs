use super::*;

    /// 旧版 O(m*n) LCS 差分（保留副本作为基准，仅测试对照用）：
    /// 返回 (line_type, content) 序列，语义与生产 lcs_diff 完全一致。
    fn legacy_lcs_pairs(old: &[String], new: &[String]) -> Vec<(String, String)> {
        let m = old.len();
        let n = new.len();
        let mut dp = vec![vec![0usize; n + 1]; m + 1];
        for i in 1..=m {
            for j in 1..=n {
                if old[i - 1] == new[j - 1] {
                    dp[i][j] = dp[i - 1][j - 1] + 1;
                } else {
                    dp[i][j] = dp[i - 1][j].max(dp[i][j - 1]);
                }
            }
        }
        let mut pairs: Vec<(String, String)> = Vec::new();
        let mut i = m;
        let mut j = n;
        while i > 0 || j > 0 {
            if i > 0 && j > 0 && old[i - 1] == new[j - 1] {
                pairs.push(("context".to_string(), old[i - 1].clone()));
                i -= 1;
                j -= 1;
            } else if j > 0 && (i == 0 || dp[i][j - 1] >= dp[i - 1][j]) {
                pairs.push(("add".to_string(), new[j - 1].clone()));
                j -= 1;
            } else if i > 0 {
                pairs.push(("delete".to_string(), old[i - 1].clone()));
                i -= 1;
            }
        }
        pairs.reverse();
        // 与生产一致：相邻 delete+add 合并为 modified
        let mut merged: Vec<(String, String)> = Vec::new();
        let mut k = 0;
        while k < pairs.len() {
            if k + 1 < pairs.len() && pairs[k].0 == "delete" && pairs[k + 1].0 == "add" {
                merged.push(("modified".to_string(), pairs[k].1.clone()));
                k += 2;
            } else {
                merged.push(pairs[k].clone());
                k += 1;
            }
        }
        merged
    }

    fn to_lines(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    /// 对照测试核心：Myers 产出与旧 LCS 基准一致（合并 modified 后逐行比较 type/content）
    fn assert_myers_matches(old: &[&str], new: &[&str]) {
        let old_l = to_lines(old);
        let new_l = to_lines(new);
        let legacy = legacy_lcs_pairs(&old_l, &new_l);
        let actual: Vec<(String, String)> = lcs_diff(&old_l, &new_l)
            .into_iter()
            .map(|l| (l.line_type, l.content))
            .collect();
        assert_eq!(
            actual, legacy,
            "Myers diff mismatch\nold={old:?}\nnew={new:?}"
        );
    }

    #[test]
    fn myers_empty_both() {
        let r = lcs_diff(&[], &[]);
        assert!(r.is_empty());
    }

    #[test]
    fn myers_identical() {
        assert_myers_matches(
            &["a", "b", "c"],
            &["a", "b", "c"],
        );
    }

    #[test]
    fn myers_all_new() {
        assert_myers_matches(&[], &["x", "y"]);
    }

    #[test]
    fn myers_all_deleted() {
        assert_myers_matches(&["x", "y"], &[]);
    }

    #[test]
    fn myers_insert_middle() {
        assert_myers_matches(&["a", "b"], &["a", "ins", "b"]);
    }

    #[test]
    fn myers_delete_middle() {
        assert_myers_matches(&["a", "gone", "b"], &["a", "b"]);
    }

    #[test]
    fn myers_modify_line() {
        assert_myers_matches(&["a", "old line", "c"], &["a", "new line", "c"]);
    }

    #[test]
    fn myers_complex_edits() {
        assert_myers_matches(
            &[
                "keep1", "keep2", "del_a", "del_b", "keep3", "change_me", "keep4", "tail1",
            ],
            &[
                "keep1", "keep2", "keep3", "changed!", "keep4", "extra_new", "tail1", "tail2",
            ],
        );
    }

    #[test]
    fn myers_reversed_words() {
        // 完全重排（D 接近 N+M，验证最坏情况不会挂、结果仍是合法 diff）
        let old = to_lines(&["one", "two", "three", "four", "five"]);
        let new = to_lines(&["five", "four", "three", "two", "one"]);
        let r = lcs_diff(&old, &new);
        // 合法 diff 的可达性验证：从产出序列能重建 new
        let mut rebuilt: Vec<String> = Vec::new();
        for l in &r {
            match l.line_type.as_str() {
                "context" | "modified" | "add" => rebuilt.push(l.content.clone()),
                "delete" => {}
                _ => {}
            }
        }
        assert_eq!(rebuilt, new, "rebuilt new content mismatch");
        // 且 old 侧行号覆盖完整
        let mut covered = vec![false; old.len()];
        for l in &r {
            if let Some(n) = l.old_line {
                covered[n - 1] = true;
            }
        }
        assert!(
            covered.iter().all(|&c| c),
            "some old lines not covered: {covered:?}"
        );
    }

    #[test]
    fn myers_line_numbers_monotonic() {
        // 行号语义验证：context 的 old_line/new_line 递增、delete 行号递增、add 行号递增
        let old = to_lines(&["a", "b", "c", "d", "e"]);
        let new = to_lines(&["a", "x", "c", "e", "f"]);
        let r = lcs_diff(&old, &new);
        let mut last_old = 0usize;
        let mut last_new = 0usize;
        for l in &r {
            if let Some(o) = l.old_line {
                assert!(o > last_old, "old_line not increasing: {o} after {last_old}");
                last_old = o;
            }
            if let Some(n) = l.new_line {
                assert!(n > last_new, "new_line not increasing: {n} after {last_new}");
                last_new = n;
            }
        }
    }

    #[test]
    fn myers_large_file_smoke() {
        // 超过旧 5000 行上限的大文件冒烟：Myers 能秒算、不 OOM
        let n = 20_000usize;
        let old: Vec<String> = (0..n).map(|i| format!("line {i:06}").to_string()).collect();
        let mut new = old.clone();
        // 改动若干行（D 小，Myers 应极快）
        for i in [3usize, 1000, 9999, 19999] {
            new[i] = format!("line {i:06} CHANGED").to_string();
        }
        // 额外加 5 行
        for i in 0..5usize {
            new.push(format!("extra tail {i}").to_string());
        }
        let t = std::time::Instant::now();
        let r = lcs_diff(&old, &new);
        assert!(r.len() > n, "expected diff covering all lines, got {}", r.len());
        // 行号覆盖验证
        let mut covered_new = vec![false; new.len()];
        for l in &r {
            if let Some(no) = l.new_line {
                covered_new[no - 1] = true;
            }
        }
        assert!(covered_new.iter().all(|&c| c), "new line coverage broken");
        eprintln!("myers_large_file_smoke took {:?} for {n} lines", t.elapsed());
    }

    #[test]
    fn diff_chars_short_line_marks_changes() {
        // 正常长度行：字符级 diff 应标出 changed 段
        let (old_segs, new_segs) = diff_chars("abc def ghi", "abc XYZ ghi");
        assert!(!old_segs.is_empty() && !new_segs.is_empty());
        assert!(
            old_segs.iter().any(|s| s.changed) && new_segs.iter().any(|s| s.changed),
            "short-line diff should flag changed chars"
        );
        // 两行内容可完整重建
        let old_joined: String = old_segs.iter().map(|s| s.text.as_str()).collect();
        let new_joined: String = new_segs.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(old_joined, "abc def ghi");
        assert_eq!(new_joined, "abc XYZ ghi");
    }

    #[test]
    fn diff_chars_long_line_degrades_gracefully() {
        // 超长行（>2000 字符，如压缩产物单行）：不 panic、不 OOM，整行降级为单段 changed=false
        let long_a = "x".repeat(2500);
        let long_b = "y".repeat(2500);
        let (old_segs, new_segs) = diff_chars(&long_a, &long_b);
        assert_eq!(old_segs.len(), 1, "oversized old should collapse to 1 segment");
        assert_eq!(new_segs.len(), 1, "oversized new should collapse to 1 segment");
        assert!(!old_segs[0].changed && !new_segs[0].changed, "oversized line should NOT be flagged as fully changed");
        assert_eq!(old_segs[0].text.len(), 2500);
        assert_eq!(new_segs[0].text.len(), 2500);

        // 一侧超长也应触发退化
        let (o2, n2) = diff_chars(&"short".to_string(), &long_b);
        assert_eq!(n2.len(), 1);
        assert_eq!(o2[0].text, "short");
    }
