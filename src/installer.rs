use anyhow::Context;
use std::fs::File;
use std::io::{self, Cursor, Read};
use std::path::{Path, PathBuf};
use std::process::Command;
use zip::read::ZipArchive;

/// 检查文件是否为有效的 PE 格式（以 `MZ` 魔数字节开头）。
pub fn is_valid_pe(path: &str) -> bool {
    let mut buf = [0u8; 2];
    if let Ok(mut f) = File::open(path) {
        if f.read_exact(&mut buf).is_ok() {
            return buf == *b"MZ";
        }
    }
    false
}

/// `%TEMP%\dll-rs` 备份目录路径。
pub fn backup_dir() -> PathBuf {
    std::env::temp_dir().join("dll-rs")
}

/// 为原始路径生成备份路径（`%TEMP%\dll-rs\<sanitized>.bak`）。
pub fn backup_path(original: &str) -> anyhow::Result<PathBuf> {
    let dir = backup_dir();
    std::fs::create_dir_all(&dir).context("创建备份目录失败")?;
    let safe = original.replace(':', "").replace('\\', "_");
    Ok(dir.join(format!("{}.bak", safe)))
}

/// 将备份文件名（如 `C_Windows_System32_dxgi.dll.bak`）还原为原始路径。
pub fn parse_backup_name(filename: &str) -> Option<String> {
    let s = filename.strip_suffix(".bak")?;
    let parts: Vec<&str> = s.split('_').collect();
    if parts.len() < 2 {
        return None;
    }
    let mut result = parts[0].to_string();
    result.push(':');
    result.push('\\');
    result.push_str(&parts[1..].join("\\"));
    Some(result)
}

/// 列出所有备份（按原始路径排序），可选按名称筛选。
pub fn list_backups(filter: Option<&str>) -> anyhow::Result<Vec<(String, PathBuf)>> {
    list_backups_from_dir(&backup_dir(), filter)
}

/// 从指定目录列出备份（测试用，`list_backups` 的底层实现）。
pub fn list_backups_from_dir(
    dir: &Path,
    filter: Option<&str>,
) -> anyhow::Result<Vec<(String, PathBuf)>> {
    if !dir.exists() {
        anyhow::bail!("备份目录不存在: {}", dir.display());
    }

    let mut entries: Vec<(String, PathBuf)> = Vec::new();
    for entry in std::fs::read_dir(dir).context("读取备份目录失败")? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("bak") {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if let Some(original) = parse_backup_name(&name) {
            if let Some(f) = filter {
                if !f.is_empty() && !original.to_lowercase().contains(&f.to_lowercase()) {
                    continue;
                }
            }
            entries.push((original, path));
        }
    }

    entries.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(entries)
}

/// 从备份路径恢复到原始位置（移动操作）。
pub fn restore_dll(backup_path: &Path, original_path: &str) -> anyhow::Result<()> {
    if let Some(parent) = Path::new(original_path).parent() {
        std::fs::create_dir_all(parent).context("创建目标目录失败")?;
    }
    std::fs::rename(backup_path, original_path)
        .with_context(|| format!("恢复文件失败: {}", original_path))?;
    Ok(())
}

/// 尝试通过 `takeown` + `icacls` 获取文件所有权和写入权限（绕过 WRP）。
fn try_take_ownership(path: &str) -> anyhow::Result<()> {
    Command::new("takeown")
        .args(["/f", path])
        .output()
        .context("takeown 执行失败")?;
    let user = std::env::var("USERNAME").unwrap_or_else(|_| "Administrators".to_string());
    Command::new("icacls")
        .args([path, "/grant", &format!("{user}:F")])
        .output()
        .context("icacls 执行失败")?;
    Ok(())
}

/// 从 ZIP 数据中提取与 `dll_name` 匹配的 DLL，校验 PE 后写入 `dest_path`。
pub fn extract_and_write(
    dll_name: &str,
    zip_data: &[u8],
    dest_path: &str,
    verbose: bool,
    force: bool,
) -> anyhow::Result<()> {
    let cursor = Cursor::new(zip_data);
    let mut archive = ZipArchive::new(cursor).context("解压 ZIP 失败")?;

    if verbose {
        println!("  ZIP 包含 {} 个文件", archive.len());
    }

    let mut extracted = false;
    for i in 0..archive.len() {
        let mut file = archive.by_index(i).context("读取 ZIP 条目失败")?;
        if !file.name().ends_with(".dll") {
            continue;
        }
        if !file
            .name()
            .to_lowercase()
            .ends_with(&dll_name.to_lowercase())
        {
            if verbose {
                println!("  跳过 ZIP 条目: {}", file.name());
            }
            continue;
        }

        if force {
            let _ = std::fs::remove_file(dest_path);
        }
        let mut dll_file = match File::create(dest_path) {
            Ok(f) => f,
            Err(e) if e.raw_os_error() == Some(5) => {
                eprintln!("  \x1b[33m⚠\x1b[0m 权限不足，尝试通过 takeown + icacls 获取写入权限...");
                try_take_ownership(dest_path)?;
                if force {
                    let _ = std::fs::remove_file(dest_path);
                }
                File::create(dest_path).with_context(|| {
                    format!(
                        "无法写入 {}（即使以管理员运行，Windows 资源保护 WRP 仍阻止修改系统文件）。\n请使用 --output <目录> 下载到自定义目录",
                        dest_path
                    )
                })?
            }
            Err(e) => anyhow::bail!("创建文件失败: {}", e),
        };
        if let Err(e) = io::copy(&mut file, &mut dll_file) {
            let _ = std::fs::remove_file(dest_path);
            anyhow::bail!("写入文件失败: {}", e);
        }
        drop(dll_file);

        if !is_valid_pe(dest_path) {
            let _ = std::fs::remove_file(dest_path);
            anyhow::bail!("下载的文件不是有效的 PE 格式");
        }

        extracted = true;
        break;
    }

    if !extracted {
        anyhow::bail!("ZIP 中未找到与 {} 匹配的 DLL 文件", dll_name);
    }

    Ok(())
}
