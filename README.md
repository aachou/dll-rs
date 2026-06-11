# dll-rs

一行命令安装缺失或损坏的 DLL 文件。

自动从 `cn.dll-files.com` 抓取 DLL，下载 ZIP 包，解压 `.dll` 文件并写入正确的系统目录——同时处理 x64（`System32`）和 x32（`SysWOW64`），两个架构**并行下载**。

## 环境要求

- Windows（x64）
- 管理员权限（写入系统目录时需要）

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

### 基本选项

| 选项 | 说明 |
|------|------|
| `-f` / `--force` | 覆盖已存在的文件，原文件自动备份到 `%TEMP%\dll-rs\` |
| `-h` / `--help` | 显示帮助信息 |
| `-v` / `--verbose` | 显示详细日志（URL、重试信息、ZIP 条目等） |
| `<name.dll>...` | 支持同时指定多个 DLL |

```powershell
dll -f dxgi.dll d3dcompiler.dll
dll -v dxgi.dll
```

### 安装目录

```powershell
dll --system32 D:\my-dlls --syswow64 D:\my-dlls\x86 dxgi.dll
```

### 仅下载（不安装到系统目录）

```powershell
dll --output D:\downloads dxgi.dll
```

### 搜索 DLL

```powershell
dll --search directx
```

交互式选择匹配的 DLL 后自动安装。

### 代理

自动读取 `HTTPS_PROXY` / `HTTP_PROXY` / `ALL_PROXY` 环境变量，也可手动指定：

```powershell
dll --proxy http://127.0.0.1:8080 dxgi.dll
```

### 恢复备份

```powershell
dll --restore              # 列出所有备份，交互选择恢复
dll --restore dxgi.dll     # 按名称筛选，单个直接恢复
```

### 配置文件

部分选项可持久化到 `%APPDATA%\dll-rs\config.json`：

```powershell
dll --save-config --system32 D:\dlls --proxy http://proxy:8080
```

## 工作原理

1. 请求 `https://cn.dll-files.com/<name>.html`，解析 x32/x64 的下载页面链接。
2. 访问下载页面，从嵌入的 JavaScript 中提取真实 ZIP 下载地址。
3. 下载 ZIP 压缩包，解压出 `.dll` 文件（失败自动重试最多 3 次）。
4. 校验 PE 头（`MZ` 魔数），确保文件有效，否则自动清理。
5. 写入对应系统目录（或 `--output` 指定目录）。

x32 和 x64 安装相互独立——即使某个架构找不到，另一个仍会正常安装。

## 测试

```powershell
cargo test
```

包含 **32 个单元测试**，覆盖：

- 参数解析（单 DLL、多 DLL、全部标志）
- `Architecture` 枚举
- PE 文件校验
- 备份/恢复 roundtrip 和筛选
- ZIP 提取和 PE 验证

## 工程结构

```
src/
  main.rs      入口点、恢复流程、所有测试
  cli.rs       参数解析、Config、配置文件
  scraper.rs   HTTP 请求（重试+代理）、HTML 抓取、搜索
  installer.rs ZIP 解压、PE 校验、备份/恢复
```

- 使用 `minreq`（HTTPS + 代理）、`regex`、`zip`、`serde` / `serde_json`

## 许可证

MIT
