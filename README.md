# dll-rs

一行命令安装缺失或损坏的 DLL 文件。

自动从 `cn.dll-files.com` 抓取 DLL，下载 ZIP 包，解压 `.dll` 文件并写入正确的系统目录——同时处理 x64（`System32`）和 x32（`SysWOW64`），两个架构**并行下载**。

## 环境要求

- Windows（x64）
- 管理员权限（写入系统目录时需要）

## 安装

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

## 测试

```powershell
cargo test
```

## 许可证

MIT
