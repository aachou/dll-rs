use anyhow::Context;
use regex::Regex;
use std::time::Duration;

use crate::cli::{Architecture, Config};
use crate::installer;

const BASE_URL: &str = "https://cn.dll-files.com";
const USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) dll-rs/0.2.0";
const TIMEOUT_SECS: u64 = 30;
const MAX_RETRIES: u32 = 3;

pub fn fetch_page(url: &str, proxy: Option<&str>) -> anyhow::Result<minreq::Response> {
    let mut req = minreq::get(url)
        .with_header("User-Agent", USER_AGENT)
        .with_timeout(TIMEOUT_SECS);
    if let Some(p) = proxy {
        req = req.with_proxy(minreq::Proxy::new(p).context("无效代理地址")?);
    }
    req.send().context("发送请求失败")
}

pub fn fetch_page_with_retry(
    url: &str,
    proxy: Option<&str>,
    verbose: bool,
) -> anyhow::Result<minreq::Response> {
    let mut last_err = None;
    for attempt in 0..=MAX_RETRIES {
        if attempt > 0 {
            let delay = Duration::from_millis(500 * 2u64.pow(attempt - 1));
            if verbose {
                eprintln!("  第 {} 次重试 ({:?})...", attempt, delay);
            }
            std::thread::sleep(delay);
        }
        match fetch_page(url, proxy) {
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

fn get_download_url(downpage_url: &str, proxy: Option<&str>) -> anyhow::Result<String> {
    let resp = fetch_page(downpage_url, proxy)?;
    let html = resp.as_str().context("读取响应失败")?;

    let url_re = Regex::new(r#"downloadUrl\s*=\s*"(?<link>.+?)";"#)?;
    match url_re.captures(html).and_then(|m| m.name("link")) {
        Some(m) => {
            let url = m.as_str().replace("amp;", "").replace("&#038;", "");
            Ok(url)
        }
        None => anyhow::bail!("未找到下载链接"),
    }
}

pub fn search_dll(query: &str, proxy: Option<&str>) -> anyhow::Result<Vec<String>> {
    let url = format!("{}/search?q={}", BASE_URL, query);
    let resp = fetch_page(&url, proxy)?;
    let html = resp.as_str().context("读取搜索结果失败")?;

    let re = Regex::new(r#"(?i)<a\s+href="/([a-z0-9_.-]+\.html)"[^>]*>"#)?;
    let mut results: Vec<String> = Vec::new();
    for cap in re.captures_iter(html) {
        let name = cap[1].trim_end_matches(".html");
        if name.ends_with(".dll") && !results.contains(&name.to_string()) {
            results.push(name.to_string());
        }
    }

    if results.is_empty() {
        let fallback_re = Regex::new(r#"(?i) ([a-z0-9_.-]+\.dll)"#)?;
        for cap in fallback_re.captures_iter(html) {
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

    pub fn process(&self) -> anyhow::Result<(bool, bool)> {
        println!("正在查询 {} 的下载信息", self.name);
        let proxy = self.config.proxy.as_deref();
        let verbose = self.config.verbose;

        let resp = fetch_page_with_retry(
            &format!("{}/{}.html", BASE_URL, self.name),
            proxy,
            verbose,
        )?;
        let html = resp.as_str().context("读取响应失败")?;

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

        for section in section_re.find_iter(html).map(|m| m.as_str()) {
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
            let full_url = format!("{}{}", BASE_URL, link);
            match arch {
                "32" => x32_url = full_url,
                "64" => x64_url = full_url,
                _ => {}
            }
        }

        Ok(std::thread::scope(|s| {
            let x32 = s.spawn(|| self.install_arch(Architecture::X32, &x32_url));
            let x64 = s.spawn(|| self.install_arch(Architecture::X64, &x64_url));
            (x32.join().unwrap(), x64.join().unwrap())
        }))
    }

    fn install_arch(&self, arch: Architecture, page_url: &str) -> bool {
        let verbose = self.config.verbose;
        if page_url.is_empty() {
            if verbose {
                println!("  未找到 {} 版本下载页面", arch.name());
            }
            return false;
        }

        let tag = format!("{} ({})", self.name, arch.name());
        if verbose {
            println!("  下载页面: {}", page_url);
        }

        let proxy = self.config.proxy.as_deref();
        let download_url = match get_download_url(page_url, proxy) {
            Ok(url) => url,
            Err(e) => {
                eprintln!("  获取 {} 下载链接失败: {}", tag, e);
                return false;
            }
        };

        if verbose {
            println!("  真实下载地址: {}", download_url);
        }

        println!("  正在下载 {}", tag);
        let resp = match fetch_page_with_retry(&download_url, proxy, verbose) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("  下载 {} 失败: {}", tag, e);
                return false;
            }
        };

        let data = resp.as_bytes();
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
            return true;
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

        match installer::extract_and_write(&self.name, data, &dest_path, verbose) {
            Ok(_) => {
                println!("  {} 安装成功", tag);
                true
            }
            Err(e) => {
                eprintln!("  {} 安装失败: {}", tag, e);
                false
            }
        }
    }
}
