# sOFD

**Super OFD, Simple OFD — or whatever you wish.**

> 开头那句把 sOFD 拆成 *Super OFD* 和 *Simple OFD*，也表明名字怎么念都行。

**OFD 文档的完整 Rust 引擎** —— 打开、渲染、转换、打印、签章、编辑、检索、合规审计，
一套 API 全覆盖。纯 Rust，零 unsafe，遵循 GB/T 33190-2016。

[English](https://github.com/emfox/sOFD/blob/HEAD/README.md) | [简体中文](https://github.com/emfox/sOFD/blob/HEAD/README.zh-CN.md)

[![Rust](https://img.shields.io/badge/rust-1.92%2B-orange)](https://www.rust-lang.org/)
[![Edition](https://img.shields.io/badge/edition-2024-blue)](https://doc.rust-lang.org/edition-guide/rust-2024/)
[![License](https://img.shields.io/badge/license-Apache--2.0-green)](https://github.com/emfox/sOFD/blob/HEAD/LICENSE)
[![Standard](https://img.shields.io/badge/standard-GB%2FT%2033190--2016-red)]()

```rust
use sofd::{Document, RenderOptions};
use sofd_io::FileSource;

let source = FileSource::open("合同.ofd")?;
let doc = Document::open(&source)?;
let png = doc.to_png(0, &RenderOptions::default())?;
```

sOFD 是 [GB/T 33190-2016《电子文件存储与交换格式 版式文档》](https://openstd.samr.gov.cn/bzgk/gb/newGbInfo?hcno=3AF6682D939116B6F5EED53D01A9DB5D)
的 Rust 实现。从解析到渲染全部自研，仓库里唯一的 C 库是 fontconfig——
由 reader 在 Linux 上经 `font-kit` 调用。

当前发布版本为 `0.5.1`，也是第一个发到 crates.io 的版本：全部 21 个 crate 都在上面，
所以 `cargo add sofd` 与 `cargo install sofd-cli` 都能用。本版本还让归档重写不再把
注释里的 `<` 当成标签。本版本提供了什么见
[`CHANGELOG`](https://github.com/emfox/sOFD/blob/HEAD/CHANGELOG.md)，尚待完成的具体工作见
[issue 列表](https://github.com/emfox/sOFD/issues)，
它服务于哪些大方向见 [`docs/Roadmap.md`](https://github.com/emfox/sOFD/blob/HEAD/docs/Roadmap.md)。

本文档分两部分：前半部分给使用 sOFD 的人，后半部分给参与开发的人。

---

# 第一部分：使用

## 特性

| 能力 | 覆盖范围 |
|---|---|
| 阅读渲染 | GB/T 33190 渲染，无损缩放，超大文档分片加载 |
| 格式转换 | OFD ↔ PDF / SVG / PNG，支持批量与合并 |
| 电子签章 | SM2 签名、SM3 摘要、SM4 加密与 GB/T 20520 时间戳。验签核对证明本身以及证书在签署时刻是否有效；调用方若要判断签发者，需自带信任锚，签发者不在锚内时另给结论。GB/T 38540-2020 列出的国际算法尚未实现 |
| 多端交付 | 同一内核编译为 CLI、WASM、C ABI、HTTP 服务与桌面阅读器 |
| 国际化 | 基于 Fluent 的消息层，内置 en-US / zh-CN |

## 可直接运行的示例

每种交付形态一个，各是一个文件：打开文档，把它带到某处，`cargo run` 即可：

| 形态 | 示例 | 展示的内容 |
|---|---|---|
| CLI | [`command_line.rs`](https://github.com/emfox/sOFD/blob/HEAD/crates/sofd-cli/examples/command_line.rs) | 按路径打开、读出文档声明的内容、逐页渲染、校验签名 |
| C ABI | [`c_abi.rs`](https://github.com/emfox/sOFD/blob/HEAD/crates/sofd-ffi/examples/c_abi.rs) | C 宿主会发起的同一组调用，对照生成的[头文件](https://github.com/emfox/sOFD/blob/HEAD/crates/sofd-ffi/include/sofd.h) |
| 浏览器 | [`browser.rs`](https://github.com/emfox/sOFD/blob/HEAD/crates/sofd-wasm/examples/browser.rs) | 从 Rust 侧看到的 JS 边界：位图、SVG、检索 |
| HTTP | [`http_service.rs`](https://github.com/emfox/sOFD/blob/HEAD/crates/sofd-server/examples/http_service.rs) | 路由、存储与优雅退出，可挂进你自己的进程 |
| 桌面 | [`desktop_viewer.rs`](https://github.com/emfox/sOFD/blob/HEAD/crates/sofd-reader/examples/desktop_viewer.rs) | 阅读器界面画进一个 `egui::Ui`，无窗口、无显示服务 |

`cargo build --workspace --examples` 可构建其中四个；浏览器那个需要
`--target wasm32-unknown-unknown`，CI 就在该目标上构建它。

## 获取构建产物

推送 `v*` 标签会构建全部交付形态，每种产品发布一个归档，文件名带发布版本：

| 产物 | 面向 | 运行方式 |
|---|---|---|
| `sofd-<version>-linux.tar.gz`、`-macos`、`-windows` | 桌面 | 解压后运行其中的 `sofd-reader`；每个系统单独一个下载，用户只取自己需要的 |
| `sofd-<version>-web.zip` | 浏览器 | 解压后用 HTTP 提供 `site/`；`file://` 页面无法加载该模块 |
| `sofd-<version>-android.apk` | Android 7.0+（arm64） | 侧载；用一次性调试密钥签名，不能上架应用商店 |
| `sofd-<version>-uos-amd64.deb` | UOS 桌面 V20 | `dpkg -i`，或在文件管理器中安装 |
| `sofd-<version>-ffi.tar.gz` | C、C++ 或其他带 C ABI 的语言 | 链接包内库文件，并包含同目录的 `sofd.h` |

iOS 是唯一被排除的 `egui` 平台：其应用必须经 macOS 签名打包后才能安装，
而本项目不附带任何密钥。

构建产物**未签名**——项目没有签名密钥——因此每次发布都附带 `SHA256SUMS`
清单，这是下载内容唯一的校验依据：

```bash
python3 scripts/release_checksums.py verify .
sha256sum -c SHA256SUMS          # 或者完全不依赖本项目
```

全部 21 个 crate 都已以 `0.5.1` 发布到 crates.io。但其中只有一部分是接口：加上
`sofd` 就够用，它依赖的那些 crate 是被 cargo 一起带过来的，并不是让你逐个点名。
哪些是哪一类，见
[Architecture.md 第 3 节](https://github.com/emfox/sOFD/blob/HEAD/docs/Architecture.md#3-crate-inventory)；
`python3 scripts/publish_order.py` 会打印依赖闭包——从 `sofd` 出发
是全部 21 个，而单独发布 `sofd-crypto` 只需要 3 个。

命令行工具也在其中，发布名是 `sofd-cli`，因为 `sofd` 这个名字留给了库：

```bash
cargo install sofd-cli        # 装出 sofd 命令
```

在自己的项目里用：

```bash
cargo add sofd                # 门面：渲染、签章、转换、打印、检索
cargo add sofd-io             # FileSource，打开文档的来源
```

## 遵循的规范

| 标准 | 范围 |
|---|---|
| [GB/T 33190-2016](https://openstd.samr.gov.cn/bzgk/gb/newGbInfo?hcno=3AF6682D939116B6F5EED53D01A9DB5D) | 电子文件存储与交换格式 版式文档 |
| [GB/T 33476.1-2016](https://openstd.samr.gov.cn/bzgk/gb/newGbInfo?hcno=419233A04D029066CF032CFACBC8713B) | 党政机关电子公文格式规范 第 1 部分：公文结构 |
| [GB/T 33476.2-2016](https://openstd.samr.gov.cn/bzgk/gb/newGbInfo?hcno=5B4E730245FF2FAF909435EBE6A659D2) | 同上 第 2 部分：显现 |
| [GB/T 38540-2020](https://openstd.samr.gov.cn/bzgk/gb/newGbInfo?hcno=EBF1360C272E40E7A8B9B32ED0724AB1) | 安全电子签章密码技术规范 |
| [GM/T 0099-2020](https://std.samr.gov.cn/hb/search/stdHBDetailed?id=C60C676BC220358AE05397BE0A0AFABB) | 开放版式文档密码应用技术规范：签名、加密、完整性 |
| [GB/T 39362-2020](https://openstd.samr.gov.cn/bzgk/gb/newGbInfo?hcno=C6B230010286DC79C84CF8EB72DE3F97) | 党政机关电子公文归档规范 |

GB/T 33476 共三部分，本项目遵循的是第 1、2 部分。GM/T 0099-2020 是国家密码管理局
发布的密码行业标准，因此在行业标准平台而非国家标准平台检索。

条款级覆盖情况见 [`compliance/gb-t-33190.toml`](https://github.com/emfox/sOFD/blob/HEAD/compliance/gb-t-33190.toml)，
判定方法见 [`compliance/CLAUSES.md`](https://github.com/emfox/sOFD/blob/HEAD/compliance/CLAUSES.md)：已登记 56 条，
全部为 `verified`。条款号由 [`scripts/gbt33190.py`](https://github.com/emfox/sOFD/blob/HEAD/scripts/gbt33190.py) 从标准的
转录文本中读取——逐条读取带编号的标题，并与标准印刷的目录页核对，
因此不可能引用标准中不存在的条款。内核能用哪些字形绘制、不能用哪些，
逐字体列在 [`docs/Fonts.md`](https://github.com/emfox/sOFD/blob/HEAD/docs/Fonts.md)；
与其他阅读器的逐页对比结果见 [`docs/Interop.md`](https://github.com/emfox/sOFD/blob/HEAD/docs/Interop.md)。
签名能证明什么、不能证明什么，以及标准为何把签发者交给调用方判断，
见 [`docs/Trust.md`](https://github.com/emfox/sOFD/blob/HEAD/docs/Trust.md)。

## 背景

OFD 和 PDF 都是版式文档，区别在于 OFD 是开放的 ZIP + XML 容器，国密算法与电子印章
写进了标准本身，且要求版面在任意软硬件环境下保持一致。因此它被用作电子公文、
电子证照、电子发票、电子档案的法定载体。

选 Rust 有两个原因。版式文档处理是 CPU 密集型任务，同时要在浏览器、服务端、桌面端
运行，零成本抽象让分层架构不必以性能为代价。另外 OFD 文件来源任意，解析器又是处理
不可信输入的那一环，`#![forbid(unsafe_code)]` 在这里格外重要。

---

# 第二部分：开发

## 模块关系

27 个 crate，五个层次。箭头由使用者指向被依赖者；门面之下的方框，
只有 feature 指名时才会被引入。

```
  ┌──────────────────────────────────────────────────────────────────────┐
  │  交付形态，作为应用而非依赖发布                                      │
  │                                                                      │
  │    reader          cli          wasm          ffi         server     │
  │    egui/eframe     二进制       浏览器        C ABI       HTTP       │
  └──────────────────────────────────────────────────────────────────────┘
                                  │
                                  ▼
  ┌──────────────────────────────────────────────────────────────────────┐
  │  sofd —— 一个句柄，每个能力对应一个 feature                 │
  └──────────────────────────────────────────────────────────────────────┘
                                  │
  ┌──────────────────────────────────────────────────────────────────────┐
  │  始终链接：从打开到显示的通路                                        │
  │                                                                      │
  │    parser ──► package ──► xml                                        │
  │      │             │       io                                        │
  │      ▼             │                                                 │
  │    model ◄─────────┘                                                 │
  │      │                                                               │
  │    display ──► font ──► geometry                                     │
  │                                                                      │
  │    以上各层都经由 error ──► common 处理错误                          │
  └──────────────────────────────────────────────────────────────────────┘

  ┌──────────────────────────────────────────────────────────────────────┐
  │  由 core 的 feature 引入：每个 feature 一个能力                      │
  │                                                                      │
  │    render    display, geometry, font                                 │
  │    print     render, display                                         │
  │    convert   render, parser, display                                 │
  │    sign      crypto, package, model                                  │
  │    crypto    common, error                                           │
  │    edit      parser, package, model                                  │
  │    search    display, geometry                                       │
  │    archive   package, model                                          │
  │    annot     无：它读的 parser 永远不是可选的                         │
  └──────────────────────────────────────────────────────────────────────┘

  sofd-i18n      供外壳使用的文案；引擎自身从不产生文案
  sofd-testkit   测试用的夹具，仅作为 dev-dependency
```

关键在 `sofd-display`：所有输出后端读取同一份扁平化后的显示列表，
导出结果因此不会与预览产生偏差。[`docs/Architecture.md`](https://github.com/emfox/sOFD/blob/HEAD/docs/Architecture.md)
第 3 节以表格形式给出同一份清单，逐 crate 列出职责与发布状态。

## 一份文档的流向

`Content.xml` 是一棵嵌套的模板树。若每个输出后端各自遍历一遍，不仅逻辑重复，
更糟的是各后端会逐渐走样——这正是导出结果与预览不一致的常见原因。

因此这棵树只扁平化一次，变成一串带绝对变换矩阵的命令，所有下游都读它：

```
Content.xml（嵌套模板树）
       |
       v  只解析一次
DisplayList（已扁平化、CTM 已绝对化、资源已解析）
       |
       +--> render    光栅
       +--> print
       +--> convert   PDF / SVG / PNG
       +--> search    文本提取
```

Office 与 OCR 转换在路线图中，不在代码里：两者都没有对应的 feature，
启用会直接编译失败，而不是编译成一个空壳。

## 引擎层不放文案

错误是以参数构成的结构化枚举，从不是散文：

```rust
// 任何语言都能渲染
FormatError::PageOutOfRange { index: 42, total: 100 }

// 在这里硬编码散文，会让引擎无法被 C ABI、JS 或 Python 使用
"page index out of range: 42 (document has 100 pages)"
```

引擎自己的消息目录（`sofd-i18n`）只装引擎消息。若某个表现层有自己的产品文案
——reader 就是如此——它把文案放在自己身边，再通过 `I18nBuilder` 合并进来，
这样引擎就永远不会提到一个不属于它的控件。

## 不可信输入

| 威胁 | 防御 |
|---|---|
| Zip Slip 路径穿越 | `PackageError::IllegalPath` |
| Zip 炸弹 | 条目数量与解压后大小的上限（512 MB） |
| 深层嵌套导致栈溢出 | `DisplayListBuilder::max_depth`，默认 64 |
| 恶意文档拖死渲染 | `RenderError::BudgetExceeded`，降级为部分输出 |
| 编辑操作悄悄破坏签章 | `EditError::WouldInvalidateSignature`，需确认后才执行 |

## 构建

需要 Rust 1.92+ 与 Edition 2024，这是 workspace 清单中 `rust-version` 声明的版本；
`.github/workflows/rust.yml` 写明 CI 使用的版本。
`cargo check --workspace` 即可确认工具链可用。

构建这些 crate 不需要 C 工具链。Linux 上的 reader 是例外：
`font-kit` 链接 fontconfig，缺少头文件的机器需要安装
`libfontconfig1-dev` 与 `pkg-config`。

```bash
git clone https://github.com/emfox/sOFD.git
cd sOFD

cargo check --workspace          # 检查整个 workspace
cargo build --release            # 发布构建
cargo test --workspace           # 运行测试
```

## 校验脚本

```bash
python3 scripts/check_all.py             # 本机能跑的全部检查
python3 scripts/check_all.py --quick     # 只跑策略类检查
python3 scripts/check_i18n.py            # 各语言消息键的一致性
python3 scripts/check_language.py        # 仅英文策略与注释密度
python3 scripts/check_publish.py --list  # 哪些 crate 会发布到注册表
python3 scripts/publish_order.py         # 注册表接受的发布顺序
python3 scripts/check_readme.py          # 两版 README 各自声明的数字
```

i18n 检查比对各语言目录：文件集合、键集合、占位符一致，且 Rust 中的
`all_keys()` 列表与 FTL 文件中的键一致。发布检查把 21 个发布、6 个不发布的
两档清单与整个 workspace 对齐，新增 crate 若未声明去向就会失败。
README 检查逐条比对两版：英文版里的每一处计数，与中文版中同一句话写下的
数字必须相等。两版因此不可能各说各的数字——以 `README.md` 为准，
`README.zh-CN.md` 跟随它。

## 贡献

欢迎提交 issue 与 pull request。提交前请确认以下检查通过：

```bash
cargo test --workspace
cargo clippy --workspace -- -D warnings
python3 scripts/check_all.py
```

新代码需要测试。不要引入 `unsafe`；`sofd-ffi` 是唯一有文档记录的例外，
因为 C ABI 必须用到它。代码、注释、提交信息与 issue 一律使用英文：
[docs/Conventions.md](https://github.com/emfox/sOFD/blob/HEAD/docs/Conventions.md)。

性能基线与运行方式见 [`docs/Performance.md`](https://github.com/emfox/sOFD/blob/HEAD/docs/Performance.md)。

## 许可证

[Apache-2.0](https://github.com/emfox/sOFD/blob/HEAD/LICENSE)

阅读器的图标来自 GNOME 项目的 Adwaita symbolic 图标集，按 CC BY-SA 3.0 US 使用，
详见 [NOTICE.md](https://github.com/emfox/sOFD/blob/HEAD/NOTICE.md)。

## 相关资源

- [国家标准全文公开系统](https://openstd.samr.gov.cn/bzgk/gb/index)，可在线预览上述 GB/T 标准
- [行业标准信息服务平台](https://std.samr.gov.cn/hb/)，检索 GM/T 标准
- [GmSSL](https://github.com/guanzhi/GmSSL)，开源的国密算法实现
