# 应用图标与托盘图标设计

## 背景

1. `cmd/agent-notify/main.go` 的 `go:generate` 用 `rsrc` 生成三个 `rsrc_windows_*.syso`，只带了 `-manifest`，**没有嵌入任何图标资源**。因此 `agent-notify.exe` 在资源管理器、任务栏、桌面快捷方式里显示的都是 Windows 默认空白 exe 图标。
2. 安装脚本与安装器已经会创建桌面快捷方式（`Agent-notify.lnk`，目标 `agent-notify.exe widget`），快捷方式图标默认继承目标 exe，所以只要 exe 有图标，快捷方式就会跟着变。
3. 安装器 `installer/agent-notify.iss` 没有设置 `SetupIconFile`，安装包自身也是默认图标。
4. 托盘图标由 `internal/ui/tray.go` 的 `CreateDotIcon` 在运行时用 GDI 画一个 16×16 纯色圆点（白描边），绿/橙/红三态分别缓存，靠颜色表达状态。

## 目标

1. exe 内嵌应用图标，桌面快捷方式、任务栏、资源管理器、安装器、控制面板卸载列表统一显示。
2. 托盘图标与应用图标同一套视觉，同时保留绿/橙/红三态状态语义。
3. 图标可复现：设计稿与生成脚本都在仓库里，改图只需跑一条命令。

## 非目标

1. 不改悬浮窗内部绘制（窗口标题栏、卡片、按钮图标等）。
2. 不改托盘的状态判定、右键菜单、显示/隐藏逻辑。
3. 不引入新的构建期依赖：生成物（`.ico`、`.syso`）提交进仓库，打包和 CI 不需要 Python。
4. 不改安装器的交互流程与选项。
5. **不做矢量重绘。** 应用图标由大模型生成的设计稿直接缩放得到，不逐形状手工重画。

## 图形来源与规格

应用图标来自大模型生成的设计稿：白色圆角聊天气泡（左下小尾巴）、气泡中央镂空一颗绿色四角星、右上角一颗白色小星，底板为品牌绿垂直渐变圆角方块。

设计稿的关键几何（像素测量得到，用于裁透明角）：

| 项 | 取值 |
|---|---|
| 画布 | 正方形，图标铺满、无外边距 |
| 底板圆角半径 | `0.166 × 边长` |
| 背景渐变 | 顶部 `#55DD9B` → 底部 `#189C84` |

设计稿原始四角是白色（不透明），生成时必须按上面的圆角半径裁成透明，否则在深色任务栏上会看到一个白方块。

仓库里的设计稿是 512×512（`assets/icon-source.png`）；ICO 最大档为 256，512 已经够用且体积可控。

### 尺寸

输出多尺寸 `.ico`：`16, 20, 24, 32, 40, 48, 64, 128, 256`。每档由设计稿用 LANCZOS 直接缩到精确像素，不为小尺寸做单独的简化画法。

16px 下星形会糊成一小块亮斑，这是直接缩放的固有代价，本次接受。

### 格式

ICO 内各条目使用 BMP(DIB) 格式而非 PNG 压缩条目，兼容性最好（Inno Setup 等工具读取更稳）。

## 落地位置

| 文件 | 改动 |
|---|---|
| `assets/icon-source.png` | 新增，512×512 设计稿（提交） |
| `assets/agent-notify.ico` | 新增，多尺寸应用图标（提交） |
| `internal/ui/assets/tray_{ready,warning,stopped}.png` | 新增，托盘三态位图（提交） |
| `tools/build-icon.py` | 新增，设计稿转 ICO 与托盘位图的脚本（提交，仅改图时需要跑） |
| `cmd/agent-notify/main.go` | 三行 `go:generate` 增加 `-ico ../../assets/agent-notify.ico` |
| `cmd/agent-notify/rsrc_windows_{386,amd64,arm64}.syso` | 重新生成并提交 |
| `installer/agent-notify.iss` | 新增 `SetupIconFile={#RepoRoot}\assets\agent-notify.ico` |
| `internal/ui/tray_icon.go` | 新增，`CreateStatusIcon`：内嵌位图 → HICON |
| `internal/ui/tray.go` | 删除 `CreateDotIcon`，改用 `CreateStatusIcon` |
| `internal/ui/win32.go` | 补 `CreateDIBSection` 声明与 `BITMAPINFO` 结构 |
| `.gitignore` | 忽略本地设计稿原图 `assets/icon-reference.png` |

`install.ps1` 创建的快捷方式目标就是 exe，图标自动继承，不改。

## 技术方案

### 1. 生成脚本 `tools/build-icon.py`

- 只依赖 Pillow（本机已有 12.2.0）。
- 读设计稿 → 逐档 LANCZOS 缩放 → 用圆角方块遮罩把四角设成透明 → 打包成多尺寸 ICO。
- 遮罩以 4× 超采样绘制再缩小，保证圆角边缘平滑。
- 用 Pillow 的 ICO 写出，`bitmap_format="bmp"` 强制 BMP 条目；每档尺寸通过 `append_images` 提供预缩放帧，避免 Pillow 再从最大档重复缩放。
- 幂等：同样的输入产生同样的字节。

### 2. 嵌入 exe

`go:generate` 的 `rsrc` 调用同时带 `-manifest` 与 `-ico`，两者可以写进同一个 `.syso`。三个架构的 `.syso` 一起重新生成并提交；`go build` 自动链接同目录的 `.syso`，`tools\build-release.ps1` 无需改动。

### 3. 安装器

`installer/agent-notify.iss` 的 `[Setup]` 增加 `SetupIconFile`。桌面/开始菜单快捷方式与 `UninstallDisplayIcon` 都指向 `{app}\agent-notify.exe`，exe 有图标后自动生效，不额外加 `IconFilename`。

### 4. 托盘图标

托盘图标直接复用应用图标的设计稿，只把底板换成三态状态色，不做第二套图形。位图由 `tools/build-icon.py` 从同一份设计稿生成到 `internal/ui/assets/tray_{ready,warning,stopped}.png`（16×16、四角透明），用 `go:embed` 打进二进制。

- 换底色的做法：用「g 通道比 r/b 中较小者高出的数值」估出白色图形（气泡 + 两颗星）的覆盖率，白色部分原样保留，其余按状态色垂直渐变重填；中心星本来就是镂空，会自动显示新底色。
- 三态底板渐变：绿 `#55DD9B → #189C84`（设计稿原色）、橙 `#DDA955 → #9A6818`、红 `#DD5555 → #9A1818`。明度关系对齐设计稿，色相取内部状态色常量。
- `internal/ui/tray_icon.go` 的 `CreateStatusIcon` 只负责把位图包成带 alpha 的 HICON：用 `CreateDIBSection` 建 32bpp 自顶向下位图写入 BGRA，再用 `CreateIconIndirect` 生成图标；遮罩用全黑的 1bpp 位图，实际透明度由 alpha 通道决定。
- `internal/ui/tray.go` 里旧的 `CreateDotIcon` 删除，三态改用 `CreateStatusIcon`；`UpdateState`、右键菜单、退出清理逻辑不变。

## 测试与验收

### 已提交的自动化测试

1. `internal/ui/tray_icon_test.go`：断言三态 `CreateStatusIcon` 都返回非 0 句柄且互不相同。
2. 现有 `internal/ui` 测试不回归。

### 改图后的人工核对（需要 Python，不进 CI）

1. 解析 `.ico` 头部：确认包含 9 档尺寸、条目为 BMP 格式、`256` 档宽高字节为 `0`。
2. 重新跑 `tools/build-icon.py`，确认 `.ico` 字节不变（SHA256 一致）。
3. 重新跑 `go generate ./cmd/agent-notify`，确认三个 `.syso` 字节不变。
4. 构建 exe 后用 Shell API（`ExtractAssociatedIcon`）提取图标，确认非系统默认图标。
5. 构建安装器后用同样方式提取图标，确认安装器也带新图标。

本轮以上 5 项均已手工执行通过。

### 真实验收（必须走真实链路）

1. 安装一次，确认桌面快捷方式、开始菜单、任务栏、资源管理器里的 exe 图标都是新图标，且四角透明（深色背景不出现白角）。
2. 安装器可执行文件本身显示新图标。
3. 托盘在「正常 / 待处理 / 异常」三态下分别观察：颜色正确、星形清晰、深浅色任务栏下都可见。

### 门禁

`go test ./...`、`go vet ./...`、`gofmt -l cmd internal`、`node_modules\.bin\tsc.cmd --noEmit`、`tools\test.ps1`、`tools\lint.ps1`。
