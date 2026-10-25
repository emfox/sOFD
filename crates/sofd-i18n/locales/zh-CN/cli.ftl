# sOFD 简体中文 · 命令行输出

cli-pages = 页数：{ $count }
cli-page-size = 页面尺寸：{ $w } x { $h } 毫米
cli-version = 版本：{ $version }
cli-embedded-fonts = 内嵌字体：{ $count }
cli-font-substituted = 字体：{ $count } 个未内嵌，已用替代字体绘制（{ $names }）
cli-font-missing = 警告：{ $count } 个字体（{ $names }）未内嵌且本机未安装，也找不到能覆盖其文字的替代字体，这些文字未绘制到页面
cli-missing-images = 警告：{ $count } 个图片无法绘制，页面上不会出现
cli-missing-glyphs = 警告：{ $count } 个字符在任何可用字体中都没有字形，页面上不会出现
cli-signatures = 签名：{ $count }
cli-no-pages = 该文档未声明任何页面
cli-wrote = 已写入 { $path }
cli-rendered-page = 已渲染第 { $page } 页，{ $dpi } dpi
cli-converted = 已将 { $count } 页转换为 { $format }
cli-search-hit = 第 { $page } 页：{ $snippet }
cli-audit-clean = 未发现归档问题
cli-audit-finding = { $code } [{ $severity }] { $location }：{ $detail }
cli-print-plan = { $pages } 页，共 { $sheets } 张纸，{ $paper }，{ $dpi } dpi
cli-sign-entry = { $loc }：{ $status }
cli-sign-none = 该文档未声明任何签名
cli-cert-validity = 证书有效期：{ $from } 至 { $to }
cli-sign-provider = 签署工具：{ $name }
cli-sign-coverage = 签名覆盖 { $count } 个文件，另有 { $loose } 个文件不在任何签名范围内
cli-sign-stamps = 印章位置：{ $list }
cli-sign-seal = 监制章：{ $text }
cli-error = 错误：{ $message }
