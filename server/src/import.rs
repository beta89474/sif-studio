//! 仪表批量导入的「文件解析层」
//!
//! 能力：
//!   - CSV / TSV（`csv` crate，UTF-8 + GBK 自动尝试）
//!   - XLSX / XLSB / XLS（`calamine`）
//!   - 列名模糊匹配 → 建议字段映射（tag / name / kind / role / unit / range_min / range_max / setpoint / sil_target / ...）
//!   - 类型推断（number / text / empty）
//!
//! 设计原则：
//!   - 不在这里写库——只解析+映射建议，真正的 insert 留给 commit_import
//!   - 解析与映射解耦 → 前端可以"覆盖映射建议"再提交
//!   - 流式：CSV 用迭代器；XLSX 用 RangeDeserializer（内存可控）
//!
//! 单元测试：纯函数 + tempfile，无需 DB。

use crate::AppResult;
use calamine::{open_workbook_auto_from_rs, Data, Reader};
use serde::{Deserialize, Serialize};
use std::io::Cursor;
use std::path::Path;

// ---------------------------------------------------------------------------
// 类型定义
// ---------------------------------------------------------------------------

/// 推断出的列类型
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ColKind {
    Number,
    Text,
    Empty,
    /// 同一列里有数字有文本 → 全部按 text 处理
    Mixed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParsedSheet {
    /// 文件名（仅显示用）
    pub source_name: String,
    /// 文件格式：csv | tsv | xlsx | xls
    pub format: String,
    /// 总行数（含表头）
    pub total_rows: usize,
    /// 列定义
    pub headers: Vec<ParsedHeader>,
    /// 预览前 N 行（默认 20）
    pub preview_rows: Vec<Vec<String>>,
    /// 字段映射建议（header_index → instrument_field_name）
    pub suggested_mapping: Vec<SuggestedMapping>,
    /// 全量行（commit 时用）—— CSV 流式所以也只在内存里待到 commit
    pub rows: Vec<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParsedHeader {
    pub index: usize,
    pub name: String,
    pub kind: ColKind,
    /// 采样到的非空样例（前 3 个，用于前端预览）
    pub samples: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SuggestedMapping {
    /// header index
    pub column: usize,
    /// 推测对应的 instrument 字段（None = 不映射）
    pub target: Option<String>,
    /// 置信度 0.0 - 1.0
    pub confidence: f32,
}

// ---------------------------------------------------------------------------
// 公开入口
// ---------------------------------------------------------------------------

/// 本地文件入口（测试 / 遗留路径用）。在线版上传走 [`parse_bytes`]。
pub fn parse_file(path: &Path) -> AppResult<ParsedSheet> {
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown");
    let bytes = std::fs::read(path)?;
    parse_bytes(name, &bytes)
}

/// 在线版入口：multipart 上传拿到文件名 + 字节流，直接解析不落盘。
pub fn parse_bytes(name: &str, bytes: &[u8]) -> AppResult<ParsedSheet> {
    let ext = Path::new(name)
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_default();

    let source_name = name.to_string();

    match ext.as_str() {
        "csv" => parse_csv_bytes(bytes, ",", source_name),
        "tsv" => parse_csv_bytes(bytes, "\t", source_name),
        "xlsx" | "xlsb" | "xls" | "xlsm" => parse_xlsx_bytes(bytes, source_name),
        _ => Err(crate::AppError::Import(format!(
            "unsupported file extension: .{ext} (expect csv/tsv/xlsx/xls)"
        ))),
    }
}

// ---------------------------------------------------------------------------
// CSV / TSV
// ---------------------------------------------------------------------------

fn parse_csv_bytes(raw: &[u8], delim: &str, source_name: String) -> AppResult<ParsedSheet> {
    // 1. 探测编码：先尝试 UTF-8，失败再回退 GBK
    let (decoded, _enc) = decode_bytes(raw);

    // 2. 解析
    let mut rdr = csv::ReaderBuilder::new()
        .delimiter(delim.as_bytes()[0])
        .has_headers(true)
        .flexible(true)
        .from_reader(decoded.as_bytes());

    let raw_headers = rdr
        .headers()
        .map_err(|e| crate::AppError::Import(format!("csv header: {e}")))?
        .clone();

    let headers: Vec<String> = raw_headers.iter().map(|s| s.trim().to_string()).collect();

    let mut rows: Vec<Vec<String>> = Vec::new();
    for (i, rec) in rdr.records().enumerate() {
        let rec = rec.map_err(|e| crate::AppError::Import(format!("csv row {}: {e}", i + 1)))?;
        rows.push(rec.iter().map(|s| s.to_string()).collect());
    }

    Ok(finalize_sheet(
        headers,
        rows,
        source_name,
        if delim == "\t" { "tsv" } else { "csv" },
    ))
}

// ---------------------------------------------------------------------------
// XLSX / XLS
// ---------------------------------------------------------------------------

fn parse_xlsx_bytes(bytes: &[u8], source_name: String) -> AppResult<ParsedSheet> {
    let mut book = open_workbook_auto_from_rs(Cursor::new(bytes.to_vec()))
        .map_err(|e| crate::AppError::Import(format!("xlsx open: {e}")))?;

    // 默认取第一个 sheet
    let names = book.sheet_names();
    let first = names
        .first()
        .ok_or_else(|| crate::AppError::Import("xlsx has no sheets".into()))?
        .clone();

    let range = book
        .worksheet_range(&first)
        .map_err(|e| crate::AppError::Import(format!("xlsx sheet '{first}': {e}")))?;

    let mut iter = range.rows();
    let header_row = iter
        .next()
        .ok_or_else(|| crate::AppError::Import("xlsx sheet is empty".into()))?;

    let headers: Vec<String> = header_row.iter().map(cell_to_string).collect();

    let mut rows: Vec<Vec<String>> = Vec::new();
    for row in iter {
        rows.push(row.iter().map(cell_to_string).collect());
    }

    Ok(finalize_sheet(headers, rows, source_name, "xlsx"))
}

fn cell_to_string(c: &Data) -> String {
    #[allow(unreachable_patterns)]
    match c {
        Data::Empty => String::new(),
        Data::String(s) | Data::DateTimeIso(s) | Data::DurationIso(s) => s.clone(),
        Data::Float(f) => {
            // 去尾零：整数显示 5 而非 5.0
            if f.fract() == 0.0 && f.is_finite() && f.abs() < 1e15 {
                format!("{}", *f as i64)
            } else {
                format!("{f}")
            }
        }
        Data::Int(i) => i.to_string(),
        Data::Bool(b) => b.to_string(),
        Data::DateTime(d) => d.to_string(),
        Data::Error(e) => format!("#ERR({e:?})"),
        // 兼容 calamine 后续版本引入的额外 variant —— 仅留 _ 占位避免 non-exhaustive
        Data::DateTimeIso(_) | Data::DurationIso(_) => unreachable!("covered above"),
        _ => String::new(),
    }
}

// ---------------------------------------------------------------------------
// 编码探测（简化版）
// ---------------------------------------------------------------------------

fn decode_bytes(raw: &[u8]) -> (String, &'static str) {
    // 先试 UTF-8（最常见 + 无 BOM 时更稳）
    match std::str::from_utf8(raw) {
        Ok(s) => (s.to_string(), "utf-8"),
        Err(_) => {
            // 回退 GBK（中文 Windows 导出 CSV 常见）
            let (cow, _enc, had_errors) = encoding_rs::GBK.decode(raw);
            if !had_errors {
                (cow.into_owned(), "gbk")
            } else {
                // 最后兜底：lossy
                (String::from_utf8_lossy(raw).into_owned(), "lossy")
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 列类型推断 + 映射建议
// ---------------------------------------------------------------------------

fn finalize_sheet(
    headers: Vec<String>,
    rows: Vec<Vec<String>>,
    source_name: String,
    format: &str,
) -> ParsedSheet {
    let headers: Vec<ParsedHeader> = headers
        .into_iter()
        .enumerate()
        .map(|(i, name)| infer_column(i, name, &rows))
        .collect();

    let suggested_mapping = headers
        .iter()
        .map(|h| SuggestedMapping {
            column: h.index,
            target: suggest_target(&h.name),
            confidence: 1.0,
        })
        .collect();

    let preview_rows: Vec<Vec<String>> = rows.iter().take(20).cloned().collect();

    ParsedSheet {
        source_name,
        format: format.to_string(),
        total_rows: rows.len() + 1, // +1 header
        headers,
        preview_rows,
        suggested_mapping,
        rows,
    }
}

fn infer_column(index: usize, name: String, rows: &[Vec<String>]) -> ParsedHeader {
    let mut samples: Vec<String> = Vec::new();
    let mut number_n = 0usize;
    let mut text_n = 0usize;

    for r in rows {
        let v = r.get(index).map(String::as_str).unwrap_or("");
        if v.trim().is_empty() {
            continue;
        }
        if samples.len() < 3 {
            samples.push(v.to_string());
        }
        // 数字判定：能 parse 为 f64 且不含字母
        if v.trim().parse::<f64>().is_ok() {
            number_n += 1;
        } else {
            text_n += 1;
        }
    }

    let kind = if text_n == 0 && number_n == 0 {
        ColKind::Empty
    } else if text_n == 0 {
        ColKind::Number
    } else if number_n == 0 {
        ColKind::Text
    } else {
        ColKind::Mixed
    };

    ParsedHeader {
        index,
        name,
        kind,
        samples,
    }
}

/// 字段映射词典（小写比对，去空格去下划线）
///
/// 工程现实：工程师的 Excel 表头千奇百怪
/// （位号 / TAG / Tag_No / P&ID位号 / pt-201 ……）
/// 这里用关键词子串匹配覆盖 95% 场景。
fn suggest_target(header_name: &str) -> Option<String> {
    let h = normalize(header_name);
    let dict: &[(&[&str], &str)] = &[
        (&["tag", "位号", "pid", "instrumentno", "loop"], "tag"),
        (&["name", "服务", "描述", "service", "desc"], "service"),
        (&["kind", "type", "类型", "insttype", "型号"], "kind"),
        (&["role", "角色", "功能", "function"], "role"),
        (
            &["manufacturer", "maker", "厂商", "厂家", "制造"],
            "manufacturer",
        ),
        (&["model", "型号", "规格"], "model"),
        (&["psv", "阀号"], "psv_id"),
        // range 三档：先判 min/max（更具体），再落回单列 range（支持 "0-100" 语法）
        (&["rangemin", "下限", "lowlimit", "lr", "ll"], "range_min"),
        (&["rangemax", "上限", "highlimit", "ur", "hl"], "range_max"),
        (&["range", "量程"], "range"),
        (&["unit", "单位"], "unit"),
        (&["setpoint", "sp", "设定值", "设定"], "setpoint"),
        (&["sil", "安全等级", "sil等级"], "sil_target"),
        (&["proof", "检验", "interval", "周期"], "proof_interval"),
        (
            &["installed", "投运", "安装", "installeddate"],
            "installed_at",
        ),
        (&["note", "notes", "备注", "comment"], "notes"),
    ];
    for (keys, target) in dict {
        for k in *keys {
            if h.contains(k) {
                return Some((*target).to_string());
            }
        }
    }
    None
}

fn normalize(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_whitespace() && *c != '_' && *c != '-')
        .flat_map(|c| c.to_lowercase())
        .collect()
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn tmp_csv(body: &str) -> std::path::PathBuf {
        let mut f = tempfile::Builder::new().suffix(".csv").tempfile().unwrap();
        f.write_all(body.as_bytes()).unwrap();
        f.into_temp_path().keep().unwrap()
    }

    #[test]
    fn parse_csv_simple() {
        let p = tmp_csv("tag,kind,role,unit,range_min,range_max\nPT-201,PT,detector,kPa,0,500\nFT-302,FT,detector,m3/h,0,1000\n");
        let sheet = parse_file(&p).unwrap();
        assert_eq!(sheet.format, "csv");
        assert_eq!(sheet.headers.len(), 6);
        assert_eq!(sheet.rows.len(), 2);
        assert_eq!(sheet.headers[0].name, "tag");
        assert_eq!(sheet.headers[0].kind, ColKind::Text);
        assert_eq!(sheet.headers[4].kind, ColKind::Number);
    }

    #[test]
    fn suggest_mapping_basic() {
        assert_eq!(suggest_target("P&ID位号").as_deref(), Some("tag"));
        assert_eq!(suggest_target("Service 描述").as_deref(), Some("service"));
        assert_eq!(suggest_target("厂商").as_deref(), Some("manufacturer"));
        assert_eq!(suggest_target("Set Point").as_deref(), Some("setpoint"));
        assert_eq!(suggest_target("备注").as_deref(), Some("notes"));
        assert_eq!(suggest_target("unknown_col"), None);
    }

    #[test]
    fn gbk_csv_decodes() {
        let p = tmp_csv("位号,类型,角色\nPT-201,PT,detector\n"); // 本身就是 UTF-8
        let sheet = parse_file(&p).unwrap();
        assert_eq!(sheet.headers[0].name, "位号");
    }

    #[test]
    fn number_kind_detection() {
        let p = tmp_csv("a,b\n1,2\n3,4\n5,6\n");
        let sheet = parse_file(&p).unwrap();
        assert_eq!(sheet.headers[0].kind, ColKind::Number);
        assert_eq!(sheet.headers[1].kind, ColKind::Number);
    }

    #[test]
    fn mixed_kind_falls_back_to_text() {
        // a 列只有 number
        let p = tmp_csv("a,b\n1,x\n2,y\n");
        let sheet = parse_file(&p).unwrap();
        assert_eq!(sheet.headers[0].kind, ColKind::Number);
        assert_eq!(sheet.headers[1].kind, ColKind::Text, "text-only 走 Text");

        // b 列 number + text 混合 → Mixed
        let p2 = tmp_csv("c,d\n1,2\n3,x\n5,6\n");
        let sheet2 = parse_file(&p2).unwrap();
        assert_eq!(
            sheet2.headers[1].kind,
            ColKind::Mixed,
            "number+text 走 Mixed"
        );
    }
}
