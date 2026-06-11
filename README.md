# dll-rs

一行命令安装缺失或损坏的 DLL 文件。

自动从 `cn.dll-files.com` 抓取 DLL，下载 ZIP 包，解压 `.dll` 文件并写入正确的系统目录——同时处理 x64（`System32`）和 x32（`SysWOW64`），两个架构**并行下载**。

## 特性

- **WRP 绕过** —— 自动用 `takeown` + `icacls` 获取系统保护文件写入权限
- **CDN 抗性** —— 下载被拒时自动切换 Referer、HTTP/1.1、Chrome UA 重试
- **彩色输出** —— 状态一目了然（`✓` 成功 / `✗` 失败 / `→` 进行中 / `⚠` 警告）
- **安全备份** —— `--force` 时先 `copy` 备份到 `%TEMP%\dll-rs\`（跨卷安全），再覆盖
- **ZIP 校验** —— 下载后立即检查 `PK\x03\x04` 魔数，无效内容提前报错并显示预览
- **并行安装** —— x86 / x64 两个架构同时下载和安装

## 环境要求

- Windows（x64）
- 建议以**管理员身份**运行（写入 `System32` / `SysWOW64` 时需要）

## 安装

```powershell
git clone https://github.com/aachou/dll-rs.git
cd dll-rs
cargo build --release
```

编译后的二进制在 `target\release\dll.exe`。

## 用法

**以管理员身份运行**，然后：

```powershell
dll dxgi.dll
```

### 基本选项

| 选项 | 说明 |
|------|------|
| `-f` / `--force` | 覆盖已存在的文件，自动备份到 `%TEMP%\dll-rs\` |
| `-h` / `--help` | 显示帮助信息 |
| `-V` / `--version` | 显示版本号 |
| `-v` / `--verbose` | 显示详细日志 |
| `<name.dll>...` | 支持同时指定多个 DLL |
| `--file <路径>` | 从文本文件读取 DLL 名称（每行一个，`#` 开头为注释） |

```powershell
dll -f dxgi.dll d3dcompiler.dll
dll --file list.txt
```

### 安装目录

默认安装到系统目录，可自定义：

```powershell
dll --system32 D:\my-dlls --syswow64 D:\my-dlls\x86 dxgi.dll
```

### 仅下载（不安装到系统目录）

如果系统文件被 Windows 保护无法写入，可用此选项下载到自定义目录：

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

在国内 cn.dll-files.com 访问缓慢时建议使用代理。

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

## 测试

```powershell
cargo test
```

目前 42 个单元测试，涵盖 CLI 解析、PE 校验、备份/恢复、ZIP 提取、Mock HTTP 集成。

## 许可证

MIT
