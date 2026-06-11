mod cli;
mod error;
mod installer;
mod scraper;

fn restore_flow(filter: &str) -> anyhow::Result<()> {
    let filter_opt = if filter.is_empty() {
        None
    } else {
        Some(filter)
    };
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

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = cli::parse_args()?;

    if let Some(ref filter) = config.restore_name {
        return restore_flow(filter.as_str());
    }

    if let Some(ref term) = config.search_term {
        let client = scraper::build_client(config.proxy.as_deref())?;
        let results = scraper::search_dll(&client, term).await?;
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
            match dll.process().await {
                Ok((x32_ok, x64_ok)) => {
                    let (sym, status) = match (x32_ok, x64_ok) {
                        (true, true) => ("\x1b[32m✓\x1b[0m", "全部成功"),
                        (false, false) => ("\x1b[31m✗\x1b[0m", "全部失败"),
                        (true, false) => ("\x1b[33m⚠\x1b[0m", "仅 x86 成功"),
                        (false, true) => ("\x1b[33m⚠\x1b[0m", "仅 x64 成功"),
                    };
                    println!("  {} {} —— {}", sym, name, status);
                }
                Err(_) => eprintln!("  \x1b[31m✗\x1b[0m {} 处理失败", name),
            }
            println!();
        }
        return Ok(());
    }

    for name in &config.dll_names {
        println!("━━━ {} ━━━", name);
        let dll = scraper::Dll::new(name.clone(), &config);
        match dll.process().await {
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
    use crate::scraper;
    use std::io::Write;
    use std::sync::Mutex;
    static MOCK_LOCK: Mutex<()> = Mutex::new(());

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
        let args = &[
            "dll".to_string(),
            "--unknown".to_string(),
            "dxgi.dll".to_string(),
        ];
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
        let args = &[
            "dll".to_string(),
            "--system32".to_string(),
            "dxgi.dll".to_string(),
        ];
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

    #[test]
    fn parse_file_flag() {
        let dir = std::env::temp_dir().join("dll-rs-test-file-flag");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("list.txt");
        std::fs::write(
            &path,
            b"dxgi.dll\n# comment\nd3dcompiler.dll\n\nother.dll\n",
        )
        .unwrap();
        let args = &[
            "dll".to_string(),
            "--file".to_string(),
            path.to_string_lossy().to_string(),
        ];
        let cfg = cli::parse_args_from(args).unwrap();
        assert_eq!(
            cfg.dll_names,
            vec!["dxgi.dll", "d3dcompiler.dll", "other.dll"]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn parse_file_flag_combined_with_inline() {
        let dir = std::env::temp_dir().join("dll-rs-test-file-combined");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("more.txt");
        std::fs::write(&path, b"extra.dll\n").unwrap();
        let args = &[
            "dll".to_string(),
            "base.dll".to_string(),
            "--file".to_string(),
            path.to_string_lossy().to_string(),
        ];
        let cfg = cli::parse_args_from(args).unwrap();
        assert_eq!(cfg.dll_names, vec!["base.dll", "extra.dll"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn parse_file_flag_invalid_name() {
        let dir = std::env::temp_dir().join("dll-rs-test-file-invalid");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("bad.txt");
        std::fs::write(&path, b"not_a_dll.txt\n").unwrap();
        let args = &[
            "dll".to_string(),
            "--file".to_string(),
            path.to_string_lossy().to_string(),
        ];
        assert!(cli::parse_args_from(args).is_err());
        let _ = std::fs::remove_dir_all(&dir);
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
            dll_file: None,
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
            dll_file: None,
        };
        assert_eq!(Architecture::X64.path(&cfg), r"C:\Windows\System32\");
    }

    // ---- is_valid_pe ----

    #[test]
    fn pe_valid_mz_header() {
        let dir = std::env::temp_dir().join("dll-rs-test");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("test.dll");
        std::fs::write(
            &path,
            b"MZ\x90\x00\x03\x00\x00\x00\x04\x00\x00\x00\xff\xff\x00\x00",
        )
        .unwrap();
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

        installer::extract_and_write("test.dll", &zip_data, &dest, false, false).unwrap();
        assert!(std::path::Path::new(&dest).exists());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn extract_and_write_rejects_non_pe() {
        let dir = std::env::temp_dir().join("dll-rs-test-extract-bad");
        let _ = std::fs::create_dir_all(&dir);
        let dest = dir.join("bad.dll").to_string_lossy().to_string();
        let zip_data = create_dummy_zip("bad.dll", b"\x00\x00\x00\x00");

        assert!(installer::extract_and_write("bad.dll", &zip_data, &dest, false, false).is_err());
        assert!(!std::path::Path::new(&dest).exists());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn extract_and_write_no_match_in_zip() {
        let dir = std::env::temp_dir().join("dll-rs-test-extract-nomatch");
        let _ = std::fs::create_dir_all(&dir);
        let dest = dir.join("other.dll").to_string_lossy().to_string();
        let zip_data = create_dummy_zip("something_else.dll", b"MZ\x90\x00");

        assert!(installer::extract_and_write("other.dll", &zip_data, &dest, false, false).is_err());

        let _ = std::fs::remove_dir_all(&dir);
    }

    // ---- Version ----

    #[test]
    fn version_string_is_not_empty() {
        assert!(!env!("CARGO_PKG_VERSION").is_empty());
    }

    // ---- CLI arg error snapshots ----

    #[test]
    fn error_no_dll_extension() {
        let args = &["dll".to_string(), "foo".to_string()];
        let err = cli::parse_args_from(args).unwrap_err().to_string();
        assert!(err.contains(".dll"));
    }

    #[test]
    fn error_unknown_flag() {
        let args = &[
            "dll".to_string(),
            "--bogus".to_string(),
            "x.dll".to_string(),
        ];
        let err = cli::parse_args_from(args).unwrap_err().to_string();
        assert!(err.contains("未知选项"));
    }

    #[test]
    fn error_system32_missing_arg() {
        let args = &[
            "dll".to_string(),
            "--system32".to_string(),
            "x.dll".to_string(),
        ];
        assert!(cli::parse_args_from(args).is_err());
    }

    // ---- Mock HTTP integration ----

    #[test]
    fn mock_full_install_flow_x64_only() {
        let _guard = MOCK_LOCK.lock().unwrap_or_else(|e| e.into_inner());

        let mut server = mockito::Server::new();
        let base = server.url();

        let zip_url = format!("{}/dl.zip", base);

        let dl_page_html = format!(r#"<script>downloadUrl = "{}";</script>"#, zip_url);

        let page_html = format!(
            r#"<!DOCTYPE html>
<section class="file-info-grid">
  <div class="right-pane">
    <p>Version</p>
    <p>64</p>
    <a href="/dl-page" data-ga-action>Download x64</a>
  </div>
</section>"#
        );

        let _m1 = server
            .mock("GET", "/dxgi.dll.html")
            .with_status(200)
            .with_header("content-type", "text/html")
            .with_body(&page_html)
            .create();

        let _m2 = server
            .mock("GET", "/dl-page")
            .with_status(200)
            .with_header("content-type", "text/html")
            .with_body(&dl_page_html)
            .create();

        let zip_data = create_dummy_zip("dxgi.dll", b"MZ\x90\x00\x00\x00");
        let _m3 = server
            .mock("GET", "/dl.zip")
            .with_status(200)
            .with_header("content-type", "application/zip")
            .with_body(&zip_data)
            .create();

        std::env::set_var("DLL_RS_BASE_URL", &base);

        let out_dir = std::env::temp_dir().join("dll-rs-integration-test");
        let _ = std::fs::create_dir_all(&out_dir);

        let config = cli::Config {
            force: true,
            system32_path: format!(r"{}\", out_dir.to_string_lossy()),
            syswow64_path: format!(r"{}\", out_dir.to_string_lossy()),
            dll_names: vec!["dxgi.dll".to_string()],
            restore_name: None,
            search_term: None,
            proxy: None,
            output_dir: None,
            verbose: false,
            dll_file: None,
        };

        let dll = scraper::Dll::new("dxgi.dll".to_string(), &config);
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(dll.process());

        std::env::remove_var("DLL_RS_BASE_URL");

        assert!(result.is_ok(), "process failed: {:?}", result.err());
        let installed = out_dir.join("dxgi.dll");
        assert!(installed.exists(), "DLL was not installed");

        let _ = std::fs::remove_dir_all(&out_dir);
    }

    #[test]
    fn mock_search_returns_results() {
        let _guard = MOCK_LOCK.lock().unwrap_or_else(|e| e.into_inner());

        let mut server = mockito::Server::new();
        let base = server.url();

        let search_html =
            r#"<a href="/dxgi.dll.html">dxgi.dll</a><a href="/d3d11.dll.html">d3d11.dll</a>"#;

        let _m = server
            .mock("GET", "/search?q=dxgi")
            .with_status(200)
            .with_header("content-type", "text/html")
            .with_body(search_html)
            .create();

        std::env::set_var("DLL_RS_BASE_URL", &base);
        let rt = tokio::runtime::Runtime::new().unwrap();
        let client = rt.block_on(async { scraper::build_client(None).unwrap() });
        let results = rt.block_on(scraper::search_dll(&client, "dxgi")).unwrap();
        std::env::remove_var("DLL_RS_BASE_URL");

        assert!(results.contains(&"dxgi.dll".to_string()));
        assert!(results.contains(&"d3d11.dll".to_string()));
    }

    #[test]
    fn mock_search_no_results() {
        let _guard = MOCK_LOCK.lock().unwrap_or_else(|e| e.into_inner());

        let mut server = mockito::Server::new();
        let base = server.url();

        let _m = server
            .mock("GET", "/search?q=nonexistent")
            .with_status(200)
            .with_header("content-type", "text/html")
            .with_body("<html>nothing here</html>")
            .create();

        std::env::set_var("DLL_RS_BASE_URL", &base);
        let rt = tokio::runtime::Runtime::new().unwrap();
        let client = rt.block_on(async { scraper::build_client(None).unwrap() });
        let results = rt
            .block_on(scraper::search_dll(&client, "nonexistent"))
            .unwrap();
        std::env::remove_var("DLL_RS_BASE_URL");

        assert!(results.is_empty());
    }
}
