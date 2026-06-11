use anyhow::Context;
use regex::Regex;
use zip::read::ZipArchive;

use std::env;
use std::fs::File;
use std::io;
use std::io::Cursor;
use std::path::Path;

const BASE_URL: &str = "https://cn.dll-files.com";

const X32_SYSTEM_PATH: &str = r"C:\Windows\SysWOW64\";
const X64_SYSTEM_PATH: &str = r"C:\Windows\System32\";


fn main() -> anyhow::Result<()> {
    let name = parse_dll_name()?;
    let dll = Dll::new(name);

    let (x32_page_url, x64_page_url) = dll
        .get_downpage_url()
        .with_context(|| format!("get {} download page url fail", dll.name))?;

    install_arch(&dll, Architecture::X32, &x32_page_url);
    install_arch(&dll, Architecture::X64, &x64_page_url);

    Ok(())
}

fn install_arch(dll: &Dll, arch: Architecture, page_url: &str) {
    let arch_name = dll.arch_name(arch);

    if page_url.is_empty() {
        println!("The {} {} download page url not found", arch_name, dll.name);
        return;
    }

    let download_url = match dll.get_download_url(page_url) {
        Ok(url) => url,
        Err(e) => {
            eprintln!("Get {} {} download url fail: {}", arch_name, dll.name, e);
            return;
        }
    };

    match dll.install_dll(&download_url, arch) {
        Ok(_) => println!("Install {} {} success!", arch_name, dll.name),
        Err(e) => eprintln!("Install {} {} fail: {}", arch_name, dll.name, e),
    }
}

fn parse_dll_name() -> anyhow::Result<String> {
    let dll = env::args().nth(1).context("Usage: dll <name.dll>")?.to_lowercase();

    if !dll.ends_with(".dll") {
        anyhow::bail!("argument must end with .dll");
    }

    Ok(dll)
}

enum Architecture {
    X32,
    X64,
}

struct Dll {
    name: String,
}

impl Dll {
    fn new(name: String) -> Self {
        Self { name }
    }

    fn arch_name(&self, arch: Architecture) -> &'static str {
        match arch {
            Architecture::X32 => "x32",
            Architecture::X64 => "x64",
        }
    }

    fn system_path(&self, arch: Architecture) -> &'static str {
        match arch {
            Architecture::X32 => X32_SYSTEM_PATH,
            Architecture::X64 => X64_SYSTEM_PATH,
        }
    }

    fn get_downpage_url(&self) -> anyhow::Result<(String, String)> {
        let resp = minreq::get(format!("{BASE_URL}/{}.html", self.name))
            .send()
            .context("send request fail")?;

        let html = resp.as_str().context("read response fail")?;
        if html.contains("error-404") {
            anyhow::bail!("dll html page not found");
        }

        let section_re = Regex::new(r#"(?s)<section class="file-info-grid".+?</section>"#).unwrap();
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

            let architecture = arch_re
                .captures_iter(meta_info)
                .filter_map(|m| m.name("arch"))
                .nth(1)
                .map(|m| m.as_str())
                .unwrap_or("");

            if architecture.is_empty() {
                continue;
            }
            if architecture == "32" && !x32_url.is_empty()
                || architecture == "64" && !x64_url.is_empty()
            {
                continue;
            }

            let link = match link_re.captures(section).and_then(|m| m.name("link")) {
                Some(m) => m.as_str(),
                None => continue,
            };

            let full_url = format!("{BASE_URL}{link}");
            match architecture {
                "32" => x32_url = full_url,
                "64" => x64_url = full_url,
                _ => {}
            }
        }

        Ok((x32_url, x64_url))
    }

    fn get_download_url(&self, downpage_url: &str) -> anyhow::Result<String> {
        let resp = minreq::get(downpage_url)
            .send()
            .context("send request fail")?;

        let html = resp.as_str().context("read response fail")?;

        let url_re = Regex::new(r#"downloadUrl\s=\s"(?<link>.+?)";"#)?;
        match url_re.captures(html).and_then(|m| m.name("link")) {
            Some(m) => Ok(m.as_str().replace("amp;", "")),
            None => anyhow::bail!("dll download url not found"),
        }
    }

    fn install_dll(&self, download_url: &str, arch: Architecture) -> anyhow::Result<()> {
        let resp = minreq::get(download_url)
            .send()
            .context("send request fail")?;

        let cursor = Cursor::new(resp.as_bytes());
        let mut archive = ZipArchive::new(cursor).context("new zip archive fail")?;

        let sys_path = self.system_path(arch);
        let dll_file_path = format!("{sys_path}{}", self.name);

        if Path::new(&dll_file_path).exists() {
            println!("{} already exists, skipping", self.name);
            return Ok(());
        }

        for i in 0..archive.len() {
            let mut file = archive.by_index(i).context("get zip entry fail")?;
            if !file.name().ends_with(".dll") {
                continue;
            }
            let mut dll_file = File::create(&dll_file_path).context("create dll file fail")?;
            io::copy(&mut file, &mut dll_file).context("write dll file fail")?;
        }

        Ok(())
    }
}
