use anyhow::Context;
use regex::Regex;
use zip::read::ZipArchive;

use std::env;
use std::fs::File;
use std::io;
use std::io::{Cursor, Read};
use std::path::Path;


const BASE_URL: &str = "https://cn.dll-files.com";
const USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) dll-rs/0.2.0";
const TIMEOUT_SECS: u64 = 30;

const X32_SYSTEM_PATH: &str = r"C:\Windows\SysWOW64\";
const X64_SYSTEM_PATH: &str = r"C:\Windows\System32\";

struct Config {
    force: bool,
    dll_names: Vec<String>,
}

fn parse_args_from(args: &[String]) -> anyhow::Result<Config> {
    if args.len() < 2 {
        anyhow::bail!("用法: dll [选项] <name.dll> [name.dll ...]");
    }

    let mut force = false;
    let mut dll_names = Vec::new();

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "-f" | "--force" => force = true,
            s if s.starts_with('-') => anyhow::bail!("未知选项: {}", s),
            name => {
                let lower = name.to_lowercase();
                if !lower.ends_with(".dll") {
                    anyhow::bail!("参数必须以 .dll 结尾: {}", name);
                }
                dll_names.push(lower);
            }
        }
        i += 1;
    }

    if dll_names.is_empty() {
        anyhow::bail!("未指定 DLL 文件名");
    }

    Ok(Config { force, dll_names })
}

fn parse_args() -> anyhow::Result<Config> {
    let args: Vec<String> = env::args().collect();
    parse_args_from(&args)
}

#[derive(Clone, Copy)]
enum Architecture {
    X32,
    X64,
}

impl Architecture {
    fn name(self) -> &'static str {
        match self {
            Architecture::X32 => "x86",
            Architecture::X64 => "x64",
        }
    }

    fn system_path(self) -> &'static str {
        match self {
            Architecture::X32 => X32_SYSTEM_PATH,
            Architecture::X64 => X64_SYSTEM_PATH,
        }
    }


}

struct Dll<'a> {
    name: String,
    config: &'a Config,
}

impl Dll<'_> {
    fn new(name: String, config: &Config) -> Dll<'_> {
        Dll { name, config }
    }

    fn process(&self) -> anyhow::Result<(bool, bool)> {
        println!("正在查询 {} 的下载信息", self.name);
        let resp = self.fetch_page(&format!("{BASE_URL}/{}.html", self.name))?;
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

            let full_url = format!("{BASE_URL}{link}");
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
        if page_url.is_empty() {
            println!("未找到 {} 版本下载页面", arch.name());
            return false;
        }

        let tag = format!("{} ({})", self.name, arch.name());
        println!("正在获取 {} 的下载链接", tag);

        let download_url = match self.get_download_url(page_url) {
            Ok(url) => url,
            Err(e) => {
                eprintln!("获取 {} 下载链接失败: {}", tag, e);
                return false;
            }
        };

        println!("正在下载 {}", tag);
        match self.install_dll(&download_url, arch) {
            Ok(()) => {
                println!("{} 安装成功", tag);
                true
            }
            Err(e) => {
                eprintln!("{} 安装失败: {}", tag, e);
                false
            }
        }
    }

    fn fetch_page(&self, url: &str) -> anyhow::Result<minreq::Response> {
        minreq::get(url)
            .with_header("User-Agent", USER_AGENT)
            .with_timeout(TIMEOUT_SECS)
            .send()
            .context("发送请求失败")
    }

    fn get_download_url(&self, downpage_url: &str) -> anyhow::Result<String> {
        let resp = self.fetch_page(downpage_url)?;
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

    fn install_dll(&self, download_url: &str, arch: Architecture) -> anyhow::Result<()> {
        let resp = self.fetch_page(download_url)?;
        let cursor = Cursor::new(resp.as_bytes());
        let mut archive = ZipArchive::new(cursor).context("解压 ZIP 失败")?;

        let sys_path = arch.system_path();
        let dll_file_path = format!("{}{}", sys_path, self.name);

        if !self.config.force && Path::new(&dll_file_path).exists() {
            println!("{} 已存在，跳过安装", self.name);
            return Ok(());
        }

        let mut extracted = false;
        for i in 0..archive.len() {
            let mut file = archive.by_index(i).context("读取 ZIP 条目失败")?;
            if !file.name().ends_with(".dll") {
                continue;
            }
            if !file.name().to_lowercase().ends_with(&self.name.to_lowercase()) {
                continue;
            }

            let mut dll_file = File::create(&dll_file_path).context("创建文件失败")?;
            if let Err(e) = io::copy(&mut file, &mut dll_file) {
                let _ = std::fs::remove_file(&dll_file_path);
                anyhow::bail!("写入文件失败: {}", e);
            }
            drop(dll_file);

            if !is_valid_pe(&dll_file_path) {
                let _ = std::fs::remove_file(&dll_file_path);
                anyhow::bail!("下载的文件不是有效的 PE 格式");
            }

            extracted = true;
            break;
        }

        if !extracted {
            anyhow::bail!("ZIP 中未找到与 {} 匹配的 DLL 文件", self.name);
        }

        Ok(())
    }
}

fn is_valid_pe(path: &str) -> bool {
    let mut buf = [0u8; 2];
    if let Ok(mut f) = File::open(path) {
        if f.read_exact(&mut buf).is_ok() {
            return buf == [b'M', b'Z'];
        }
    }
    false
}

fn main() -> anyhow::Result<()> {
    let config = parse_args()?;

    for name in &config.dll_names {
        println!("━━━ {} ━━━", name);
        let dll = Dll::new(name.clone(), &config);
        match dll.process() {
            Ok((x32_ok, x64_ok)) => {
                let status = match (x32_ok, x64_ok) {
                    (true, true) => "全部成功",
                    (false, false) => "全部失败",
                    (true, false) => "仅 x86 成功",
                    (false, true) => "仅 x64 成功",
                };
                println!("结果: {} —— {}", name, status);
            }
            Err(e) => {
                eprintln!("{} 处理失败: {}", name, e);
            }
        }
        println!();
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- parse_args_from ----

    #[test]
    fn parse_single_dll() {
        let args = &["dll".to_string(), "dxgi.dll".to_string()];
        let cfg = parse_args_from(args).unwrap();
        assert!(!cfg.force);
        assert_eq!(cfg.dll_names, vec!["dxgi.dll"]);
    }

    #[test]
    fn parse_multiple_dlls() {
        let args = &[
            "dll".to_string(),
            "dxgi.dll".to_string(),
            "d3dcompiler.dll".to_string(),
        ];
        let cfg = parse_args_from(args).unwrap();
        assert!(!cfg.force);
        assert_eq!(cfg.dll_names, vec!["dxgi.dll", "d3dcompiler.dll"]);
    }

    #[test]
    fn parse_force_flag() {
        let args = &["dll".to_string(), "-f".to_string(), "dxgi.dll".to_string()];
        let cfg = parse_args_from(args).unwrap();
        assert!(cfg.force);
        assert_eq!(cfg.dll_names, vec!["dxgi.dll"]);
    }

    #[test]
    fn parse_force_long_flag() {
        let args = &[
            "dll".to_string(),
            "--force".to_string(),
            "dxgi.dll".to_string(),
        ];
        let cfg = parse_args_from(args).unwrap();
        assert!(cfg.force);
        assert_eq!(cfg.dll_names, vec!["dxgi.dll"]);
    }

    #[test]
    fn parse_no_args() {
        let args = &["dll".to_string()];
        assert!(parse_args_from(args).is_err());
    }

    #[test]
    fn parse_no_dll_extension() {
        let args = &["dll".to_string(), "dxgi".to_string()];
        assert!(parse_args_from(args).is_err());
    }

    #[test]
    fn parse_unknown_flag() {
        let args = &["dll".to_string(), "--unknown".to_string(), "dxgi.dll".to_string()];
        assert!(parse_args_from(args).is_err());
    }

    #[test]
    fn parse_lowercases_name() {
        let args = &["dll".to_string(), "DXGI.DLL".to_string()];
        let cfg = parse_args_from(args).unwrap();
        assert_eq!(cfg.dll_names, vec!["dxgi.dll"]);
    }

    #[test]
    fn parse_force_between_dlls() {
        let args = &[
            "dll".to_string(),
            "a.dll".to_string(),
            "-f".to_string(),
            "b.dll".to_string(),
        ];
        let cfg = parse_args_from(args).unwrap();
        assert!(cfg.force);
        assert_eq!(cfg.dll_names, vec!["a.dll", "b.dll"]);
    }

    // ---- Architecture ----

    #[test]
    fn arch_x32_name() {
        assert_eq!(Architecture::X32.name(), "x86");
    }

    #[test]
    fn arch_x64_name() {
        assert_eq!(Architecture::X64.name(), "x64");
    }

    #[test]
    fn arch_x32_system_path() {
        assert_eq!(Architecture::X32.system_path(), r"C:\Windows\SysWOW64\");
    }

    #[test]
    fn arch_x64_system_path() {
        assert_eq!(Architecture::X64.system_path(), r"C:\Windows\System32\");
    }

    // ---- is_valid_pe ----

    #[test]
    fn pe_valid_mz_header() {
        let dir = std::env::temp_dir().join("dll-rs-test");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("test.dll");
        std::fs::write(&path, b"MZ\x90\x00\x03\x00\x00\x00\x04\x00\x00\x00\xff\xff\x00\x00").unwrap();
        assert!(is_valid_pe(path.to_str().unwrap()));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn pe_invalid_no_mz() {
        let dir = std::env::temp_dir().join("dll-rs-test");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("notadll.dll");
        std::fs::write(&path, b"\x00\x00\x00\x00").unwrap();
        assert!(!is_valid_pe(path.to_str().unwrap()));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn pe_empty_file() {
        let dir = std::env::temp_dir().join("dll-rs-test");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("empty.dll");
        std::fs::write(&path, b"").unwrap();
        assert!(!is_valid_pe(path.to_str().unwrap()));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn pe_nonexistent_file() {
        assert!(!is_valid_pe(r"C:\dll-rs-test-nonexistent.dll"));
    }
}
