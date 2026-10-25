# sOFD 简体中文 · 签章模块
# 注意复数形式：中文无复数变化，但保留 Fluent 选择结构以便其他语言复用同一键。

sign-valid = 有效
sign-invalid = 无效
sign-unverified = 无法验证
sign-untrusted = 签署证书不可信
sign-expired = 证书已过期
sign-integrity-ok = 文档未被篡改
sign-tampered = 文档内容已被修改

# 签名数量（复数）
sign-count = { $count ->
    [0] 无签名
    [one] { $count } 个签名
   *[other] { $count } 个签名
}

sign-signer = 签署人：{ $name }
sign-signed-at = 签署时间：{ $time }
sign-algorithm = 算法：{ $algo }
sign-issuer = 颁发机构：{ $issuer }
sign-export-report = 导出验签报告
sign-panel-title = 签名
sign-valid-until = 证书有效期：{ $from } 至 { $to }
sign-covers = { $count ->
    [0] 签名未覆盖任何文件
   *[other] 签名覆盖 { $count } 个文件
}，{ $loose ->
    [0] 没有文件落在签名范围之外
   *[other] 另有 { $loose } 个文件不在任何签名范围内
}
sign-seal-text = 监制章：{ $text }
sign-seal-details = 印章详情

# Annotations, clause 15. Read apart from the pages they mark up.
annot-panel-title = 注释
annot-count = { $count ->
    [0] 无注释
   *[other] 共 { $count } 条注释
}
annot-none = 本文档没有注释

# Comparison, two revisions of one file. Read over the flattened pages.
compare-panel-title = 版本比较
compare-open-second = 打开用于比较的版本
compare-identical = 两个版本绘制的页面相同
compare-changed = { $count ->
    [0] 没有页面改动
   *[other] 共 { $count } 页有改动
}
compare-page = 第 { $page } 页：{ $change }
