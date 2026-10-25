# sOFD 简体中文 · 错误信息
# 设计原则：错误条目由「结构化错误枚举」驱动，
# 参数在展示层填入，因此同一错误可被任意语言重排语序。

err-not-ofd = 该文件不是合法的 OFD 文档
err-password-required = 文档已加密，请输入密码
err-page-out-of-range = 页索引越界：第 { $index } 页（共 { $total } 页）
err-package-corrupted = 文档容器已损坏：{ $reason }
err-font-missing = 缺少字体「{ $family }」，文字显示可能与原文不一致
err-dangling-reference = 文档引用了不存在的资源：{ $kind }（ID: { $id }）
err-resource-limit = 超出资源限制：{ $detail }
err-io = 文件读写失败：{ $detail }
err-unsupported = 暂不支持该特性：{ $feature }
