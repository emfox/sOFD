# sOFD English (US) · Reader module

reader-open = Open File
reader-close = Close Document
reader-drop-hint = Drop an .ofd file here, or use "Open File"
# Shown on Android, which has no drop target: the way in is the open button,
# which asks the system's own picker for the document.
reader-open-hint = Open an .ofd file with the "Open File" button
reader-info = Document: { $name }
reader-page-of = Page { $current } of { $total }
reader-zoom = Zoom { $percent }%
reader-fit-width = Fit Width
reader-fit-page = Fit Page
reader-layout-single = Single Page
reader-layout-double = Facing Pages
reader-layout-continuous = Continuous
reader-blank-page = This page is blank
reader-short-glyphs = { $count } glyph(s) on this page could not be drawn
reader-short-fonts = { $count } text run(s) are missing their font
reader-short-images = { $count } image(s) on this page could not be drawn
reader-error-label = This page could not be drawn
reader-search-panel-title = Results
reader-search-title = Search
reader-no-match = No matches found
reader-export = Export
reader-export-pdf = Export as PDF
reader-export-png = Export as PNG
reader-export-pdf-done = Wrote { $count ->
    [one] { $count } page
   *[other] { $count } pages
} as PDF: { $path }
reader-export-png-done = Wrote { $count ->
    [one] { $count } PNG
   *[other] { $count } PNG files
}: { $path }
reader-export-pdf-title = Export PDF
reader-export-png-title = Choose a folder for the PNG files
reader-print = Print
reader-print-done = Printed { $pages ->
    [one] { $pages } page
   *[other] { $pages } pages
} on { $sheets ->
    [one] { $sheets } sheet
   *[other] { $sheets } sheets
}, { $paper }
reader-paper-a3 = A3
reader-paper-a4 = A4
reader-paper-a5 = A5
reader-paper-b5 = B5
reader-paper-letter = Letter
reader-paper-legal = Legal
reader-paper-document = Document paper
reader-settings = Settings
reader-main-menu = Main Menu
reader-main-menu-hint = Settings, language and about
reader-keyboard-shortcuts = Keyboard Shortcuts
reader-about = About sOFD { $version }
reader-views = View
reader-panel-toggle = Results
reader-language = Language
reader-theme = Theme
reader-theme-system = Follow System
reader-theme-light = Light
reader-theme-dark = Dark
reader-text-size = Text Size
reader-text-size-normal = Normal ({ $points } pt)
reader-text-size-large = Large ({ $points } pt)
reader-text-size-larger = Largest ({ $points } pt)
reader-high-contrast = High Contrast
reader-high-contrast-hint = Sharpen text and status colours
reader-prev-page = Previous Page
reader-next-page = Next Page
reader-prev-hit = Previous Match
reader-next-hit = Next Match
reader-prev-annotation = Previous Annotation
reader-next-annotation = Next Annotation
reader-close-hint = Close the document
reader-verdict-valid = Valid
reader-verdict-tampered = Tampered
reader-verdict-unverified = Unverified
reader-verdict-untrusted = Untrusted
