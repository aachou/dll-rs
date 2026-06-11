use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::env;
use std::io::{self, Write};
use std::path::PathBuf;
use std::process;

pub const X32_SYSTEM_PATH: &str = r"C:\Windows\SysWOW64\";
pub const X64_SYSTEM_PATH: &str = r"C:\Windows\System32\";

/// 全局配置，由命令行参数或配置文件填充。
#[derive(Clone, Debug)]
pub struct Config {
    pub force: bool,
    pub system32_path: String,
    pub syswow64_path: String,
    pub dll_names: Vec<String>,
    pub restore_name: Option<String>,
    pub search_term: Option<String>,
    pub proxy: Option<String>,
    pub output_dir: Option<String>,
    pub verbose: bool,
}

/// 目标架构（x86 = SysWOW64, x64 = System32）。
#[derive(Clone, Copy)]
pub enum Architecture {
    X32,
    X64,
}

impl Architecture {
    /// 返回架构的短名称：`"x86"` 或 `"x64"`。
    pub fn name(self) -> &'static str {
        match self {
            Architecture::X32 => "x86",
            Architecture::X64 => "x64",
        }
    }

    /// 返回该架构对应配置中的系统目录路径。
    pub fn path(self, config: &Config) -> &str {
        match self {
            Architecture::X32 => &config.syswow64_path,
            Architecture::X64 => &config.system32_path,
        }
    }
}

#[derive(Serialize, Deserialize)]
struct ConfigFile {
    #[serde(default)]
    system32_path: String,
    #[serde(default)]
    syswow64_path: String,
    #[serde(default)]
    proxy: String,
}

fn config_file_path() -> PathBuf {
    let appdata = env::var("APPDATA").unwrap_or_else(|_| "".to_string());
    if appdata.is_empty() {
        PathBuf::from("config.json")
    } else {
        PathBuf::from(&appdata).join("dll-rs").join("config.json")
    }
}

fn load_config_file() -> ConfigFile {
    let path = config_file_path();
    match std::fs::read_to_string(&path) {
        Ok(content) => serde_json::from_str(&content).unwrap_or(ConfigFile::default()),
        Err(_) => ConfigFile::default(),
    }
}

impl Default for ConfigFile {
    fn default() -> Self {
        ConfigFile {
            system32_path: X64_SYSTEM_PATH.to_string(),
            syswow64_path: X32_SYSTEM_PATH.to_string(),
            proxy: String::new(),
        }
    }
}

/// 将当前配置保存到 `%APPDATA%\dll-rs\config.json`。
pub fn save_config_file(config: &Config) -> anyhow::Result<()> {
    let path = config_file_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let cf = ConfigFile {
        system32_path: config.system32_path.clone(),
        syswow64_path: config.syswow64_path.clone(),
        proxy: config.proxy.clone().unwrap_or_default(),
    };
    let json = serde_json::to_string_pretty(&cf)?;
    std::fs::write(&path, json)?;
    println!("配置已保存到 {}", path.display());
    Ok(())
}

/// 打印帮助信息到 stderr。
pub fn print_help() {
    eprintln!(
        r"用法: dll [选项] <name.dll> [name.dll ...]

安装缺失的 DLL 文件到系统目录。

参数:
  <name.dll>...        要安装的 DLL 文件名（至少一个）

选项:
  -f, --force           强制覆盖已存在的文件（自动备份到 %%TEMP%%\dll-rs\）
  -h, --help            显示此帮助信息
  -V, --version         显示版本号
  -v, --verbose         显示详细日志
      --system32 <路径>  自定义 x64 系统目录（默认: C:\Windows\System32\）
      --syswow64 <路径>  自定义 x86 系统目录（默认: C:\Windows\SysWOW64\）
      --search <关键词>  搜索 DLL 文件并交互选择
      --restore [名称]   从 %%TEMP%%\dll-rs\ 恢复备份
      --proxy <地址>     使用 HTTP 代理（默认读取 HTTPS_PROXY/HTTP_PROXY 环境变量）
      --output <目录>    只下载到指定目录，不安装到系统
      --save-config      将当前选项保存到配置文件

示例:
  dll dxgi.dll
  dll -f dxgi.dll d3dcompiler.dll
  dll --search directx
  dll --restore
  dll --restore dxgi.dll
  dll --proxy http://127.0.0.1:8080 dxgi.dll"
    );
}

fn proxy_from_env() -> Option<String> {
    for key in &[
        "HTTPS_PROXY",
        "https_proxy",
        "HTTP_PROXY",
        "http_proxy",
        "ALL_PROXY",
        "all_proxy",
    ] {
        if let Ok(val) = env::var(key) {
            if !val.is_empty() {
                return Some(val);
            }
        }
    }
    None
}

/// 交互式选择：打印带编号的列表，等待用户输入序号。
pub fn select_interactive(items: &[String], prompt: &str) -> anyhow::Result<String> {
    for (i, item) in items.iter().enumerate() {
        println!("  {}. {}", i + 1, item);
    }
    print!("\n{} (1-{}): ", prompt, items.len());
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let idx: usize = input
        .trim()
        .parse()
        .map_err(|_| anyhow::anyhow!("无效输入，请输入数字"))?;
    if idx < 1 || idx > items.len() {
        anyhow::bail!("编号超出范围 (1-{})", items.len());
    }
    Ok(items[idx - 1].clone())
}

/// 从原始参数切片解析配置（测试用 inject 版本）。
pub fn parse_args_from(args: &[String]) -> anyhow::Result<Config> {
    let cf = load_config_file();

    if args.len() < 2 {
        print_help();
        anyhow::bail!("用法: dll [选项] <name.dll> [name.dll ...]");
    }

    let mut force = false;
    let mut system32_path = if cf.system32_path.is_empty() {
        X64_SYSTEM_PATH.to_string()
    } else {
        cf.system32_path.clone()
    };
    let mut syswow64_path = if cf.syswow64_path.is_empty() {
        X32_SYSTEM_PATH.to_string()
    } else {
        cf.syswow64_path.clone()
    };
    let env_proxy = proxy_from_env();
    let mut proxy = if !cf.proxy.is_empty() {
        Some(cf.proxy.clone())
    } else if let Some(ref p) = env_proxy {
        Some(p.clone())
    } else {
        None
    };
    let mut dll_names = Vec::new();
    let mut search_term = None;
    let mut restore_name = None;
    let mut output_dir = None;
    let mut verbose = false;
    let mut save_config = false;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "-f" | "--force" => force = true,
            "-h" | "--help" => {
                print_help();
                process::exit(0);
            }
            "-V" | "--version" => {
                println!("dll {}", env!("CARGO_PKG_VERSION"));
                process::exit(0);
            }
            "-v" | "--verbose" => verbose = true,
            "--save-config" => save_config = true,
            "--system32" => {
                i += 1;
                let mut p = args.get(i).context("--system32 需要路径参数")?.clone();
                if !p.ends_with('\\') && !p.ends_with('/') {
                    p.push('\\');
                }
                system32_path = p;
            }
            "--syswow64" => {
                i += 1;
                let mut p = args.get(i).context("--syswow64 需要路径参数")?.clone();
                if !p.ends_with('\\') && !p.ends_with('/') {
                    p.push('\\');
                }
                syswow64_path = p;
            }
            "--proxy" => {
                i += 1;
                proxy = Some(args.get(i).context("--proxy 需要代理地址")?.clone());
            }
            "--output" => {
                i += 1;
                let mut p = args.get(i).context("--output 需要目录路径")?.clone();
                if !p.ends_with('\\') && !p.ends_with('/') {
                    p.push('\\');
                }
                output_dir = Some(p);
            }
            "--search" => {
                i += 1;
                search_term = Some(args.get(i).context("--search 需要搜索关键词")?.clone());
            }
            "--restore" => match args.get(i + 1) {
                Some(n) if !n.starts_with('-') => {
                    i += 1;
                    restore_name = Some(n.to_lowercase());
                }
                _ => restore_name = Some(String::new()),
            },
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

    if dll_names.is_empty() && restore_name.is_none() && search_term.is_none() && !save_config {
        print_help();
        anyhow::bail!("未指定 DLL 文件名");
    }

    let config = Config {
        force,
        system32_path,
        syswow64_path,
        dll_names,
        restore_name,
        search_term,
        proxy,
        output_dir,
        verbose,
    };

    if save_config {
        save_config_file(&config)?;
        process::exit(0);
    }

    Ok(config)
}

/// 从 `env::args()` 解析配置（入口版本）。
pub fn parse_args() -> anyhow::Result<Config> {
    let args: Vec<String> = env::args().collect();
    parse_args_from(&args)
}
