mod cli;
mod installer;
mod scraper;

fn restore_flow(filter: &str) -> anyhow::Result<()> {
    let filter_opt = if filter.is_empty() { None } else { Some(filter) };
    let entries = installer::list_backups(filter_opt)?;

    if entries.is_empty() {
        anyhow::bail!("没有找到匹配的备份文件");
    }

    let (original, backup) = if entries.len() == 1 {
        println!("找到备份: {}", entries[0].0);
        entries.into_iter().next().unwrap()
    } else {
        let names: Vec<String> = entries.iter().map(|(o, _)| o.clone()).collect();
        let selected = cli::select_interactive(&names, "请选择要恢复的备份")?;
        entries.into_iter().find(|(o, _)| *o == selected).unwrap()
    };

    installer::restore_dll(&backup, &original)?;
    println!("已恢复: {}", original);
    Ok(())
}

fn main() -> anyhow::Result<()> {
    let config = cli::parse_args()?;

    if let Some(ref filter) = config.restore_name {
        return restore_flow(filter.as_str());
    }

    if let Some(ref term) = config.search_term {
        let results = scraper::search_dll(term, config.proxy.as_deref())?;
        let selected = if results.len() == 1 {
            println!("找到: {}", results[0]);
            results[0].clone()
        } else if results.is_empty() {
            anyhow::bail!("未找到与 '{}' 相关的 DLL", term);
        } else {
            println!("\n找到以下匹配的 DLL 文件：\n");
            cli::select_interactive(&results, "请选择编号")?
        };
        let mut names = config.dll_names.clone();
        names.push(selected.to_lowercase());
        for name in &names {
            println!("━━━ {} ━━━", name);
            let dll = scraper::Dll::new(name.clone(), &config);
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
                Err(e) => eprintln!("{} 处理失败: {}", name, e),
            }
            println!();
        }
        return Ok(());
    }

    for name in &config.dll_names {
        println!("━━━ {} ━━━", name);
        let dll = scraper::Dll::new(name.clone(), &config);
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
            Err(e) => eprintln!("{} 处理失败: {}", name, e),
        }
        println!();
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::cli::{self, Architecture, Config};
    use crate::installer;
    use std::io::Write;

    // ---- parse_args_from ----

    #[test]
    fn parse_single_dll() {
        let args = &["dll".to_string(), "dxgi.dll".to_string()];
        let cfg = cli::parse_args_from(args).unwrap();
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
        let cfg = cli::parse_args_from(args).unwrap();
        assert!(!cfg.force);
        assert_eq!(cfg.dll_names, vec!["dxgi.dll", "d3dcompiler.dll"]);
    }

    #[test]
    fn parse_force_flag() {
        let args = &["dll".to_string(), "-f".to_string(), "dxgi.dll".to_string()];
        let cfg = cli::parse_args_from(args).unwrap();
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
        let cfg = cli::parse_args_from(args).unwrap();
        assert!(cfg.force);
        assert_eq!(cfg.dll_names, vec!["dxgi.dll"]);
    }

    #[test]
    fn parse_no_args() {
        let args = &["dll".to_string()];
        assert!(cli::parse_args_from(args).is_err());
    }

    #[test]
    fn parse_no_dll_extension() {
        let args = &["dll".to_string(), "dxgi".to_string()];
        assert!(cli::parse_args_from(args).is_err());
    }

    #[test]
    fn parse_unknown_flag() {
        let args = &["dll".to_string(), "--unknown".to_string(), "dxgi.dll".to_string()];
        assert!(cli::parse_args_from(args).is_err());
    }

    #[test]
    fn parse_lowercases_name() {
        let args = &["dll".to_string(), "DXGI.DLL".to_string()];
        let cfg = cli::parse_args_from(args).unwrap();
        assert_eq!(cfg.dll_names, vec!["dxgi.dll"]);
    }

    #[test]
    fn parse_system32_flag() {
        let args = &[
            "dll".to_string(),
            "--system32".to_string(),
            r"D:\custom".to_string(),
            "dxgi.dll".to_string(),
        ];
        let cfg = cli::parse_args_from(args).unwrap();
        assert_eq!(cfg.system32_path, r"D:\custom\");
    }

    #[test]
    fn parse_syswow64_flag() {
        let args = &[
            "dll".to_string(),
            "--syswow64".to_string(),
            r"E:\other".to_string(),
            "dxgi.dll".to_string(),
        ];
        let cfg = cli::parse_args_from(args).unwrap();
        assert_eq!(cfg.syswow64_path, r"E:\other\");
    }

    #[test]
    fn parse_system32_trailing_slash_not_added() {
        let args = &[
            "dll".to_string(),
            "--system32".to_string(),
            r"C:\Windows\".to_string(),
            "dxgi.dll".to_string(),
        ];
        let cfg = cli::parse_args_from(args).unwrap();
        assert_eq!(cfg.system32_path, r"C:\Windows\");
    }

    #[test]
    fn parse_system32_flag_missing_arg() {
        let args = &["dll".to_string(), "--system32".to_string(), "dxgi.dll".to_string()];
        assert!(cli::parse_args_from(args).is_err());
    }

    #[test]
    fn parse_force_between_dlls() {
        let args = &[
            "dll".to_string(),
            "a.dll".to_string(),
            "-f".to_string(),
            "b.dll".to_string(),
        ];
        let cfg = cli::parse_args_from(args).unwrap();
        assert!(cfg.force);
        assert_eq!(cfg.dll_names, vec!["a.dll", "b.dll"]);
    }

    #[test]
    fn parse_proxy_flag() {
        let args = &[
            "dll".to_string(),
            "--proxy".to_string(),
            "http://127.0.0.1:8080".to_string(),
            "dxgi.dll".to_string(),
        ];
        let cfg = cli::parse_args_from(args).unwrap();
        assert_eq!(cfg.proxy, Some("http://127.0.0.1:8080".to_string()));
    }

    #[test]
    fn parse_output_flag() {
        let args = &[
            "dll".to_string(),
            "--output".to_string(),
            r"D:\dlldl".to_string(),
            "dxgi.dll".to_string(),
        ];
        let cfg = cli::parse_args_from(args).unwrap();
        assert_eq!(cfg.output_dir, Some(r"D:\dlldl\".to_string()));
    }

    #[test]
    fn parse_verbose_flag() {
        let args = &["dll".to_string(), "-v".to_string(), "dxgi.dll".to_string()];
        let cfg = cli::parse_args_from(args).unwrap();
        assert!(cfg.verbose);
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
    fn arch_x32_path() {
        let cfg = Config {
            force: false,
            system32_path: r"C:\Windows\System32\".to_string(),
            syswow64_path: r"C:\Windows\SysWOW64\".to_string(),
            dll_names: vec![],
            restore_name: None,
            search_term: None,
            proxy: None,
            output_dir: None,
            verbose: false,
        };
        assert_eq!(Architecture::X32.path(&cfg), r"C:\Windows\SysWOW64\");
    }

    #[test]
    fn arch_x64_path() {
        let cfg = Config {
            force: false,
            system32_path: r"C:\Windows\System32\".to_string(),
            syswow64_path: r"C:\Windows\SysWOW64\".to_string(),
            dll_names: vec![],
            restore_name: None,
            search_term: None,
            proxy: None,
            output_dir: None,
            verbose: false,
        };
        assert_eq!(Architecture::X64.path(&cfg), r"C:\Windows\System32\");
    }

    // ---- is_valid_pe ----

    #[test]
    fn pe_valid_mz_header() {
        let dir = std::env::temp_dir().join("dll-rs-test");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("test.dll");
        std::fs::write(&path, b"MZ\x90\x00\x03\x00\x00\x00\x04\x00\x00\x00\xff\xff\x00\x00").unwrap();
        assert!(installer::is_valid_pe(path.to_str().unwrap()));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn pe_invalid_no_mz() {
        let dir = std::env::temp_dir().join("dll-rs-test");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("notadll.dll");
        std::fs::write(&path, b"\x00\x00\x00\x00").unwrap();
        assert!(!installer::is_valid_pe(path.to_str().unwrap()));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn pe_empty_file() {
        let dir = std::env::temp_dir().join("dll-rs-test");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("empty.dll");
        std::fs::write(&path, b"").unwrap();
        assert!(!installer::is_valid_pe(path.to_str().unwrap()));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn pe_nonexistent_file() {
        assert!(!installer::is_valid_pe(r"C:\dll-rs-test-nonexistent.dll"));
    }

    // ---- Backup / Restore ----

    #[test]
    fn backup_path_returns_path_in_temp() {
        let original = r"C:\Windows\System32\dxgi.dll";
        let bp = installer::backup_path(original).unwrap();
        assert!(bp.to_string_lossy().contains("dll-rs"));
        assert!(bp.to_string_lossy().ends_with(".bak"));
        assert!(bp.parent().unwrap().exists());
    }

    #[test]
    fn parse_backup_name_roundtrip() {
        let original = r"C:\Windows\System32\dxgi.dll";
        let bp = installer::backup_path(original).unwrap();
        let name = bp.file_name().unwrap().to_string_lossy().to_string();
        let parsed = installer::parse_backup_name(&name).unwrap();
        assert_eq!(parsed, original);
    }

    #[test]
    fn backup_and_restore_roundtrip() {
        let tag = format!("dll-rs-test-restore-{}", std::process::id());
        let dir = std::env::temp_dir().join(&tag);
        let _ = std::fs::create_dir_all(&dir);
        let original_path = dir.join("test.dll");
        std::fs::write(&original_path, b"MZ\x90\x00").unwrap();

        let bp = installer::backup_path(original_path.to_str().unwrap()).unwrap();
        std::fs::rename(&original_path, &bp).unwrap();
        assert!(!original_path.exists());
        assert!(bp.exists());

        installer::restore_dll(&bp, original_path.to_str().unwrap()).unwrap();
        assert!(original_path.exists());
        assert!(!bp.exists());
        assert_eq!(std::fs::read(&original_path).unwrap(), b"MZ\x90\x00");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn list_backups_returns_matching_entries() {
        let tag = format!("dll-rs-test-list-{}", std::process::id());
        let dir = std::env::temp_dir().join(&tag);
        let _ = std::fs::create_dir_all(&dir);

        let test_path = dir.join("C_Windows_System32_foo.dll.bak");
        std::fs::write(&test_path, b"dummy").unwrap();

        let entries = installer::list_backups_from_dir(&dir, Some("foo")).unwrap();
        assert!(!entries.is_empty());
        assert!(entries.iter().any(|(o, _)| o.contains("foo")));

        let entries_all = installer::list_backups_from_dir(&dir, None).unwrap();
        assert!(entries_all.len() >= entries.len());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn list_backups_filter_nonexistent() {
        let tag = format!("dll-rs-test-filter-{}", std::process::id());
        let dir = std::env::temp_dir().join(&tag);
        let _ = std::fs::create_dir_all(&dir);

        let test_path = dir.join("C_Windows_System32_bar.dll.bak");
        std::fs::write(&test_path, b"dummy").unwrap();

        let entries = installer::list_backups_from_dir(&dir, Some("nonexistent")).unwrap();
        assert!(entries.is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }

    // ---- extract_and_write ----

    fn create_dummy_zip(dll_name: &str, content: &[u8]) -> Vec<u8> {
        let mut buf = std::io::Cursor::new(Vec::new());
        let mut zip = zip::ZipWriter::new(&mut buf);
        let options = zip::write::FileOptions::default();
        zip.start_file(dll_name, options).unwrap();
        zip.write_all(content).unwrap();
        drop(zip);
        buf.into_inner()
    }

    #[test]
    fn extract_and_write_valid_dll() {
        let dir = std::env::temp_dir().join("dll-rs-test-extract");
        let _ = std::fs::create_dir_all(&dir);
        let dest = dir.join("test.dll").to_string_lossy().to_string();
        let zip_data = create_dummy_zip("test.dll", b"MZ\x90\x00\x00\x00");

        installer::extract_and_write("test.dll", &zip_data, &dest, false).unwrap();
        assert!(std::path::Path::new(&dest).exists());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn extract_and_write_rejects_non_pe() {
        let dir = std::env::temp_dir().join("dll-rs-test-extract-bad");
        let _ = std::fs::create_dir_all(&dir);
        let dest = dir.join("bad.dll").to_string_lossy().to_string();
        let zip_data = create_dummy_zip("bad.dll", b"\x00\x00\x00\x00");

        assert!(installer::extract_and_write("bad.dll", &zip_data, &dest, false).is_err());
        assert!(!std::path::Path::new(&dest).exists());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn extract_and_write_no_match_in_zip() {
        let dir = std::env::temp_dir().join("dll-rs-test-extract-nomatch");
        let _ = std::fs::create_dir_all(&dir);
        let dest = dir.join("other.dll").to_string_lossy().to_string();
        let zip_data = create_dummy_zip("something_else.dll", b"MZ\x90\x00");

        assert!(
            installer::extract_and_write("other.dll", &zip_data, &dest, false).is_err()
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
