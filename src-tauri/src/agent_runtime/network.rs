//! L1 外部只读工具的网络边界：`web_search`（可替换 SearchProvider，任务 3
//! 只实现 Wikipedia provider）与受控 `fetch_web_page`。
//!
//! 安全边界（方案 §15）：SSRF 拒绝私网/回环/保留地址；每次解析与每次重定向
//! 重新校验域名与最终 IP（防 DNS rebinding）；解析后把连接目标固定到已验证 IP；
//! 限制重定向次数、超时与响应字节数；只接受文本内容类型；不携带 cookies 与
//! 用户自定义 header；抓取正文剥离 script/iframe 等 active content。外部内容
//! 始终作为不可信数据回传模型，不覆盖系统策略。

use crate::agent_runtime::protocol::{AgentError, AgentErrorKind, SourceMetadata};
use futures_util::future::BoxFuture;
use serde_json::json;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, ToSocketAddrs};

/// 单次抓取最大重定向跳数。
const MAX_REDIRECTS: usize = 5;
/// 抓取响应体上限；超过即截断并标记（防止压缩炸弹/无限下载）。
const MAX_RESPONSE_BYTES: usize = 2 * 1024 * 1024;
/// 抓取连接超时。
const CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);
/// 抓取整体超时（读取每个 chunk 后重置的读超时由 reqwest timeout 兜底）。
const FETCH_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);
/// 搜索结果数量上限（Wikipedia srlimit）。
const MAX_WIKIPEDIA_RESULTS: u32 = 5;

/// 只允许的文本内容类型；`;` 前的媒体类型（小写）必须命中白名单。
const ALLOWED_CONTENT_TYPES: &[&str] = &[
    "text/plain",
    "text/html",
    "text/markdown",
    "text/xml",
    "application/json",
    "application/xml",
    "application/xhtml+xml",
    "application/ld+json",
    "application/rss+xml",
    "application/atom+xml",
];

// ---------------------------------------------------------------------------
// URL 与 IP 校验（纯函数，可离线测试）
// ---------------------------------------------------------------------------

/// 解析并校验抓取目标 URL。返回 (scheme, host, port)。
/// 拒绝：非 http/https、userinfo、敏感查询参数、私网/回环/保留主机名与 IP。
pub(crate) fn validate_fetch_url(url: &str) -> Result<(String, String, Option<u16>), String> {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .ok_or_else(|| "抓取 URL 只能使用 HTTP(S)。".to_string())?;
    let (authority, _path_and_query) = rest
        .split_once(['/', '?', '#'])
        .map_or((rest, ""), |(authority, remainder)| (authority, remainder));
    if authority.is_empty() || authority.contains('@') {
        return Err("抓取 URL 不得包含 userinfo。".to_string());
    }
    let scheme = if url.starts_with("https://") {
        "https"
    } else {
        "http"
    };
    let (host, port) = split_host_port(authority)?;
    validate_host(&host)?;
    let lower_query = _path_and_query.to_ascii_lowercase();
    for pair in lower_query.split('&') {
        let key = pair.split('=').next().unwrap_or("");
        if [
            "key",
            "api_key",
            "apikey",
            "token",
            "access_token",
            "auth",
            "authorization",
            "password",
            "passwd",
            "secret",
            "cookie",
            "session",
            "credential",
        ]
        .iter()
        .any(|sensitive| key.contains(sensitive))
        {
            return Err("抓取 URL 查询参数疑似包含凭据，已拒绝。".to_string());
        }
    }
    Ok((scheme.to_string(), host, port))
}

fn split_host_port(authority: &str) -> Result<(String, Option<u16>), String> {
    if let Some(bracketed) = authority.strip_prefix('[') {
        let end = bracketed
            .find(']')
            .ok_or_else(|| "IPv6 主机必须使用 [] 包裹。".to_string())?;
        let host = &bracketed[..end];
        let port = bracketed[end + 1..]
            .strip_prefix(':')
            .map(parse_port)
            .transpose()?;
        return Ok((host.to_string(), port));
    }
    if let Some((host, port)) = authority.rsplit_once(':') {
        if host.contains(':') {
            // 未加 [] 的多冒号字面量是畸形 IPv6。
            return Err("IPv6 地址必须使用 [] 包裹。".to_string());
        }
        return Ok((host.to_string(), Some(parse_port(port)?)));
    }
    Ok((authority.to_string(), None))
}

fn parse_port(port: &str) -> Result<u16, String> {
    let port: u16 = port
        .parse()
        .map_err(|_| "抓取 URL 端口无效。".to_string())?;
    if port == 0 {
        return Err("抓取 URL 端口不能为 0。".to_string());
    }
    Ok(port)
}

/// 主机名校验：域名结构、黑名单、字面量 IP 直接查保留网段。
fn validate_host(host: &str) -> Result<(), String> {
    if host.is_empty() || host.len() > 253 {
        return Err("抓取 URL 主机名无效。".to_string());
    }
    for label in host.split('.') {
        if label.is_empty() || label.len() > 63 {
            return Err("抓取 URL 主机名标签无效。".to_string());
        }
    }
    let lower = host.to_ascii_lowercase();
    let is_bare_ip = lower.parse::<IpAddr>().is_ok();
    if is_bare_ip {
        let ip: IpAddr = lower.parse().expect("已确认可解析");
        if is_private_or_reserved_ip(ip) {
            return Err("抓取目标 IP 属于私网/回环/保留地址，已拒绝。".to_string());
        }
        return Ok(());
    }
    let labels: Vec<&str> = lower.split('.').collect();
    let last = labels.last().copied().unwrap_or("");
    if matches!(
        last,
        "local" | "localhost" | "internal" | "lan" | "home" | "localdomain"
    ) {
        return Err("抓取 URL 使用保留域名后缀，已拒绝。".to_string());
    }
    if labels.first() == Some(&"localhost") || lower == "localhost" {
        return Err("抓取 URL 指向 localhost，已拒绝。".to_string());
    }
    if !lower
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_'))
    {
        return Err("抓取 URL 主机名包含非法字符。".to_string());
    }
    Ok(())
}

/// 私网/回环/链路本地/保留/组播/文档/云 metadata 网段一律拒绝。
pub(crate) fn is_private_or_reserved_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ipv4) => is_reserved_ipv4(ipv4),
        IpAddr::V6(ipv6) => is_reserved_ipv6(ipv6),
    }
}

fn is_reserved_ipv4(ip: Ipv4Addr) -> bool {
    let octets = ip.octets();
    ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_broadcast()
        || ip.is_unspecified()
        || ip.is_multicast()
        // 100.64.0.0/10（运营商级 NAT，std is_shared 未稳定）
        || (octets[0] == 100 && octets[1] & 0xC0 == 0x40)
        // 198.18.0.0/15（基准测试网段，std is_benchmarking 未稳定）
        || (octets[0] == 198 && octets[1] & 0xFE == 0x12)
        // 240.0.0.0/4（保留网段，std is_reserved 未稳定）
        || octets[0] >= 240
        // 云 metadata（169.254.169.254 落在 link-local 内，此处显式兜底）。
        || ip == Ipv4Addr::new(169, 254, 169, 254)
}

fn is_reserved_ipv6(ip: Ipv6Addr) -> bool {
    ip.is_loopback()
        || ip.is_unspecified()
        || ip.is_multicast()
        || ip.is_unique_local()
        || ip.is_unicast_link_local()
        // IPv4-mapped（::ffff:0:0/96）按内嵌 IPv4 再查保留网段。
        || ip.to_ipv4_mapped().is_some_and(is_reserved_ipv4)
        // 2001:db8::/32（文档网段，std is_documentation 在 IPv6 上未稳定）。
        || ip.octets()[..4] == [0x20, 0x01, 0x0d, 0xb8]
}

/// 解析域名到全部 IP，逐个执行保留网段校验；返回通过的地址。
pub(crate) fn resolve_safe_addrs(host: &str, port: u16) -> Result<Vec<SocketAddr>, String> {
    let addrs: Vec<SocketAddr> = (host, port)
        .to_socket_addrs()
        .map_err(|_| format!("无法解析抓取目标主机：{host}。"))?
        .collect();
    if addrs.is_empty() {
        return Err(format!("抓取目标主机解析结果为空：{host}。"));
    }
    let mut safe: Vec<SocketAddr> = Vec::new();
    for addr in addrs {
        if is_private_or_reserved_ip(addr.ip()) {
            continue;
        }
        if !safe.contains(&addr) {
            safe.push(addr);
        }
    }
    if safe.is_empty() {
        return Err(format!(
            "抓取目标 {host} 的全部解析地址都属于保留网段，已拒绝。"
        ));
    }
    Ok(safe)
}

/// DNS 解析边界：生产用系统解析器，测试注入固定公网地址（避免真实 DNS）。
pub(crate) trait DnsResolver: Send + Sync {
    fn resolve(&self, host: &str, port: u16) -> Result<Vec<SocketAddr>, String>;
}

pub(crate) struct SystemDnsResolver;

impl DnsResolver for SystemDnsResolver {
    fn resolve(&self, host: &str, port: u16) -> Result<Vec<SocketAddr>, String> {
        resolve_safe_addrs(host, port)
    }
}

// ---------------------------------------------------------------------------
// 受控抓取
// ---------------------------------------------------------------------------

/// 可注入的 HTTP 传输边界：每跳解析固定 IP 后发起一次 GET。
pub(crate) trait FetchTransport: Send + Sync {
    fn get(
        &self,
        url: &str,
        fixed_addr: Option<SocketAddr>,
    ) -> BoxFuture<'static, Result<FetchResponse, String>>;
}

#[derive(Debug)]
pub(crate) struct FetchResponse {
    pub status: u16,
    pub location: Option<String>,
    pub content_type: Option<String>,
    pub body: Vec<u8>,
    pub truncated: bool,
}

/// 生产传输：一次性 client（resolve 固定已验证 IP、无 cookies、无自动重定向、
/// 严格超时）。
pub(crate) struct ReqwestFetchTransport;

impl FetchTransport for ReqwestFetchTransport {
    fn get(
        &self,
        url: &str,
        fixed_addr: Option<SocketAddr>,
    ) -> BoxFuture<'static, Result<FetchResponse, String>> {
        let url = url.to_string();
        Box::pin(async move {
            let mut builder = reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .connect_timeout(CONNECT_TIMEOUT)
                .timeout(FETCH_TIMEOUT)
                .user_agent("ReadRay/0.1 (English-learning desktop app)");
            if let Some(addr) = fixed_addr {
                // 把域名固定到已通过保留网段校验的 IP，防 DNS rebinding。
                builder = builder.resolve(&url_host(&url)?, addr);
            }
            let client = builder
                .build()
                .map_err(|error| format!("抓取客户端创建失败：{error}"))?;
            let response = client
                .get(&url)
                .send()
                .await
                .map_err(|error| format!("抓取请求失败：{error}"))?;
            let status = response.status().as_u16();
            let location = response
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|value| value.to_str().ok())
                .map(str::to_string);
            let content_type = response
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok())
                .map(|value| value.split(';').next().unwrap_or(value).trim().to_string());
            let mut body = Vec::new();
            let mut truncated = false;
            let mut stream = response.bytes_stream();
            while let Some(chunk) = futures_util::StreamExt::next(&mut stream).await {
                let chunk = chunk.map_err(|error| format!("抓取响应读取失败：{error}"))?;
                if body.len().saturating_add(chunk.len()) > MAX_RESPONSE_BYTES {
                    truncated = true;
                    break;
                }
                body.extend_from_slice(&chunk);
            }
            Ok(FetchResponse {
                status,
                location,
                content_type,
                body,
                truncated,
            })
        })
    }
}

fn url_host(url: &str) -> Result<String, String> {
    validate_fetch_url(url).map(|(_, host, _)| host)
}

/// 抓取结果（正文已裁剪为受控文本）。
#[derive(Debug)]
pub(crate) struct FetchOutcome {
    pub canonical_url: String,
    pub title: Option<String>,
    pub content_type: Option<String>,
    pub truncated: bool,
    pub text: String,
}

/// 受控网页抓取器。`transport`/`resolver` 注入后完全离线可测。
pub(crate) struct WebFetcher {
    transport: Box<dyn FetchTransport>,
    resolver: Box<dyn DnsResolver>,
}

impl Default for WebFetcher {
    fn default() -> Self {
        Self {
            transport: Box::new(ReqwestFetchTransport),
            resolver: Box::new(SystemDnsResolver),
        }
    }
}

impl WebFetcher {
    #[cfg(test)]
    pub(crate) fn with_transport(transport: Box<dyn FetchTransport>) -> Self {
        Self {
            transport,
            resolver: Box::new(SystemDnsResolver),
        }
    }

    #[cfg(test)]
    pub(crate) fn with_deps(
        transport: Box<dyn FetchTransport>,
        resolver: Box<dyn DnsResolver>,
    ) -> Self {
        Self {
            transport,
            resolver,
        }
    }

    /// 抓取单个 URL：逐跳校验 URL 与解析后的 IP，限制重定向/大小/内容类型。
    ///
    /// 错误分类（问题 3 评审）：`NetworkBlocked` 只留给真正的安全拒绝——SSRF
    /// （私网/回环/保留网段/云 metadata）、userinfo 与凭据 URL、重定向到非
    /// HTTP(S) 协议；其余运行时失败（DNS 解析失败、传输错误、非 200、缺少
    /// Location、内容类型拒绝、重定向超限）归 `ToolExecutionFailed`，由模型
    /// 诚实降级（可恢复，不杀死整个 run）。
    pub(crate) fn fetch(&self, url: &str) -> Result<FetchOutcome, AgentError> {
        let (mut scheme, mut host, mut port) = validate_fetch_url(url).map_err(network_error)?;
        let mut current_url = url.to_string();
        for _ in 0..=MAX_REDIRECTS {
            let effective_port = port.unwrap_or(if scheme == "https" { 443 } else { 80 });
            let addrs = self
                .resolver
                .resolve(&host, effective_port)
                .map_err(|message| {
                    tool_error(
                        AgentErrorKind::ToolExecutionFailed,
                        format!("无法解析抓取目标 {host}：{message}"),
                    )
                })?;
            // 独立防御：resolver 返回后再次过滤保留网段（防解析层被绕过）。
            let mut safe_addrs: Vec<SocketAddr> = Vec::new();
            for addr in addrs {
                if is_private_or_reserved_ip(addr.ip()) {
                    continue;
                }
                if !safe_addrs.contains(&addr) {
                    safe_addrs.push(addr);
                }
            }
            if safe_addrs.is_empty() {
                return Err(network_error(format!(
                    "抓取目标 {host} 的全部解析地址都属于保留网段，已拒绝。"
                )));
            }
            let response = tauri::async_runtime::block_on(
                self.transport
                    .get(&current_url, safe_addrs.first().copied()),
            )
            .map_err(|message| {
                tool_error(
                    AgentErrorKind::ToolExecutionFailed,
                    format!("{current_url} 抓取失败：{message}"),
                )
            })?;

            if (300..400).contains(&response.status) {
                let Some(location) = response.location.as_deref() else {
                    return Err(tool_error(
                        AgentErrorKind::ToolExecutionFailed,
                        format!("重定向响应缺少 Location：{current_url}。"),
                    ));
                };
                let next = resolve_redirect_url(&current_url, location).map_err(|message| {
                    // 重定向到非 HTTP(S) 协议属于安全绕过尝试，按安全拒绝处理。
                    network_error(message)
                })?;
                let validated = validate_fetch_url(&next).map_err(network_error)?;
                scheme = validated.0;
                host = validated.1;
                port = validated.2;
                current_url = next;
                continue;
            }

            if response.status != 200 {
                return Err(tool_error(
                    AgentErrorKind::ToolExecutionFailed,
                    format!("网页返回 HTTP {}：{current_url}。", response.status),
                ));
            }
            let content_type = response.content_type.clone();
            let Some(media_type) = content_type.as_deref() else {
                return Err(tool_error(
                    AgentErrorKind::ToolExecutionFailed,
                    "网页缺少 Content-Type，已拒绝。".to_string(),
                ));
            };
            if !ALLOWED_CONTENT_TYPES.contains(&media_type.to_ascii_lowercase().as_str()) {
                return Err(tool_error(
                    AgentErrorKind::ToolExecutionFailed,
                    format!("网页内容类型 {media_type} 不是允许的文本类型，已拒绝。"),
                ));
            }
            let raw = String::from_utf8_lossy(&response.body);
            let (title, text) = if media_type.eq_ignore_ascii_case("text/html")
                || media_type.eq_ignore_ascii_case("application/xhtml+xml")
            {
                extract_html(&raw)
            } else {
                (None, compress_whitespace(raw.trim()))
            };
            // 防御性兜底：transport 返回的 body 也按同一上限截断并标记。
            let truncated = response.truncated || response.body.len() > MAX_RESPONSE_BYTES;
            let text = if truncated && text.len() > MAX_RESPONSE_BYTES {
                text.chars().take(MAX_RESPONSE_BYTES).collect()
            } else {
                text
            };
            return Ok(FetchOutcome {
                canonical_url: current_url,
                title,
                content_type,
                truncated,
                text,
            });
        }
        Err(tool_error(
            AgentErrorKind::ToolExecutionFailed,
            format!("重定向次数超过 {MAX_REDIRECTS} 次，已放弃。"),
        ))
    }
}

/// 解析重定向 Location（相对/绝对/协议相对），结果必须仍是 HTTP(S)。
fn resolve_redirect_url(current: &str, location: &str) -> Result<String, String> {
    if let Some((candidate, _)) = location.split_once("://") {
        if !matches!(candidate, "http" | "https") {
            return Err(format!("重定向目标使用了不允许的协议：{candidate}。"));
        }
        return Ok(location.to_string());
    }
    let (scheme, authority) = current
        .strip_prefix("https://")
        .map(|rest| ("https", rest))
        .or_else(|| current.strip_prefix("http://").map(|rest| ("http", rest)))
        .ok_or_else(|| "当前抓取 URL 协议无效。".to_string())?;
    let authority = authority
        .split(['/', '?', '#'])
        .next()
        .ok_or_else(|| "当前抓取 URL 主机无效。".to_string())?;
    if let Some(path) = location.strip_prefix("//") {
        return Ok(format!("{scheme}://{path}"));
    }
    if let Some(path) = location.strip_prefix('/') {
        return Ok(format!("{scheme}://{authority}/{path}"));
    }
    // 相对路径：去掉当前路径的最后一段后拼接。
    let base = current
        .split(['?', '#'])
        .next()
        .unwrap_or(current)
        .rsplit_once('/')
        .map(|(prefix, _)| prefix.to_string())
        .unwrap_or_else(|| format!("{scheme}://{authority}"));
    Ok(format!("{base}/{location}"))
}

/// 抽取 HTML 的 title 与去除 active content 后的正文文本。
fn extract_html(raw: &str) -> (Option<String>, String) {
    // title 从原始 HTML 提取（head 块会在剥离时被移除）。
    let title = extract_title(raw);
    let without_blocks = strip_active_blocks(raw);
    let text = strip_tags(&without_blocks);
    let text = decode_entities(&text);
    (title, compress_whitespace(&text))
}

/// 移除 script/style/noscript/iframe/svg/template/注释等 active 或非正文块。
fn strip_active_blocks(html: &str) -> String {
    const BLOCKS: &[&str] = &[
        "script", "style", "noscript", "iframe", "svg", "template", "head",
    ];
    let mut result = html.to_string();
    for tag in BLOCKS {
        let (open, close) = (format!("<{tag}"), format!("</{tag}>"));
        loop {
            let Some(start) = result.to_ascii_lowercase().find(&open) else {
                break;
            };
            let Some(end) = result.to_ascii_lowercase().find(&close) else {
                result.replace_range(start.., "");
                break;
            };
            result.replace_range(start..end + close.len(), " ");
        }
    }
    let mut filtered = String::with_capacity(result.len());
    let mut rest = result.as_str();
    while let Some(start) = rest.find("<!--") {
        filtered.push_str(&rest[..start]);
        let Some(end) = rest.find("-->") else {
            filtered.push_str(" ");
            rest = "";
            break;
        };
        filtered.push(' ');
        rest = &rest[end + 3..];
    }
    filtered.push_str(rest);
    filtered
}

fn extract_title(html: &str) -> Option<String> {
    let lower = html.to_ascii_lowercase();
    let start = lower.find("<title>")? + "<title>".len();
    let end = lower[start..].find("</title>")? + start;
    let title = decode_entities(&html[start..end]);
    let title = compress_whitespace(&title);
    if title.is_empty() {
        None
    } else {
        Some(title)
    }
}

/// 去除全部标签，保留标签间文本。
fn strip_tags(html: &str) -> String {
    let mut text = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(start) = rest.find('<') {
        text.push_str(&rest[..start]);
        let Some(end) = rest[start..].find('>') else {
            break;
        };
        rest = &rest[start + end + 1..];
    }
    text.push_str(rest);
    text
}

/// 解码常用 HTML 实体（含数字与十六进制实体）。
fn decode_entities(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('&') {
        result.push_str(&rest[..start]);
        let Some(end) = rest[start..].find(';') else {
            result.push_str(&rest[start..]);
            return result;
        };
        let entity = &rest[start + 1..start + end];
        let decoded = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" | "#39" => Some('\''),
            "nbsp" => Some(' '),
            _ => entity
                .strip_prefix("#x")
                .and_then(|hex| u32::from_str_radix(hex, 16).ok())
                .or_else(|| entity.strip_prefix('#').and_then(|dec| dec.parse().ok()))
                .and_then(char::from_u32),
        };
        match decoded {
            Some(character) => {
                result.push(character);
                rest = &rest[start + end + 1..];
            }
            None => {
                result.push('&');
                rest = &rest[start + 1..];
            }
        }
    }
    result.push_str(rest);
    result
}

/// 把空白序列压缩为单个空格并去除首尾。
fn compress_whitespace(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut pending_space = false;
    for character in text.chars() {
        if character.is_whitespace() {
            pending_space = true;
        } else {
            if pending_space && !result.is_empty() {
                result.push(' ');
            }
            pending_space = false;
            result.push(character);
        }
    }
    result
}

// ---------------------------------------------------------------------------
// web_search：可替换 SearchProvider，任务 3 只实现 Wikipedia
// ---------------------------------------------------------------------------

/// 单条搜索结果（标题/URL/摘要）。
#[derive(Debug)]
pub(crate) struct SearchResultItem {
    pub title: String,
    pub url: String,
    pub snippet: String,
}

/// 搜索 provider 边界：未来接入 Tavily 等 key 服务时只替换实现，
/// 不改变 Agent loop 与 UI 协议（方案 §14.1）。
pub(crate) trait SearchProvider: Send + Sync {
    fn search(
        &self,
        query: &str,
        lang: &str,
        max_results: u32,
    ) -> Result<Vec<SearchResultItem>, AgentError>;
}

/// Wikipedia API provider（无 key）：search + snippet，覆盖范围限于维基百科。
pub(crate) struct WikipediaSearchProvider;

impl SearchProvider for WikipediaSearchProvider {
    fn search(
        &self,
        query: &str,
        lang: &str,
        max_results: u32,
    ) -> Result<Vec<SearchResultItem>, AgentError> {
        let limit = max_results.clamp(1, MAX_WIKIPEDIA_RESULTS);
        let endpoint = format!("https://{lang}.wikipedia.org/w/api.php");
        let request_body = json!({
            "action": "query",
            "list": "search",
            "srsearch": query,
            "srlimit": limit,
            "format": "json",
            "redirects": 1,
        });
        let response = tauri::async_runtime::block_on(async {
            crate::deepseek_client::shared_http_client()?
                .get(&endpoint)
                .query(&request_body)
                .send()
                .await
                .map_err(|_| "Wikipedia 搜索请求失败。".to_string())
        })
        .map_err(|message| tool_error(AgentErrorKind::ToolExecutionFailed, message))?;
        if !response.status().is_success() {
            return Err(tool_error(
                AgentErrorKind::ToolExecutionFailed,
                format!("Wikipedia 搜索返回 HTTP {}。", response.status().as_u16()),
            ));
        }
        let body = tauri::async_runtime::block_on(async {
            response
                .text()
                .await
                .map_err(|_| "Wikipedia 搜索响应读取失败。".to_string())
        })
        .map_err(|message| tool_error(AgentErrorKind::ToolExecutionFailed, message))?;
        self.parse_response(&body, lang)
    }
}

impl WikipediaSearchProvider {
    /// 解析 Wikipedia API 的 query.search JSON 响应（离线可测）。
    ///
    /// 区分"无结果"与"解析失败"（问题 4 评审）：JSON 非法或响应缺少
    /// query.search 列表是解析/协议失败（ToolExecutionFailed），只有真正
    /// 返回空 search 数组才是"没有找到匹配条目"。
    fn parse_response(&self, body: &str, lang: &str) -> Result<Vec<SearchResultItem>, AgentError> {
        let value: serde_json::Value = serde_json::from_str(body).map_err(|error| {
            tool_error(
                AgentErrorKind::ToolExecutionFailed,
                format!("Wikipedia 搜索响应不是合法 JSON：{error}"),
            )
        })?;
        let Some(items) = value
            .get("query")
            .and_then(|query| query.get("search"))
            .and_then(serde_json::Value::as_array)
        else {
            return Err(tool_error(
                AgentErrorKind::ToolExecutionFailed,
                "Wikipedia 搜索响应缺少结果列表（协议或服务错误）。",
            ));
        };
        let mut results = Vec::with_capacity(items.len());
        for item in items {
            let Some(title) = item.get("title").and_then(serde_json::Value::as_str) else {
                continue;
            };
            let title = title.trim();
            if title.is_empty() {
                continue;
            }
            let slug = title.replace(' ', "_");
            let url = format!("https://{lang}.wikipedia.org/wiki/{slug}");
            let snippet = item
                .get("snippet")
                .and_then(serde_json::Value::as_str)
                .map(|snippet| compress_whitespace(&decode_entities(&strip_tags(snippet))))
                .unwrap_or_default();
            results.push(SearchResultItem {
                title: title.to_string(),
                url,
                snippet,
            });
        }
        Ok(results)
    }
}

/// 搜索/抓取工具结果中携带的结构化来源（SourceMetadata 数组）。
pub(crate) fn sources_from_details(details: &Option<serde_json::Value>) -> Vec<SourceMetadata> {
    let Some(details) = details else {
        return Vec::new();
    };
    let Some(sources) = details.get("sources").and_then(serde_json::Value::as_array) else {
        return Vec::new();
    };
    let mut result = Vec::new();
    for source in sources {
        if let Ok(source) = serde_json::from_value::<SourceMetadata>(source.clone()) {
            if source.validate().is_ok() {
                result.push(source);
            }
        }
    }
    result
}

/// 从查询与来源构造 web_search 的 ToolResult 内容（只陈述真实结果）。
pub(crate) fn format_search_content(results: &[SearchResultItem], lang: &str) -> String {
    if results.is_empty() {
        return format!(
            "维基百科（{lang}）没有找到匹配条目。注意：搜索结果只覆盖维基百科，\
             不是通用网页搜索；不得把模型记忆中的信息冒充为已核实事实。"
        );
    }
    let mut lines = vec![format!(
        "维基百科（{lang}）搜索结果（共 {} 条，只覆盖维基百科，非通用搜索）：",
        results.len()
    )];
    for (index, item) in results.iter().enumerate() {
        lines.push(format!("{}. {} — {}", index + 1, item.title, item.url));
        if !item.snippet.trim().is_empty() {
            lines.push(format!("   摘要：{}", item.snippet));
        }
    }
    lines.join("\n")
}

/// 把搜索结果投影为 ToolResult.details 的来源数组。
pub(crate) fn search_details_sources(
    results: &[SearchResultItem],
    lang: &str,
    retrieved_at_unix_ms: u64,
) -> serde_json::Value {
    let sources: Vec<serde_json::Value> = results
        .iter()
        .map(|item| {
            serde_json::to_value(SourceMetadata {
                source_id: stable_source_id(&item.url),
                title: item.title.clone(),
                url: item.url.clone(),
                site_name: Some(format!("Wikipedia ({lang})")),
                published_at: None,
                retrieved_at_unix_ms,
                content_type: Some("text/html".to_string()),
            })
            .expect("SourceMetadata 必须可序列化")
        })
        .collect();
    json!({ "sources": sources })
}

/// 与来源元数据同构的稳定 source_id（不暴露 URL 或其前缀）。
pub(crate) fn stable_source_id(url: &str) -> String {
    const OFFSET_BASIS: [u64; 4] = [
        0xcbf29ce484222325,
        0x84222325cbf29ce4,
        0x9e3779b185ebca87,
        0xd6e8feb86659fd93,
    ];
    const PRIME: u64 = 0x00000100000001b3;
    let mut lanes = OFFSET_BASIS;
    for (index, byte) in url.bytes().enumerate() {
        for (lane_index, lane) in lanes.iter_mut().enumerate() {
            let salt = (lane_index as u64 + 1).wrapping_mul(0x9e3779b9);
            *lane ^=
                u64::from(byte).wrapping_add((index as u64).rotate_left(lane_index as u32)) ^ salt;
            *lane = lane.wrapping_mul(PRIME);
        }
    }
    format!(
        "source-{0:016x}{1:016x}{2:016x}{3:016x}",
        lanes[0], lanes[1], lanes[2], lanes[3]
    )
}

fn network_error(message: impl Into<String>) -> AgentError {
    tool_error(AgentErrorKind::NetworkBlocked, message)
}

fn tool_error(kind: AgentErrorKind, message: impl Into<String>) -> AgentError {
    AgentError::new(kind, message).expect("网络工具错误消息必须有效")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn fetch_url_validation_accepts_public_http_and_https() {
        assert_eq!(
            validate_fetch_url("https://example.com/article?q=rust").unwrap(),
            ("https".to_string(), "example.com".to_string(), None)
        );
        assert_eq!(
            validate_fetch_url("http://example.com:8080/path").unwrap(),
            ("http".to_string(), "example.com".to_string(), Some(8080))
        );
        assert!(
            validate_fetch_url("https://en.wikipedia.org/wiki/Rust_(programming_language)").is_ok()
        );
    }

    #[test]
    fn fetch_url_validation_rejects_bad_schemes_and_userinfo() {
        for url in [
            "ftp://example.com/file",
            "javascript:alert(1)",
            "file:///etc/passwd",
            "data:text/html,hi",
            "https://user:pass@example.com/",
            "https://example.com/page?api_key=redacted",
            "https://example.com/page?token=abc",
            "https://",
        ] {
            assert!(validate_fetch_url(url).is_err(), "应拒绝：{url}");
        }
    }

    #[test]
    fn fetch_url_validation_rejects_private_and_reserved_hosts() {
        for url in [
            "http://localhost/",
            "http://localhost.localdomain/",
            "http://127.0.0.1/",
            "http://10.0.0.1/",
            "http://172.16.0.1/",
            "http://192.168.1.1/",
            "http://169.254.169.254/latest/meta-data/",
            "http://0.0.0.0/",
            "http://224.0.0.1/",
            "http://[::1]/",
            "http://[fc00::1]/",
            "http://[fe80::1]/",
            "http://[::ffff:127.0.0.1]/",
            "http://test.local/",
            "http://router.internal/",
        ] {
            assert!(validate_fetch_url(url).is_err(), "应拒绝：{url}");
        }
    }

    #[test]
    fn reserved_ip_classification_covers_all_netblocks() {
        let cases: &[(IpAddr, bool)] = &[
            ("8.8.8.8".parse().unwrap(), false),
            ("1.1.1.1".parse().unwrap(), false),
            ("127.0.0.1".parse().unwrap(), true),
            ("127.255.255.255".parse().unwrap(), true),
            ("10.1.2.3".parse().unwrap(), true),
            ("172.16.0.1".parse().unwrap(), true),
            ("172.31.255.255".parse().unwrap(), true),
            ("172.32.0.1".parse().unwrap(), false),
            ("192.168.0.1".parse().unwrap(), true),
            ("169.254.169.254".parse().unwrap(), true),
            ("0.0.0.0".parse().unwrap(), true),
            ("224.0.0.1".parse().unwrap(), true),
            ("239.255.255.255".parse().unwrap(), true),
            ("240.0.0.1".parse().unwrap(), true),
            ("198.18.0.1".parse().unwrap(), true),
            ("100.64.0.1".parse().unwrap(), true),
            ("::1".parse().unwrap(), true),
            ("::".parse().unwrap(), true),
            ("fc00::1".parse().unwrap(), true),
            ("fe80::1".parse().unwrap(), true),
            ("ff02::1".parse().unwrap(), true),
            ("::ffff:10.0.0.1".parse().unwrap(), true),
            ("::ffff:8.8.8.8".parse().unwrap(), false),
            ("2001:db8::1".parse().unwrap(), true),
            ("2606:4700:4700::1111".parse().unwrap(), false),
        ];
        for (ip, expected) in cases {
            assert_eq!(is_private_or_reserved_ip(*ip), *expected, "ip={ip}");
        }
    }

    #[test]
    fn redirect_resolution_handles_relative_protocol_relative_and_absolute() {
        let current = "https://example.com/a/b?x=1";
        assert_eq!(
            resolve_redirect_url(current, "/c").unwrap(),
            "https://example.com/c"
        );
        assert_eq!(
            resolve_redirect_url(current, "c").unwrap(),
            "https://example.com/a/c"
        );
        assert_eq!(
            resolve_redirect_url(current, "//other.com/d").unwrap(),
            "https://other.com/d"
        );
        assert_eq!(
            resolve_redirect_url(current, "https://other.com/e").unwrap(),
            "https://other.com/e"
        );
        assert!(resolve_redirect_url(current, "file:///etc").is_err());
    }

    #[test]
    fn html_extraction_strips_active_content_and_decodes_entities() {
        let html = r#"<html><head><title>Rust &amp; Web</title></head>
            <body>
              <script>alert("x")</script>
              <style>.a{}</style>
              <noscript>no</noscript>
              <iframe src="evil"></iframe>
              <p>Hello&nbsp;world &lt;b&gt;bold&lt;/b&gt;</p>
              <svg><text>shapes</text></svg>
            </body></html>"#;
        let (title, text) = extract_html(html);
        assert_eq!(title.as_deref(), Some("Rust & Web"));
        assert!(text.contains("Hello world <b>bold</b>"));
        assert!(!text.contains("alert"));
        assert!(!text.contains("shapes"));
        assert!(!text.contains("noscript"));
        assert!(!text.contains("iframe"));
    }

    #[test]
    fn html_extraction_handles_unclosed_active_blocks_and_comments() {
        let html = "<html><body><script>never closed <p>text</body></html>";
        let (_, text) = extract_html(html);
        assert!(!text.contains("never closed"));
        let commented = "<!-- hidden --><p>visible</p><!-- trailing";
        let (_, text) = extract_html(commented);
        assert!(text.contains("visible"));
        assert!(!text.contains("hidden"));
    }

    #[test]
    fn content_type_rejection_is_runtime_recoverable_not_security_blocked() {
        let fetcher = WebFetcher::with_deps(Box::new(FakeTransport), Box::new(FakeResolver));
        // 内容类型在 fake 传输里按 URL 区分；拒绝是运行时失败（模型可诚实降级），
        // 不是 NetworkBlocked（安全拒绝只留给 SSRF/私网/凭据 URL）。
        let rejected = fetcher
            .fetch("https://evil.example.com/binary.png")
            .unwrap_err();
        assert_eq!(rejected.kind, AgentErrorKind::ToolExecutionFailed);
        assert!(rejected.message.contains("内容类型"));
    }

    #[test]
    fn fetch_runtime_failures_are_tool_execution_failed() {
        // 非 200、传输错误、DNS 失败、重定向超限都归 ToolExecutionFailed（可恢复）。
        let http_error = with_transport(|url, _| {
            if url == "https://a.example.com/missing" {
                Ok(FetchResponse {
                    status: 404,
                    location: None,
                    content_type: Some("text/html".to_string()),
                    body: Vec::new(),
                    truncated: false,
                })
            } else {
                Ok(FetchResponse {
                    status: 200,
                    location: None,
                    content_type: Some("text/plain".to_string()),
                    body: b"ok".to_vec(),
                    truncated: false,
                })
            }
        });
        let error = http_error
            .fetch("https://a.example.com/missing")
            .unwrap_err();
        assert_eq!(error.kind, AgentErrorKind::ToolExecutionFailed);
        assert!(error.message.contains("404"));

        let transport_error = with_transport(|_, _| Err("connection reset".to_string()));
        let error = transport_error
            .fetch("https://a.example.com/any")
            .unwrap_err();
        assert_eq!(error.kind, AgentErrorKind::ToolExecutionFailed);

        struct FailingResolver;
        impl DnsResolver for FailingResolver {
            fn resolve(&self, host: &str, _port: u16) -> Result<Vec<SocketAddr>, String> {
                Err(format!("no such host {host}"))
            }
        }
        let dns_error = WebFetcher::with_deps(Box::new(FakeTransport), Box::new(FailingResolver));
        let error = dns_error
            .fetch("https://no-such-host.example/")
            .unwrap_err();
        assert_eq!(error.kind, AgentErrorKind::ToolExecutionFailed);
        assert!(error.message.contains("无法解析"));
    }

    #[test]
    fn security_rejections_stay_network_blocked() {
        let fetcher = WebFetcher::with_deps(Box::new(FakeTransport), Box::new(FakeResolver));
        // SSRF/凭据 URL 保持安全拒绝；重定向到非 HTTP(S) 协议同样按安全拒绝。
        for url in [
            "http://127.0.0.1/admin",
            "http://169.254.169.254/latest/meta-data",
            "http://10.1.2.3/",
            "https://user:pass@example.com/",
            "https://example.com/?api_key=secret",
        ] {
            let error = fetcher.fetch(url).unwrap_err();
            assert_eq!(error.kind, AgentErrorKind::NetworkBlocked, "url={url}");
        }
        let redirect_to_ftp = with_transport(|_, _| {
            Ok(FetchResponse {
                status: 301,
                location: Some("ftp://example.com/file".to_string()),
                content_type: None,
                body: Vec::new(),
                truncated: false,
            })
        });
        let error = redirect_to_ftp
            .fetch("https://a.example.com/start")
            .unwrap_err();
        assert_eq!(error.kind, AgentErrorKind::NetworkBlocked);
        assert!(error.message.contains("协议"));
    }

    #[test]
    fn fetcher_rejects_ssrf_targets_before_any_request() {
        let fetcher = WebFetcher::with_deps(Box::new(FakeTransport), Box::new(FakeResolver));
        for url in [
            "http://127.0.0.1:8000/admin",
            "http://169.254.169.254/latest/meta-data",
            "http://192.168.1.1/",
            "http://localhost:8080/",
        ] {
            let error = fetcher.fetch(url).unwrap_err();
            assert_eq!(error.kind, AgentErrorKind::NetworkBlocked, "url={url}");
        }
    }

    #[test]
    fn fetcher_blocks_dns_rebinding_mixed_resolution() {
        // resolver 返回"公网 + 私网"混合地址（模拟 DNS rebinding）：私网地址
        // 被独立防御过滤，只连接公网 IP；全部私网时直接拒绝。
        struct MixedResolver;
        impl DnsResolver for MixedResolver {
            fn resolve(&self, _host: &str, port: u16) -> Result<Vec<SocketAddr>, String> {
                Ok(vec![
                    SocketAddr::new(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)), port),
                    SocketAddr::new(IpAddr::V4(Ipv4Addr::new(93, 184, 216, 34)), port),
                ])
            }
        }
        struct PrivateOnlyResolver;
        impl DnsResolver for PrivateOnlyResolver {
            fn resolve(&self, _host: &str, port: u16) -> Result<Vec<SocketAddr>, String> {
                Ok(vec![SocketAddr::new(
                    IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1)),
                    port,
                )])
            }
        }

        let recording = std::sync::Arc::new(std::sync::Mutex::new(None));
        struct RecordingTransport(std::sync::Arc<std::sync::Mutex<Option<SocketAddr>>>);
        impl FetchTransport for RecordingTransport {
            fn get(
                &self,
                url: &str,
                fixed_addr: Option<SocketAddr>,
            ) -> BoxFuture<'static, Result<FetchResponse, String>> {
                *self.0.lock().unwrap() = fixed_addr;
                let url = url.to_string();
                Box::pin(async move { fake_response_from_url(&url) })
            }
        }
        let fetcher = WebFetcher::with_deps(
            Box::new(RecordingTransport(recording.clone())),
            Box::new(MixedResolver),
        );
        let outcome = fetcher.fetch("https://rebind.example.com/page").unwrap();
        assert!(outcome.text.contains("fake body"));
        let fixed_addr = *recording.lock().unwrap();
        assert_eq!(
            fixed_addr.map(|addr| addr.ip()),
            Some(IpAddr::V4(Ipv4Addr::new(93, 184, 216, 34))),
            "只允许连接已验证的公网 IP"
        );
        drop(outcome);

        let fetcher = WebFetcher::with_deps(
            Box::new(RecordingTransport(recording)),
            Box::new(PrivateOnlyResolver),
        );
        let error = fetcher
            .fetch("https://rebind.example.com/page")
            .unwrap_err();
        assert_eq!(error.kind, AgentErrorKind::NetworkBlocked);
        assert!(error.message.contains("保留网段"));
    }

    #[test]
    fn fetcher_follows_redirects_with_revalidation() {
        let fetcher = with_transport(|url, _| {
            if url == "https://a.example.com/start" {
                Ok(FetchResponse {
                    status: 302,
                    location: Some("https://b.example.com/landing".to_string()),
                    content_type: None,
                    body: Vec::new(),
                    truncated: false,
                })
            } else {
                Ok(FetchResponse {
                    status: 200,
                    location: None,
                    content_type: Some("text/html".to_string()),
                    body: b"<html><title>Landed</title><p>final text</p></html>".to_vec(),
                    truncated: false,
                })
            }
        });
        let outcome = fetcher.fetch("https://a.example.com/start").unwrap();
        assert_eq!(outcome.canonical_url, "https://b.example.com/landing");
        assert_eq!(outcome.title.as_deref(), Some("Landed"));
        assert!(outcome.text.contains("final text"));
    }

    #[test]
    fn fetcher_caps_redirect_count_and_body_size() {
        let fetcher = with_transport(|url, _| {
            if url.starts_with("https://a.example.com/") {
                let hops = url
                    .split('/')
                    .nth(3)
                    .unwrap_or("0")
                    .parse::<u32>()
                    .unwrap_or(0);
                Ok(FetchResponse {
                    status: 301,
                    location: Some(format!("https://a.example.com/hop-{}", hops + 1)),
                    content_type: None,
                    body: Vec::new(),
                    truncated: false,
                })
            } else {
                Ok(FetchResponse {
                    status: 200,
                    location: None,
                    content_type: Some("text/plain".to_string()),
                    body: vec![b'x'; 3 * 1024 * 1024],
                    truncated: true,
                })
            }
        });
        let redirect_error = fetcher.fetch("https://a.example.com/hop-0").unwrap_err();
        assert!(redirect_error.message.contains("重定向次数"));
        let body_outcome = fetcher.fetch("https://b.example.com/body").unwrap();
        assert!(body_outcome.truncated);
        assert_eq!(body_outcome.text.len(), 2 * 1024 * 1024);
    }

    #[test]
    fn wikipedia_search_parses_results_and_builds_details() {
        let provider = WikipediaSearchProvider;
        let items = provider
            .parse_response(
                r#"{"query":{"search":[
                    {"title":"Rust (programming language)","snippet":"A <span class=\"searchmatch\">systems</span> language","pageid":1},
                    {"title":"Rust (fungus)","snippet":"A plant disease","pageid":2}
                ]}}"#,
                "en",
            )
            .expect("合法响应必须解析成功");
        assert_eq!(items.len(), 2);
        assert_eq!(
            items[0].url,
            "https://en.wikipedia.org/wiki/Rust_(programming_language)"
        );
        assert_eq!(items[0].snippet, "A systems language");
        let details = search_details_sources(&items, "en", 123);
        let sources = sources_from_details(&Some(details));
        assert_eq!(sources.len(), 2);
        assert_eq!(sources[0].site_name.as_deref(), Some("Wikipedia (en)"));
        assert!(!sources[0].source_id.contains("wikipedia"));
    }

    #[test]
    fn wikipedia_parse_failure_is_distinct_from_no_results() {
        let provider = WikipediaSearchProvider;
        // JSON 非法 → 解析失败（ToolExecutionFailed），不是"没有匹配条目"。
        let invalid = provider.parse_response("{not-json", "en").unwrap_err();
        assert_eq!(invalid.kind, AgentErrorKind::ToolExecutionFailed);
        assert!(invalid.message.contains("不是合法 JSON"));
        // 缺少 query.search 列表（服务/协议错误）→ 解析失败。
        let missing = provider
            .parse_response(r#"{"error":{"code":"badquery"}}"#, "en")
            .unwrap_err();
        assert_eq!(missing.kind, AgentErrorKind::ToolExecutionFailed);
        assert!(missing.message.contains("缺少结果列表"));
        // 真正无结果：空 search 数组 → Ok(vec![])，由 format_search_content
        // 展示"没有找到匹配条目"。
        let empty = provider
            .parse_response(r#"{"query":{"search":[]}}"#, "en")
            .expect("空结果列表是合法无结果");
        assert!(empty.is_empty());
        let content = format_search_content(&empty, "en");
        assert!(content.contains("没有找到匹配条目"));
    }

    #[test]
    fn search_content_honestly_notes_wikipedia_coverage() {
        let items = vec![SearchResultItem {
            title: "Rust".to_string(),
            url: "https://en.wikipedia.org/wiki/Rust".to_string(),
            snippet: "Systems language".to_string(),
        }];
        let content = format_search_content(&items, "en");
        assert!(content.contains("Rust"));
        assert!(content.contains("非通用搜索"));
        let empty = format_search_content(&[], "zh");
        assert!(empty.contains("没有找到匹配条目"));
        assert!(empty.contains("不得把模型记忆中的信息冒充为已核实事实"));
    }

    #[test]
    fn sources_from_details_skips_invalid_entries() {
        let details = json!({
            "sources": [
                {"sourceId": "s1", "title": "OK", "url": "https://example.com/a", "retrievedAtUnixMs": 1},
                {"sourceId": "s2", "title": "Bad", "url": "ftp://not-http", "retrievedAtUnixMs": 1}
            ]
        });
        let sources = sources_from_details(&Some(details));
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].url, "https://example.com/a");
        assert!(sources_from_details(&None).is_empty());
    }

    // ---- fake 传输 ----

    #[derive(Clone)]
    struct FakeTransport;

    impl Default for FakeTransport {
        fn default() -> Self {
            Self
        }
    }

    /// 测试 DNS：固定解析到公网 IP（避免真实 DNS 依赖与保留网段拦截）。
    struct FakeResolver;

    impl DnsResolver for FakeResolver {
        fn resolve(&self, _host: &str, port: u16) -> Result<Vec<SocketAddr>, String> {
            Ok(vec![SocketAddr::new(
                IpAddr::V4(Ipv4Addr::new(93, 184, 216, 34)),
                port,
            )])
        }
    }

    fn fake_response_from_url(url: &str) -> Result<FetchResponse, String> {
        if url.contains("binary.png") {
            return Ok(FetchResponse {
                status: 200,
                location: None,
                content_type: Some("image/png".to_string()),
                body: vec![0x89, 0x50, 0x4e, 0x47],
                truncated: false,
            });
        }
        if url.starts_with("http://") || url.starts_with("https://") {
            return Ok(FetchResponse {
                status: 200,
                location: None,
                content_type: Some("text/html".to_string()),
                body: b"<html><title>Fake</title><p>fake body</p></html>".to_vec(),
                truncated: false,
            });
        }
        Err("unsupported".to_string())
    }

    impl FetchTransport for FakeTransport {
        fn get(
            &self,
            url: &str,
            _fixed_addr: Option<SocketAddr>,
        ) -> BoxFuture<'static, Result<FetchResponse, String>> {
            let url = url.to_string();
            Box::pin(async move { fake_response_from_url(&url) })
        }
    }

    fn with_transport<F>(handler: F) -> WebFetcher
    where
        F: Fn(&str, Option<SocketAddr>) -> Result<FetchResponse, String> + Send + Sync + 'static,
    {
        struct HandlerTransport<F>(Arc<F>);
        impl<F> FetchTransport for HandlerTransport<F>
        where
            F: Fn(&str, Option<SocketAddr>) -> Result<FetchResponse, String>
                + Send
                + Sync
                + 'static,
        {
            fn get(
                &self,
                url: &str,
                fixed_addr: Option<SocketAddr>,
            ) -> BoxFuture<'static, Result<FetchResponse, String>> {
                let url = url.to_string();
                let handler = Arc::clone(&self.0);
                Box::pin(async move { handler(&url, fixed_addr) })
            }
        }
        WebFetcher::with_deps(
            Box::new(HandlerTransport(Arc::new(handler))),
            Box::new(FakeResolver),
        )
    }
}
