use anyhow::Context;
use indicatif::{ProgressBar, ProgressStyle};
use regex::Regex;
use reqwest::Client;
use tokio::time::{sleep, Duration};

use crate::cli::{Architecture, Config};
use crate::installer;

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

async fn download_zip_data(client: &Client, url: &str, verbose: bool) -> anyhow::Result<Vec<u8>> {
    let resp = client.get(url).send().await.context("下载请求失败")?;
    let total = resp.content_length().unwrap_or(0);

    let pb = if !verbose {
        if total > 0 {
            let pb = ProgressBar::new(total);
            pb.set_style(
                ProgressStyle::default_bar()
                    .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta})")
                    .unwrap()
                    .progress_chars("#>-"),
            );
            Some(pb)
        } else {
            let pb = ProgressBar::new_spinner();
            pb.set_style(
                ProgressStyle::default_spinner()
                    .template("{spinner:.green} 下载中...")
                    .unwrap(),
            );
            pb.enable_steady_tick(std::time::Duration::from_millis(100));
            Some(pb)
        }
    } else {
        None
    };

    let data = resp.bytes().await.context("读取响应数据失败")?.to_vec();

    if let Some(pb) = pb {
        if total > 0 {
            pb.inc(total);
        }
        pb.finish_and_clear();
    }

    Ok(data)
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
        println!("正在查询 {} 的下载信息", self.name);
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
            anyhow::bail!("未找到 {} 的下载页面", self.name);
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

        let (x32, x64) = tokio::join!(
            self.install_arch(&client, Architecture::X32, &x32_url),
            self.install_arch(&client, Architecture::X64, &x64_url),
        );

        let x32_ok = x32.unwrap_or_else(|e| {
            eprintln!("  {} (x86) 失败: {}", self.name, e);
            false
        });
        let x64_ok = x64.unwrap_or_else(|e| {
            eprintln!("  {} (x64) 失败: {}", self.name, e);
            false
        });
        Ok((x32_ok, x64_ok))
    }

    async fn install_arch(
        &self,
        client: &Client,
        arch: Architecture,
        page_url: &str,
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

        if verbose {
            println!("  真实下载地址: {}", download_url);
        }

        println!("  正在下载 {}", tag);
        let data = download_zip_data(client, &download_url, verbose)
            .await
            .with_context(|| format!("下载 {} 失败", tag))?;
        let size = data.len();
        println!("  下载完成 ({})", format_size(size));

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
            println!("  {} 已存在，跳过安装", self.name);
            return Ok(true);
        }

        if std::path::Path::new(&dest_path).exists() && self.config.force {
            match installer::backup_path(&dest_path) {
                Ok(backup) => match std::fs::rename(&dest_path, &backup) {
                    Ok(_) => println!("  已备份原文件到 {}", backup.display()),
                    Err(e) => eprintln!("  备份失败: {}", e),
                },
                Err(e) => eprintln!("  备份失败: {}", e),
            }
        }

        installer::extract_and_write(&self.name, &data, &dest_path, verbose)
            .with_context(|| format!("{} 安装失败", tag))?;

        println!("  {} 安装成功", tag);
        Ok(true)
    }
}
