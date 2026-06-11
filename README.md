# dll-rs

一行命令安装缺失或损坏的 DLL 文件。

自动从 `cn.dll-files.com` 抓取 DLL，下载 ZIP 包，解压 `.dll` 文件并写入正确的系统目录——同时处理 x64（`System32`）和 x32（`SysWOW64`）。

## 环境要求

- Windows（x64）
- 管理员权限

## 安装

```powershell
cargo install dll
```

或从源码构建：

```powershell
git clone https://github.com/aachou/dll-rs.git
cd dll-rs
cargo build --release
```

## 用法

**以管理员身份运行**，然后：

```powershell
dll dxgi.dll
```

支持多个 DLL 同时安装：

```powershell
dll dxgi.dll d3dcompiler.dll d3dx9.dll
```

使用 `--force`（或 `-f`）强制覆盖已存在的文件（自动备份为 `.bak`）：

```powershell
dll -f dxgi.dll d3dcompiler.dll
```

自定义安装目录：

```powershell
dll --system32 D:\my-dlls --syswow64 D:\my-dlls\x86 dxgi.dll
```

搜索并交互选择：

```powershell
dll --search directx
```

参数**必须以 `.dll` 结尾**（搜索模式除外）。工具会自动查找并安装 32 位和 64 位两个版本到对应的系统目录。如果 DLL 已存在则跳过（除非指定 `-f`）。

## 工作原理

1. 请求 `https://cn.dll-files.com/<name>.html`，解析各架构的下载页面链接。
2. 访问下载页面，从嵌入的 JavaScript 中提取真实下载地址。
3. 下载 ZIP 压缩包，解压出 `.dll` 文件。
4. 校验 PE 头（`MZ` 魔数），确保文件有效。
5. 写入 `C:\Windows\System32\`（x64）和 `C:\Windows\SysWOW64\`（x32）。

x32 和 x64 安装相互独立——即使某个架构在页面上找不到，另一个架构仍会正常安装。两个版本会**并行下载**以加快速度。

## 许可证

MIT
