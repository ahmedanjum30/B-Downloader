# Draws the store logo and promo tiles into store\images.
#
#   powershell -ExecutionPolicy Bypass -File scripts\store-images.ps1
#
# Screenshots are taken from the running app and aren't made here.

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
$out = Join-Path (Split-Path $PSScriptRoot -Parent) 'store\images'
New-Item -ItemType Directory -Force $out | Out-Null

$accent = [Drawing.Color]::FromArgb(255, 0x25, 0x63, 0xeb)

function New-Canvas([int]$w, [int]$h) {
    $bmp = New-Object Drawing.Bitmap $w, $h
    $g = [Drawing.Graphics]::FromImage($bmp)
    $g.SmoothingMode = 'AntiAlias'
    $g.TextRenderingHint = 'AntiAliasGridFit'
    $g.InterpolationMode = 'HighQualityBicubic'
    return $bmp, $g
}

function Add-RoundRect($path, [float]$x, [float]$y, [float]$w, [float]$h, [float]$r) {
    $path.AddArc($x, $y, 2 * $r, 2 * $r, 180, 90)
    $path.AddArc($x + $w - 2 * $r, $y, 2 * $r, 2 * $r, 270, 90)
    $path.AddArc($x + $w - 2 * $r, $y + $h - 2 * $r, 2 * $r, 2 * $r, 0, 90)
    $path.AddArc($x, $y + $h - 2 * $r, 2 * $r, 2 * $r, 90, 90)
    $path.CloseFigure()
}

# The app icon: accent rounded square with a download arrow, `size` pixels at (x, y).
function Draw-Icon($g, [float]$x, [float]$y, [float]$size) {
    $s = $size / 24.0
    $path = New-Object Drawing.Drawing2D.GraphicsPath
    Add-RoundRect $path $x $y $size $size (6 * $s)
    $g.FillPath((New-Object Drawing.SolidBrush $accent), $path)
    $pen = New-Object Drawing.Pen ([Drawing.Color]::White), (2.2 * $s)
    $pen.StartCap = 'Round'; $pen.EndCap = 'Round'; $pen.LineJoin = 'Round'
    $p = { param($a, $b) New-Object Drawing.PointF ($x + $a * $s), ($y + $b * $s) }
    $g.DrawLine($pen, (& $p 12 5.5), (& $p 12 14))
    $g.DrawLines($pen, [Drawing.PointF[]]@((& $p 8 10.5), (& $p 12 14.5), (& $p 16 10.5)))
    $g.DrawLines($pen, [Drawing.PointF[]]@((& $p 6 15), (& $p 6 18), (& $p 18 18), (& $p 18 15)))
}

function Draw-Promo([int]$w, [int]$h, [string]$file) {
    $bmp, $g = New-Canvas $w $h
    $rect = New-Object Drawing.Rectangle 0, 0, $w, $h
    $bg = New-Object Drawing.Drawing2D.LinearGradientBrush $rect, ([Drawing.Color]::FromArgb(255, 0x02, 0x06, 0x17)), ([Drawing.Color]::FromArgb(255, 0x0f, 0x17, 0x2a)), 20.0
    $g.FillRectangle($bg, $rect)
    # A soft accent glow behind the icon.
    $glow = New-Object Drawing.Drawing2D.GraphicsPath
    $gs = $h * 1.1
    $glow.AddEllipse([float](-$gs * 0.25), [float](($h - $gs) / 2), [float]$gs, [float]$gs)
    $pgb = New-Object Drawing.Drawing2D.PathGradientBrush $glow
    $pgb.CenterColor = [Drawing.Color]::FromArgb(90, 0x3b, 0x82, 0xf6)
    $pgb.SurroundColors = [Drawing.Color[]]@([Drawing.Color]::FromArgb(0, 0x3b, 0x82, 0xf6))
    $g.FillPath($pgb, $glow)

    # Sized by the smaller of height and width, so narrow tiles fit too.
    $k = [Math]::Min($h, $w / 2.5)
    $icon = $k * 0.42
    $x = ($w - $k * 2.25) / 2
    Draw-Icon $g $x (($h - $icon) / 2) $icon
    $tx = $x + $icon + $k * 0.1
    $title = New-Object Drawing.Font 'Segoe UI', ([float]($k * 0.15)), ([Drawing.FontStyle]::Bold), ([Drawing.GraphicsUnit]::Pixel)
    $sub = New-Object Drawing.Font 'Segoe UI', ([float]($k * 0.068)), ([Drawing.FontStyle]::Regular), ([Drawing.GraphicsUnit]::Pixel)
    $g.DrawString('BDownloader', $title, [Drawing.Brushes]::White, [float]$tx, [float]($h / 2 - $k * 0.2))
    $muted = New-Object Drawing.SolidBrush ([Drawing.Color]::FromArgb(255, 0x94, 0xa3, 0xb8))
    $g.DrawString("Faster downloads over many connections.`nPause and resume any time.", $sub, $muted, [float]($tx + $k * 0.01), [float]($h / 2 + $k * 0.02))
    $bmp.Save((Join-Path $out $file), [Drawing.Imaging.ImageFormat]::Png)
    $g.Dispose(); $bmp.Dispose()
}

# The Microsoft Store's 1:1 box art (its main logo): the icon and the name,
# on the promo tiles' background.
function Draw-BoxArt([int]$size, [string]$file) {
    $bmp, $g = New-Canvas $size $size
    $rect = New-Object Drawing.Rectangle 0, 0, $size, $size
    $bg = New-Object Drawing.Drawing2D.LinearGradientBrush $rect, ([Drawing.Color]::FromArgb(255, 0x02, 0x06, 0x17)), ([Drawing.Color]::FromArgb(255, 0x0f, 0x17, 0x2a)), 45.0
    $g.FillRectangle($bg, $rect)
    $glow = New-Object Drawing.Drawing2D.GraphicsPath
    $glow.AddEllipse([float]($size * 0.1), [float]($size * 0.02), [float]($size * 0.8), [float]($size * 0.8))
    $pgb = New-Object Drawing.Drawing2D.PathGradientBrush $glow
    $pgb.CenterColor = [Drawing.Color]::FromArgb(90, 0x3b, 0x82, 0xf6)
    $pgb.SurroundColors = [Drawing.Color[]]@([Drawing.Color]::FromArgb(0, 0x3b, 0x82, 0xf6))
    $g.FillPath($pgb, $glow)
    $icon = $size * 0.42
    Draw-Icon $g (($size - $icon) / 2) ($size * 0.21) $icon
    $font = New-Object Drawing.Font 'Segoe UI', ([float]($size * 0.085)), ([Drawing.FontStyle]::Bold), ([Drawing.GraphicsUnit]::Pixel)
    $format = New-Object Drawing.StringFormat
    $format.Alignment = 'Center'
    $g.DrawString('B-Downloader', $font, [Drawing.Brushes]::White, (New-Object Drawing.RectangleF 0, ([float]($size * 0.7)), ([float]$size), ([float]($size * 0.15))), $format)
    $bmp.Save((Join-Path $out $file), [Drawing.Imaging.ImageFormat]::Png)
    $g.Dispose(); $bmp.Dispose()
}
Draw-BoxArt 1080 'boxart-1080.png'
Draw-BoxArt 2160 'boxart-2160.png'

# The promo tiles' background: a dark gradient with a soft accent glow at (cx, cy).
function New-Backdrop([int]$w, [int]$h, [float]$cx, [float]$cy, [float]$glowSize) {
    $bmp, $g = New-Canvas $w $h
    $rect = New-Object Drawing.Rectangle 0, 0, $w, $h
    $bg = New-Object Drawing.Drawing2D.LinearGradientBrush $rect, ([Drawing.Color]::FromArgb(255, 0x02, 0x06, 0x17)), ([Drawing.Color]::FromArgb(255, 0x0f, 0x17, 0x2a)), 45.0
    $g.FillRectangle($bg, $rect)
    $glow = New-Object Drawing.Drawing2D.GraphicsPath
    $glow.AddEllipse([float]($cx - $glowSize / 2), [float]($cy - $glowSize / 2), $glowSize, $glowSize)
    $pgb = New-Object Drawing.Drawing2D.PathGradientBrush $glow
    $pgb.CenterColor = [Drawing.Color]::FromArgb(90, 0x3b, 0x82, 0xf6)
    $pgb.SurroundColors = [Drawing.Color[]]@([Drawing.Color]::FromArgb(0, 0x3b, 0x82, 0xf6))
    $g.FillPath($pgb, $glow)
    return $bmp, $g
}

# Microsoft Store 9:16 poster art: the icon and the name, upright.
foreach ($poster in @(@(720, 1080, 'poster-720x1080.png'), @(1440, 2160, 'poster-1440x2160.png'))) {
    $w, $h, $file = $poster
    $bmp, $g = New-Backdrop $w $h ($w / 2) ($h * 0.4) ($w * 0.95)
    $icon = $w * 0.46
    Draw-Icon $g (($w - $icon) / 2) ($h * 0.4 - $icon / 2) $icon
    $font = New-Object Drawing.Font 'Segoe UI', ([float]($w * 0.105)), ([Drawing.FontStyle]::Bold), ([Drawing.GraphicsUnit]::Pixel)
    $format = New-Object Drawing.StringFormat
    $format.Alignment = 'Center'
    $g.DrawString('B-Downloader', $font, [Drawing.Brushes]::White, (New-Object Drawing.RectangleF 0, ([float]($h * 0.62)), ([float]$w), ([float]($h * 0.12))), $format)
    $bmp.Save((Join-Path $out $file), [Drawing.Imaging.ImageFormat]::Png)
    $g.Dispose(); $bmp.Dispose()
}

# Microsoft Store 16:9 "super hero" art. It must not include the product's title.
foreach ($hero in @(@(1920, 1080, 'hero-1920x1080.png'), @(3840, 2160, 'hero-3840x2160.png'))) {
    $w, $h, $file = $hero
    $bmp, $g = New-Backdrop $w $h ($w * 0.5) ($h * 0.5) ($h * 1.2)
    $icon = $h * 0.36
    Draw-Icon $g (($w - $icon) / 2) (($h - $icon) / 2) $icon
    $bmp.Save((Join-Path $out $file), [Drawing.Imaging.ImageFormat]::Png)
    $g.Dispose(); $bmp.Dispose()
}

# Microsoft Store tile icons.
foreach ($size in 150, 71) {
    $bmp, $g = New-Canvas $size $size
    $g.Clear([Drawing.Color]::Transparent)
    $pad = $size * 16 / 128
    Draw-Icon $g $pad $pad ($size - 2 * $pad)
    $bmp.Save((Join-Path $out "icon-$size.png"), [Drawing.Imaging.ImageFormat]::Png)
    $g.Dispose(); $bmp.Dispose()
}

# Store screenshots must be at least 1366x768: the extension window, enlarged
# on the promo background, from store\images\extension-window.png.
$window = Join-Path $out 'extension-window.png'
if (Test-Path $window) {
    $bmp, $g = New-Backdrop 1600 990 800 495 1300
    $src = [Drawing.Image]::FromFile($window)
    $scale = [Math]::Min(1400 / $src.Width, 860 / $src.Height)
    $dw = $src.Width * $scale; $dh = $src.Height * $scale
    $g.DrawImage($src, [float]((1600 - $dw) / 2), [float]((990 - $dh) / 2), [float]$dw, [float]$dh)
    $src.Dispose()
    $bmp.Save((Join-Path $out 'screenshot-2.png'), [Drawing.Imaging.ImageFormat]::Png)
    $g.Dispose(); $bmp.Dispose()
}

foreach ($size in 128, 300) {
    $bmp, $g = New-Canvas $size $size
    $g.Clear([Drawing.Color]::Transparent)
    # Chrome's guideline: 96x96 artwork inside a 128x128 icon; same proportion for the bigger logo.
    $pad = $size * 16 / 128
    Draw-Icon $g $pad $pad ($size - 2 * $pad)
    $name = if ($size -eq 128) { 'icon-128.png' } else { 'logo-300.png' }
    $bmp.Save((Join-Path $out $name), [Drawing.Imaging.ImageFormat]::Png)
    $g.Dispose(); $bmp.Dispose()
}
# The MSIX package's icons (Start menu, taskbar, Store), into installer\msix\Assets.
$assets = Join-Path (Split-Path $PSScriptRoot -Parent) 'installer\msix\Assets'
New-Item -ItemType Directory -Force $assets | Out-Null
foreach ($asset in @(@('Square44x44Logo.png', 44, 44, 0.04), @('Square150x150Logo.png', 150, 150, 0.2), @('StoreLogo.png', 50, 50, 0.04), @('Wide310x150Logo.png', 310, 150, 0.2))) {
    $name, $w, $h, $pad = $asset
    $bmp, $g = New-Canvas $w $h
    $g.Clear([Drawing.Color]::Transparent)
    $size = [Math]::Min($w, $h) * (1 - 2 * $pad)
    Draw-Icon $g (($w - $size) / 2) (($h - $size) / 2) $size
    $bmp.Save((Join-Path $assets $name), [Drawing.Imaging.ImageFormat]::Png)
    $g.Dispose(); $bmp.Dispose()
}
# The taskbar and Start list use these sizes. "Unplated" icons are shown as
# they are; without them, Windows draws the icon on a plate in the accent color.
foreach ($size in 16, 20, 24, 30, 32, 36, 40, 44, 48, 60, 64, 72, 80, 96, 256) {
    $bmp, $g = New-Canvas $size $size
    $g.Clear([Drawing.Color]::Transparent)
    $pad = [Math]::Max(0, [Math]::Floor($size * 0.04))
    Draw-Icon $g $pad $pad ($size - 2 * $pad)
    foreach ($form in 'unplated', 'lightunplated') {
        $bmp.Save((Join-Path $assets "Square44x44Logo.targetsize-${size}_altform-$form.png"), [Drawing.Imaging.ImageFormat]::Png)
    }
    $g.Dispose(); $bmp.Dispose()
}

Draw-Promo 440 280 'promo-small-440x280.png'
Draw-Promo 1400 560 'promo-marquee-1400x560.png'
# Link previews: GitHub's social preview, and the website's og:image.
Draw-Promo 1280 640 'social-1280x640.png'
Get-ChildItem $out | ForEach-Object { "  $($_.Name)" }
