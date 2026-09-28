//! OpenCode 可用模型读取（模型下拉数据源）：只读访问 OpenCode 桌面端本地服务 HTTP API。
//!
//! 数据源：OpenCode 桌面端后台服务的 `GET /api/model`（返回当前可用模型快照）。
//! - **认证**：Basic `opencode:<password>`，密码读取 `~/.config/opencode/service.json`；
//! - **服务地址**：优先从 OpenCode 桌面端最新日志解析 `url: 'http://127.0.0.1:<port>'`；
//!   解析不到时探测约定端口 49374（用真实请求验证，不做无验证的猜测）。
//!
//! 失败一律明确报错（不猜、不静默），由界面退回手动输入 `provider/model`；只读访问，不修改 OpenCode 数据。

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;

use crate::bridge::dto::OpencodeModelDto;
use crate::bridge::error::CommandError;

/// 无法读取 OpenCode 模型列表（服务未启动 / 端口不可达 / 响应异常）。
pub const OPENCODE_MODELS_UNAVAILABLE: &str = "opencode_models_unavailable";
/// 未找到 OpenCode 服务信息（`service.json` 缺失或密码无效）。
pub const OPENCODE_SERVICE_INFO_MISSING: &str = "opencode_service_info_missing";

/// OpenCode 桌面端日志目录（相对用户目录；`AppData/Roaming` 为 Windows 约定）。
const OPENCODE_LOGS_RELATIVE: &str = "AppData/Roaming/ai.opencode.desktop/logs";
/// OpenCode 服务信息文件（相对用户目录）。
const OPENCODE_SERVICE_RELATIVE: &str = ".config/opencode/service.json";
/// 约定服务端口：桌面端各版本观察到的稳定监听端口（仅作日志解析失败后的验证性回退）。
const OPENCODE_DEFAULT_PORT: u16 = 49374;
/// 本地回环请求超时。
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
/// 日志读取上限（只关心末尾的服务就绪行）。
const LOG_READ_LIMIT_BYTES: u64 = 4 * 1024 * 1024;

/// 读取默认位置的 OpenCode 可用模型列表（界面下拉 + 手动输入兜底）。
pub async fn list_models() -> Result<Vec<OpencodeModelDto>, CommandError> {
    let home = home_dir()?;
    let password = read_service_password(&home.join(OPENCODE_SERVICE_RELATIVE))?;

    let mut candidates: Vec<u16> = Vec::new();
    if let Some(port) = port_from_latest_log(&home.join(OPENCODE_LOGS_RELATIVE)) {
        candidates.push(port);
    }
    if !candidates.contains(&OPENCODE_DEFAULT_PORT) {
        candidates.push(OPENCODE_DEFAULT_PORT);
    }

    let client = reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .build()
        .map_err(|error| models_unavailable(format!("初始化 OpenCode 模型请求失败：{error}")))?;

    let mut last_error: Option<CommandError> = None;
    for port in candidates {
        match fetch_models(&client, port, &password).await {
            Ok(models) => return Ok(models),
            Err(error) => last_error = Some(error),
        }
    }
    Err(last_error.unwrap_or_else(|| {
        models_unavailable("未找到可连接的 OpenCode 服务（请确认 OpenCode 桌面端已打开）")
    }))
}

fn home_dir() -> Result<PathBuf, CommandError> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .ok_or_else(|| {
            CommandError::new(
                OPENCODE_SERVICE_INFO_MISSING,
                "无法确定用户目录，不能读取 OpenCode 模型列表",
            )
        })
}

/// 从 `service.json` 读取服务密码（Basic 认证用）：缺失/无效一律明确报错。
fn read_service_password(path: &Path) -> Result<String, CommandError> {
    if !path.is_file() {
        return Err(CommandError::new(
            OPENCODE_SERVICE_INFO_MISSING,
            format!(
                "未找到 OpenCode 服务信息：{}（可手动输入 provider/model）",
                path.display()
            ),
        ));
    }
    let raw = std::fs::read_to_string(path).map_err(|error| {
        CommandError::new(
            OPENCODE_SERVICE_INFO_MISSING,
            format!("读取 OpenCode 服务信息失败：{error}（可手动输入 provider/model）"),
        )
    })?;
    let parsed: ServiceInfo = serde_json::from_str(&raw).map_err(|error| {
        CommandError::new(
            OPENCODE_SERVICE_INFO_MISSING,
            format!("解析 OpenCode 服务信息失败：{error}（可手动输入 provider/model）"),
        )
    })?;
    let password = parsed.password.trim().to_string();
    if password.is_empty() {
        return Err(CommandError::new(
            OPENCODE_SERVICE_INFO_MISSING,
            "OpenCode 服务信息缺少 password（可手动输入 provider/model）",
        ));
    }
    Ok(password)
}

/// 从最新一次启动日志中解析后台服务端口（日志目录按时间戳命名；解析不到返回 None）。
fn port_from_latest_log(logs_dir: &Path) -> Option<u16> {
    let mut newest: Option<(String, PathBuf)> = None;
    for entry in std::fs::read_dir(logs_dir).ok()?.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if newest.as_ref().is_none_or(|(current, _)| name > *current) {
            newest = Some((name, path));
        }
    }
    let (_, dir) = newest?;
    let text = read_log_tail(&dir.join("main.log"))?;
    parse_last_service_port(&text)
}

/// 读取日志尾部（上限 `LOG_READ_LIMIT_BYTES`）：服务就绪行在末尾附近。
fn read_log_tail(path: &Path) -> Option<String> {
    use std::io::{Read, Seek, SeekFrom};

    let mut file = std::fs::File::open(path).ok()?;
    let length = file.metadata().ok()?.len();
    let offset = length.saturating_sub(LOG_READ_LIMIT_BYTES);
    file.seek(SeekFrom::Start(offset)).ok()?;
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer).ok()?;
    String::from_utf8(buffer).ok()
}

/// 解析日志文本中最后一次出现的服务地址端口（`url: 'http://127.0.0.1:<port>'`）。
fn parse_last_service_port(text: &str) -> Option<u16> {
    const MARKER: &str = "url: 'http://127.0.0.1:";
    let mut last: Option<u16> = None;
    let mut rest = text;
    while let Some(index) = rest.find(MARKER) {
        let after = &rest[index + MARKER.len()..];
        let digits: String = after.chars().take_while(char::is_ascii_digit).collect();
        if let Ok(port) = digits.parse::<u16>() {
            last = Some(port);
        }
        rest = &after[digits.len()..];
    }
    last
}

/// 请求一次 `/api/model`：非 2xx / 网络失败 / 响应解析失败都带明确中文原因。
async fn fetch_models(
    client: &reqwest::Client,
    port: u16,
    password: &str,
) -> Result<Vec<OpencodeModelDto>, CommandError> {
    let url = format!("http://127.0.0.1:{port}/api/model");
    let response = client
        .get(&url)
        .basic_auth("opencode", Some(password))
        .send()
        .await
        .map_err(|error| models_unavailable(format!("连接 OpenCode 服务失败（{url}）：{error}")))?;
    let status = response.status();
    if !status.is_success() {
        return Err(models_unavailable(format!(
            "OpenCode 服务未返回模型列表（{url} → HTTP {status}）"
        )));
    }
    let payload: ModelsResponse = response
        .json()
        .await
        .map_err(|error| models_unavailable(format!("解析 OpenCode 模型列表失败：{error}")))?;
    let models = normalize_models(payload.data);
    if models.is_empty() {
        return Err(models_unavailable(
            "OpenCode 未返回可用模型（请确认已配置并启用模型提供方）",
        ));
    }
    Ok(models)
}

/// 归一化：跳过停用/废弃项、去重、按名称排序（名称相同按 provider 稳定排序）。
fn normalize_models(raw: Vec<RawModel>) -> Vec<OpencodeModelDto> {
    let mut models: Vec<OpencodeModelDto> = raw
        .into_iter()
        .filter(|model| model.enabled != Some(false))
        .filter(|model| model.status.as_deref() != Some("deprecated"))
        .map(|model| {
            let name = model
                .name
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| model.id.clone());
            OpencodeModelDto {
                provider_id: model.provider_id.trim().to_string(),
                model_id: model.id.trim().to_string(),
                name,
            }
        })
        .filter(|model| !model.provider_id.is_empty() && !model.model_id.is_empty())
        .collect();
    models.sort_by(|left, right| {
        left.name
            .to_lowercase()
            .cmp(&right.name.to_lowercase())
            .then_with(|| left.provider_id.cmp(&right.provider_id))
    });
    models.dedup_by(|left, right| {
        left.provider_id == right.provider_id && left.model_id == right.model_id
    });
    models
}

fn models_unavailable(detail: impl Into<String>) -> CommandError {
    CommandError::new(
        OPENCODE_MODELS_UNAVAILABLE,
        format!("读取 OpenCode 模型列表失败：{}", detail.into()),
    )
}

#[derive(Deserialize)]
struct ServiceInfo {
    password: String,
}

#[derive(Deserialize)]
struct ModelsResponse {
    #[serde(default)]
    data: Vec<RawModel>,
}

#[derive(Deserialize)]
struct RawModel {
    id: String,
    #[serde(rename = "providerID")]
    provider_id: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    enabled: Option<bool>,
    #[serde(default)]
    status: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 端口解析：取最后一次出现的服务地址；非数字/超范围忽略。
    #[test]
    fn parse_last_service_port_reads_latest_line() {
        let text = "v2 CLI background service ready { url: 'http://127.0.0.1:49374' }\n\
                    ...\n\
                    v2 CLI background service ready { url: 'http://127.0.0.1:51234', hostname }";
        assert_eq!(parse_last_service_port(text), Some(51234));
        assert_eq!(parse_last_service_port("no url here"), None);
        assert_eq!(
            parse_last_service_port("url: 'http://127.0.0.1:99999999'"),
            None,
            "超出 u16 的端口必须忽略（不是猜测兜底）"
        );
    }

    /// 日志目录解析：按时间戳目录名取最新一次的 main.log。
    #[test]
    fn port_from_latest_log_prefers_newest_directory() {
        let root = tempfile::tempdir().expect("测试临时目录必须可创建");
        let logs = root.path().join("logs");
        for (name, port) in [("20260101T000000", 11111), ("20260102T000000", 22222)] {
            let dir = logs.join(name);
            std::fs::create_dir_all(&dir).expect("创建日志目录必须成功");
            std::fs::write(
                dir.join("main.log"),
                format!("v2 CLI background service ready {{ url: 'http://127.0.0.1:{port}' }}"),
            )
            .expect("写日志必须成功");
        }
        assert_eq!(port_from_latest_log(&logs), Some(22222));
        assert_eq!(port_from_latest_log(&root.path().join("missing")), None);
    }

    /// 模型归一化：过滤停用/废弃、名称兜底 id、按名称排序、去重。
    #[test]
    fn normalize_models_filters_and_sorts() {
        let models = normalize_models(vec![
            RawModel {
                id: "beta".into(),
                provider_id: "p1".into(),
                name: Some("  Beta  ".into()),
                enabled: Some(true),
                status: None,
            },
            RawModel {
                id: "disabled".into(),
                provider_id: "p1".into(),
                name: None,
                enabled: Some(false),
                status: None,
            },
            RawModel {
                id: "old".into(),
                provider_id: "p1".into(),
                name: Some("Old".into()),
                enabled: Some(true),
                status: Some("deprecated".into()),
            },
            RawModel {
                id: "alpha".into(),
                provider_id: "p2".into(),
                name: None,
                enabled: None,
                status: None,
            },
            RawModel {
                id: "beta".into(),
                provider_id: "p1".into(),
                name: Some("Beta".into()),
                enabled: Some(true),
                status: None,
            },
        ]);
        let keys: Vec<String> = models
            .iter()
            .map(|model| format!("{}/{}", model.provider_id, model.model_id))
            .collect();
        assert_eq!(
            keys,
            vec!["p2/alpha", "p1/beta"],
            "停用/废弃过滤 + 名称排序 + 去重"
        );
        assert_eq!(models[0].name, "alpha", "名称缺失时用 id 兜底");
        assert_eq!(models[1].name, "Beta");
    }

    /// 服务信息：缺失/无密码 → 明确错误（界面退回手动输入）。
    #[test]
    fn service_password_requires_non_empty_password() {
        let root = tempfile::tempdir().expect("测试临时目录必须可创建");
        let missing = root.path().join("service.json");
        let error = read_service_password(&missing).expect_err("缺失必须报错");
        assert_eq!(error.code(), OPENCODE_SERVICE_INFO_MISSING);
        assert!(error.message().contains("手动输入"));

        let empty = root.path().join("empty.json");
        std::fs::write(&empty, r#"{"password":"   "}"#).expect("写文件必须成功");
        assert!(read_service_password(&empty).is_err());

        let valid = root.path().join("valid.json");
        std::fs::write(&valid, r#"{"password":" secret "}"#).expect("写文件必须成功");
        assert_eq!(
            read_service_password(&valid).expect("有效密码必须可读"),
            "secret"
        );
    }
}
