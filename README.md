# dll-rs

Install missing or corrupted DLL files from the command line.

Scrapes `cn.dll-files.com` for the DLL, downloads the ZIP, extracts the `.dll`, and writes it to the correct system directory — both x64 (`System32`) and x32 (`SysWOW64`).

## Requirements

- Windows (x64)
- Administrator privileges

## Install

```powershell
cargo install dll
```

Or build from source:

```powershell
git clone https://github.com/aachou/dll-rs.git
cd dll-rs
cargo build --release
```

## Usage

Run **as Administrator**, then:

```powershell
dll dxgi.dll
```

The argument **must end with `.dll`**. The tool finds both 32-bit and 64-bit versions and installs them to the appropriate system directories. If a DLL already exists, it is skipped silently.

## How it works

1. Fetches `https://cn.dll-files.com/<name>.html` and parses the download page URLs for each architecture.
2. Visits each download page to extract the real download link from embedded JavaScript.
3. Downloads the ZIP archive and extracts the `.dll` file.
4. Writes it to `C:\Windows\System32\` (x64) and `C:\Windows\SysWOW64\` (x32).

x32 and x64 installations are independent — if one architecture is not found on the site, the other is still installed.

## License

MIT
