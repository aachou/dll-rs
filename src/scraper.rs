use anyhow::Context;
use indicatif::{ProgressBar, ProgressStyle};
use regex::Regex;
use reqwest::Client;
use tokio::time::{sleep, Duration};

use crate::cli::{Architecture, Config};
use crate::installer;

const C_GREEN: &str = "\x1b[32m";
const C_RED: &str = "\x1b[31m";
const C_YELLOW: &str = "\x1b[33m";
const C_CYAN: &str = "\x1b[36m";

const C_RESET: &str = "\x1b[0m";

fn ok(msg: impl std::fmt::Display) -> String {
    format!("{C_GREEN}✓{C_RESET} {msg}")
}
fn err(msg: impl std::fmt::Display) -> String {
    format!("{C_RED}✗{C_RESET} {msg}")
}
fn info(msg: impl std::fmt::Display) -> String {
    format!("{C_CYAN}→{C_RESET} {msg}")
}
fn warn(msg: impl std::fmt::Display) -> String {
    format!("{C_YELLOW}⚠{C_RESET} {msg}")
}

fn short_path(p: &std::path::Path) -> String {
    let s = p.to_string_lossy();
    let temp = std::env::temp_dir().to_string_lossy().to_string();
    s.replace(&temp, "%TEMP%")
}

fn base_url() -> String {
    std::env::var("DLL_RS_BASE_URL").unwrap_or_else(|_| "https://cn.dll-files.com".to_string())
}

const USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) dll-rs/0.2.0";
const TIMEOUT_SECS: u64 = 30;
const MAX_RETRIES: u32 = 3;

pub fn build_client(proxy: Option<&str>) -> anyhow::Result<Client> {
    let mut builder = Client::builder()
        .timeout(Duration::from_secs(TIMEOUT_SECS))
        .user_agent(USER_AGENT);
    if let Some(p) = proxy {
        let proxy = reqwest::Proxy::all(p).context("无效代理地址")?;
        builder = builder.proxy(proxy);
    }
    builder.build().context("创建 HTTP 客户端失败")
}

pub async fn fetch_page(client: &Client, url: &str) -> anyhow::Result<String> {
    let resp = client.get(url).send().await.context("发送请求失败")?;
    resp.text().await.context("读取响应失败")
}

pub async fn fetch_page_with_retry(
    client: &Client,
    url: &str,
    verbose: bool,
) -> anyhow::Result<String> {
    let mut last_err = None;
    for attempt in 0..=MAX_RETRIES {
        if attempt > 0 {
            let delay = Duration::from_millis(500 * 2u64.pow(attempt - 1));
            if verbose {
                eprintln!("  第 {} 次重试 ({:?})...", attempt, delay);
            }
            sleep(delay).await;
        }
        match fetch_page(client, url).await {
            ok @ Ok(_) => return ok,
            Err(e) => {
                last_err = Some(e);
                if verbose {
                    eprintln!("  请求失败: {}", last_err.as_ref().unwrap());
                }
            }
        }
    }
    Err(last_err.unwrap())
}

const BROWSER_UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";

fn make_spinner() -> ProgressBar {
    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.green} 下载中...")
            .unwrap(),
    );
    pb.enable_steady_tick(Duration::from_millis(100));
    pb
}

fn validate_zip(data: &[u8], verbose: bool) -> anyhow::Result<()> {
    if data.len() < 4 || &data[..4] != b"PK\x03\x04" {
        let preview = if data.is_empty() {
            "空文件".to_string()
        } else {
            let first = &data[..data.len().min(128)];
            String::from_utf8_lossy(first).to_string()
        };
        anyhow::bail!(
            "下载的内容不是有效的 ZIP 文件 (前 {} 字节: {:?})",
            data.len().min(128),
            preview
        );
    }
    if verbose {
        println!("  ZIP 校验通过");
    }
    Ok(())
}

async fn try_download(client: &Client, url: &str, referer: &str) -> anyhow::Result<Vec<u8>> {
    let resp = client
        .get(url)
        .header("Referer", referer)
        .header("Origin", "https://cn.dll-files.com")
        .header(
            "Accept",
            "text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,*/*;q=0.8",
        )
        .header("Accept-Language", "zh-CN,zh;q=0.9,en;q=0.8")
        .header("Sec-Fetch-Dest", "document")
        .header("Sec-Fetch-Mode", "navigate")
        .header("Sec-Fetch-Site", "same-origin")
        .send()
        .await
        .context("下载请求失败")?;
    let status = resp.status();
    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        anyhow::bail!(
            "服务器返回 {} (前 256 字节: {})",
            status,
            &text[..text.len().min(256)]
        );
    }
    resp.bytes()
        .await
        .map(|b| b.to_vec())
        .context("读取响应数据失败")
}

async fn try_download_http1(url: &str, referer: &str) -> anyhow::Result<Vec<u8>> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(TIMEOUT_SECS))
        .user_agent(BROWSER_UA)
        .http1_only()
        .build()?;
    try_download(&client, url, referer).await
}

async fn download_zip_data(
    client: &Client,
    url: &str,
    referers: &[&str],
    verbose: bool,
) -> anyhow::Result<Vec<u8>> {
    if verbose {
        println!("  URL: {}", url);
    }
    let spinner = if verbose { None } else { Some(make_spinner()) };

    let mut last_err = None;
    for &referer in referers {
        match try_download(client, url, referer).await {
            Ok(data) => {
                validate_zip(&data, verbose)?;
                if let Some(ref pb) = spinner {
                    pb.finish_and_clear();
                }
                return Ok(data);
            }
            Err(e) => {
                if verbose {
                    eprintln!("  Referer '{}': {}", referer, e);
                }
                last_err = Some(e);
            }
        }
    }

    for &referer in referers {
        match try_download_http1(url, referer).await {
            Ok(data) => {
                validate_zip(&data, verbose)?;
                if let Some(ref pb) = spinner {
                    pb.finish_and_clear();
                }
                return Ok(data);
            }
            Err(e) => {
                if verbose {
                    eprintln!("  HTTP/1.1 Referer '{}': {}", referer, e);
                }
                last_err = Some(e);
            }
        }
    }

    if let Some(ref pb) = spinner {
        pb.finish_and_clear();
    }
    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("所有下载策略均失败")))
}

async fn get_download_url(client: &Client, downpage_url: &str) -> anyhow::Result<String> {
    let html = fetch_page(client, downpage_url).await?;

    let url_re = Regex::new(r#"downloadUrl\s*=\s*"(?<link>.+?)";"#)?;
    match url_re.captures(&html).and_then(|m| m.name("link")) {
        Some(m) => {
            let url = m.as_str().replace("amp;", "").replace("&#038;", "");
            Ok(url)
        }
        None => anyhow::bail!("未找到下载链接"),
    }
}

pub async fn search_dll(client: &Client, query: &str) -> anyhow::Result<Vec<String>> {
    let url = format!("{}/search?q={}", base_url(), query);
    let html = fetch_page(client, &url).await?;

    let re = Regex::new(r#"(?i)<a\s+href="/([a-z0-9_.-]+\.html)"[^>]*>"#)?;
    let mut results: Vec<String> = Vec::new();
    for cap in re.captures_iter(&html) {
        let name = cap[1].trim_end_matches(".html");
        if name.ends_with(".dll") && !results.contains(&name.to_string()) {
            results.push(name.to_string());
        }
    }

    if results.is_empty() {
        let fallback_re = Regex::new(r#"(?i) ([a-z0-9_.-]+\.dll)"#)?;
        for cap in fallback_re.captures_iter(&html) {
            let n = cap[1].to_lowercase();
            if !results.contains(&n) {
                results.push(n);
            }
        }
    }

    results.sort();
    results.dedup();
    Ok(results)
}

fn format_size(bytes: usize) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else if bytes >= 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{} B", bytes)
    }
}

pub struct Dll<'a> {
    pub name: String,
    pub config: &'a Config,
}

impl Dll<'_> {
    pub fn new(name: String, config: &Config) -> Dll<'_> {
        Dll { name, config }
    }

    pub async fn process(&self) -> anyhow::Result<(bool, bool)> {
        println!("  {}", info(format!("正在查询 {} 的下载信息", self.name)));
        let proxy = self.config.proxy.as_deref();
        let verbose = self.config.verbose;

        let client = build_client(proxy)?;

        let html = fetch_page_with_retry(
            &client,
            &format!("{}/{}.html", base_url(), self.name),
            verbose,
        )
        .await?;

        if html.contains("error-404") {
            anyhow::bail!(
                "未找到 {} 的下载页面，请尝试使用 --search 搜索正确的文件名",
                self.name
            );
        }

        let section_re = Regex::new(r#"(?s)<section class="file-info-grid".+?</section>"#)
            .context("编译 section 正则失败")?;
        let meta_re = Regex::new(r#"(?s)<div\sclass="right-pane".+?</div>"#)?;
        let arch_re = Regex::new(r#"(?s)<p>(?<arch>.+?)</p>"#)?;
        let link_re = Regex::new(r#"(?s)<a href="(?<link>.+?)"\sdata-ga-action"#)?;

        let mut x32_url = String::new();
        let mut x64_url = String::new();

        for section in section_re.find_iter(&html).map(|m| m.as_str()) {
            if !x32_url.is_empty() && !x64_url.is_empty() {
                break;
            }
            let meta_info = match meta_re.find(section) {
                Some(m) => m.as_str(),
                None => continue,
            };
            let arch = arch_re
                .captures_iter(meta_info)
                .filter_map(|m| m.name("arch"))
                .nth(1)
                .map(|m| m.as_str())
                .unwrap_or("");
            if arch.is_empty()
                || (arch == "32" && !x32_url.is_empty())
                || (arch == "64" && !x64_url.is_empty())
            {
                continue;
            }
            let link = match link_re.captures(section).and_then(|m| m.name("link")) {
                Some(m) => m.as_str(),
                None => continue,
            };
            let full_url = format!("{}{}", base_url(), link);
            match arch {
                "32" => x32_url = full_url,
                "64" => x64_url = full_url,
                _ => {}
            }
        }

        let main_page_url = format!("{}/{}.html", base_url(), self.name);
        let (x32, x64) = tokio::join!(
            self.install_arch(&client, Architecture::X32, &x32_url, &main_page_url),
            self.install_arch(&client, Architecture::X64, &x64_url, &main_page_url),
        );

        let x32_ok = x32.unwrap_or_else(|e| {
            eprintln!("  {}", err(format!("{} (x86) 失败", self.name)));
            for (i, line) in format!("{:#}", e).lines().enumerate() {
                eprintln!("    {:>2}. {}", i + 1, line);
            }
            false
        });
        let x64_ok = x64.unwrap_or_else(|e| {
            eprintln!("  {}", err(format!("{} (x64) 失败", self.name)));
            for (i, line) in format!("{:#}", e).lines().enumerate() {
                eprintln!("    {:>2}. {}", i + 1, line);
            }
            false
        });
        Ok((x32_ok, x64_ok))
    }

    async fn install_arch(
        &self,
        client: &Client,
        arch: Architecture,
        page_url: &str,
        main_page_url: &str,
    ) -> anyhow::Result<bool> {
        let verbose = self.config.verbose;
        if page_url.is_empty() {
            if verbose {
                println!("  未找到 {} 版本下载页面", arch.name());
            }
            return Ok(false);
        }

        let tag = format!("{} ({})", self.name, arch.name());
        if verbose {
            println!("  下载页面: {}", page_url);
        }

        let download_url = get_download_url(client, page_url)
            .await
            .with_context(|| format!("获取 {} 下载链接失败", tag))?;

        println!("  {}", info(format!("正在下载 {}", tag)));
        let referers = [page_url, main_page_url];
        let data = download_zip_data(client, &download_url, &referers, verbose)
            .await
            .with_context(|| {
                format!(
                    "下载 {} 失败。提示: 可尝试 --proxy http://127.0.0.1:7897",
                    tag
                )
            })?;
        println!(
            "  {}",
            ok(format!("{} 下载完成 ({})", tag, format_size(data.len())))
        );

        let dest_path = if let Some(ref out) = self.config.output_dir {
            format!("{}{}", out, self.name)
        } else {
            let sys_path = arch.path(self.config);
            format!("{}{}", sys_path, self.name)
        };

        if !self.config.force
            && self.config.output_dir.is_none()
            && std::path::Path::new(&dest_path).exists()
        {
            println!("  {}", info(format!("{} 已存在，跳过安装", self.name)));
            return Ok(true);
        }

        if std::path::Path::new(&dest_path).exists() && self.config.force {
            match installer::backup_path(&dest_path) {
                Ok(backup) => match std::fs::copy(&dest_path, &backup) {
                    Ok(_) => {
                        let short = short_path(&backup);
                        println!("  {}", ok(format!("已备份原文件到 {}", short)));
                        let _ = std::fs::remove_file(&dest_path);
                    }
                    Err(e) => eprintln!("  {}", warn(format!("备份失败: {}", e))),
                },
                Err(e) => eprintln!("  {}", warn(format!("备份失败: {}", e))),
            }
        }

        installer::extract_and_write(&self.name, &data, &dest_path, verbose, self.config.force)
            .with_context(|| format!("{} 安装失败", tag))?;

        println!("  {}", ok(format!("{} 安装成功", tag)));
        Ok(true)
    }
}
