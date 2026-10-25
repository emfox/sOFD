# sOFD English (US) · Error messages
# Design: errors originate from structured enums; parameters are filled in by the
# presentation layer, so word order can be rearranged per language.

err-not-ofd = This file is not a valid OFD document
err-password-required = The document is encrypted. Please enter the password.
err-page-out-of-range = Page index out of range: page { $index } (of { $total })
err-package-corrupted = Document container is corrupted: { $reason }
err-font-missing = Font "{ $family }" is missing; text may not match the original
err-dangling-reference = The document references a missing resource: { $kind } (ID: { $id })
err-resource-limit = Resource limit exceeded: { $detail }
err-io = File I/O failed: { $detail }
err-unsupported = Not supported: { $feature }
