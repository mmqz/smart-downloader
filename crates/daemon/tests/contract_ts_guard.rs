//! 契约类型守卫的守卫（#161 P2）：Req/Out/Dto 必须 ts-rs 化。
//!
//! 背景：`max_download_kb_s` → `down_kb_s` 字段名事故（serde 静默丢弃、
//! 空操作返回 200、限速从未生效）的根因是请求结构体没有 TS 对应物——
//! 任何一方改名，另一边编译期毫无信号。#148 起逐个补 derive，但靠人肉
//! 记忆防漏不可持续：#161 审查发现 `AddRssFeedReq` 是最后一个漏网者，
//! 本测试把「漏网」本身变成编译产物的测试失败。
//!
//! 规则（静态源码检查，与特性无关，任何特性组合下结果一致）：
//! 1. 非特性门控的 `*Req` / `*Out` / `*Dto` 结构体（daemon 前后端契约
//!    命名约定）必须有 `#[derive(..., TS ...)]` 与 `#[ts(export ...)]`；
//! 2. 特性门控的结构体：CI 矩阵覆盖的特性（见 CI_COVERED_FEATURES）
//!    同样必须有（nas 特性在 rust job 的 ftp,nas 组合里跑导出测试）；
//!    CI 未覆盖的特性登记在 GATED_UNCOVERED 豁免（导出测试永不运行，
//!    生成文件将无人验证地腐化——显式不生成优于假守卫）；
//! 3. 有 `#[ts(export)]` 的结构体，对应 `ui/src/lib/daemon/generated/`
//!    生成文件必须已提交（防「本地生成了忘提交」——默认特性 job 的
//!    git diff 漂移检查只覆盖默认特性，nas 门控导出须靠本规则兜底）；
//! 4. 登记簿精确匹配：源码里的未覆盖门控结构体集合必须与
//!    GATED_UNCOVERED 完全一致（删 cfg / CI 补特性 → 测试逼你同步）。
//!
//! 非目标（记录边界，防误解为疏漏）：
//! - `*Query` 类（URL 查询参数）：UI 手工拼 query 串，ts-rs 类型无消费方，
//!   待 UI 侧类型化拼参后再纳入；
//! - provider crate 的 `PwdReq` 等迅雷登录页内部结构体：反序列化第三方
//!   HTML 表单，非 daemon↔UI 契约。
//!
//! 有效性自证（回滚必红）：临时去掉任一 derive（如 MagnetMetaReq）本测试
//! 必须失败，恢复后转绿——提交前已双向验证。
//!
//! 维护提醒：变更 ci.yml 特性矩阵时须同步 CI_COVERED_FEATURES。

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

/// ci.yml 跑过 `cargo test -p smart-dl-daemon --features …` 的特性
/// （rust job：ftp / nas / ftp,nas / sftp；bt-integration job：bt）。
const CI_COVERED_FEATURES: &[&str] = &["ftp", "nas", "sftp", "bt"];

/// CI 矩阵未覆盖、豁免 ts-rs 的门控结构体登记簿：(结构体名, 特性)。
/// 解除豁免三步：ci.yml 补该特性 → 删除本条 → 加 `#[derive(TS)]`。
const GATED_UNCOVERED: &[(&str, &str)] = &[("AddXunleiImportReq", "xunlei-import")];

#[derive(Debug, Default)]
struct StructInfo {
    file: String,
    line: usize,
    name: String,
    feature: Option<String>,
    ts_derive: bool,
    ts_export: bool,
}

/// 递归收集 .rs 文件。
fn collect_rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            collect_rs_files(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

/// 解析 `struct` 定义行，返回 (名字, 是否契约命名)。
fn parse_struct_name(line: &str) -> Option<String> {
    let t = line.trim_start();
    let rest = t
        .strip_prefix("pub struct ")
        .or_else(|| t.strip_prefix("struct "))?;
    let name: String = rest
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

/// 从结构体定义行向上收集属性块（#[…]、///、//、空行；上限 15 行），
/// 途中撞到其他项（fn/impl/enum/}/…) 即停——防止吞入上一项的属性。
fn attribute_window(lines: &[&str], idx: usize) -> String {
    let mut buf = String::new();
    let mut count = 0;
    let mut i = idx;
    while i > 0 && count < 15 {
        let t = lines[i - 1].trim();
        if t.is_empty() || t.starts_with("#[") || t.starts_with("///") || t.starts_with("//") {
            writeln!(buf, "{t}").ok();
            count += 1;
            i -= 1;
        } else {
            break;
        }
    }
    buf
}

fn extract_feature(attrs: &str) -> Option<String> {
    // 形如 #[cfg(feature = "nas")] / #[cfg(all(feature = "a", …))]
    let mut feats = Vec::new();
    let mut rest = attrs;
    while let Some(pos) = rest.find("feature = ") {
        let after = &rest[pos + 10..];
        let quote = after.find('"')?;
        let end = after[quote + 1..].find('"')? + quote + 1;
        feats.push(after[quote + 1..end].to_string());
        rest = &after[end..];
    }
    feats.into_iter().next()
}

fn scan_daemon_src() -> Vec<StructInfo> {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let src = manifest.join("src");
    let mut files = Vec::new();
    collect_rs_files(&src, &mut files);
    files.sort();
    let mut out = Vec::new();
    for f in files {
        let Ok(text) = fs::read_to_string(&f) else {
            panic!("读取失败: {}", f.display());
        };
        let lines: Vec<&str> = text.lines().collect();
        for (i, line) in lines.iter().enumerate() {
            let Some(name) = parse_struct_name(line) else {
                continue;
            };
            if !(name.ends_with("Req") || name.ends_with("Out") || name.ends_with("Dto")) {
                continue;
            }
            let attrs = attribute_window(&lines, i);
            out.push(StructInfo {
                file: f
                    .strip_prefix(manifest.parent().unwrap().parent().unwrap())
                    .unwrap_or(&f)
                    .display()
                    .to_string(),
                line: i + 1,
                name,
                feature: extract_feature(&attrs),
                ts_derive: attrs.contains("TS") && attrs.contains("#[derive("),
                ts_export: attrs.contains("#[ts(") && attrs.contains("export"),
            });
        }
    }
    out
}

#[test]
fn req_out_dto_structs_must_have_ts_derive() {
    let structs = scan_daemon_src();
    assert!(
        !structs.is_empty(),
        "扫描器空转：未发现任何契约结构体，src 路径或解析逻辑已失效"
    );

    let mut violations = Vec::new();
    let mut uncovered_found: BTreeSet<String> = BTreeSet::new();
    let registry: BTreeSet<(String, String)> = GATED_UNCOVERED
        .iter()
        .map(|(n, f)| (n.to_string(), f.to_string()))
        .collect();

    // 规则 3 的生成文件存在性检查（仓库根 = crates/daemon/../..）
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    let gen_dir = repo_root.join("ui/src/lib/daemon/generated");

    for s in &structs {
        let must_ts = match &s.feature {
            None => true,
            Some(f) => {
                if CI_COVERED_FEATURES.contains(&f.as_str()) {
                    true // CI 覆盖的门控结构体：导出测试会跑，必须有
                } else {
                    uncovered_found.insert(format!("{}/{}", s.name, f));
                    false // 未覆盖：登记簿校验在后面统一做
                }
            }
        };
        if must_ts && (!s.ts_derive || !s.ts_export) {
            violations.push(format!(
                "{}:{} `{}`（feature={:?}）缺 TS 契约：derive(TS)={} ts(export)={} —— 加 #[derive(serde::Deserialize, TS)] 与 #[ts(export)]，并提交生成文件",
                s.file, s.line, s.name, s.feature, s.ts_derive, s.ts_export
            ));
        }
        // 规则 3：声明了导出 → 生成文件必须已提交
        if s.ts_export {
            let gen = gen_dir.join(format!("{}.ts", s.name));
            if !gen.exists() {
                violations.push(format!(
                    "{}:{} `{}` 声明 #[ts(export)] 但 {} 不存在——本地跑 `npm run gen:types`（门控结构体加对应 --features）后提交",
                    s.file, s.line, s.name,
                    gen.strip_prefix(&repo_root).unwrap_or(&gen).display()
                ));
            }
        }
    }

    // 规则 4：登记簿与源码精确一致（防陈旧条目 / 防新门控静默漏登）
    let found_pairs: BTreeSet<(String, String)> = uncovered_found
        .iter()
        .filter_map(|s| {
            let (name, feat) = s.split_once('/')?;
            Some((name.to_string(), feat.to_string()))
        })
        .collect();
    for (name, feat) in found_pairs.difference(&registry) {
        violations.push(format!(
            "新出现 CI 未覆盖的门控契约结构体 `{}`（feature={feat}）：加入 GATED_UNCOVERED 登记簿并注明原因，或给 CI 矩阵补该特性后加 TS derive",
            name
        ));
    }
    for (name, feat) in registry.difference(&found_pairs) {
        violations.push(format!(
            "登记簿条目 `{}`（feature={feat}）已失效（源码中不存在或特性已被 CI 覆盖）——删除该条目",
            name
        ));
    }

    assert!(
        violations.is_empty(),
        "契约类型守卫发现 {} 处违规：\n  - {}",
        violations.len(),
        violations.join("\n  - ")
    );
}
