# Draws the store logo and promo tiles into store\images.
#
#   powershell -ExecutionPolicy Bypass -File scripts\store-images.ps1
#
# Screenshots are taken from the running app and aren't made here.

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
$out = Join-Path (Split-Path $PSScriptRoot -Parent) 'store\images'
New-Item -ItemType Directory -Force $out | Out-Null

$accent = [Drawing.Color]::FromArgb(255, 0x6e, 0x7b, 0xff)

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
    $bg = New-Object Drawing.Drawing2D.LinearGradientBrush $rect, ([Drawing.Color]::FromArgb(255, 0x14, 0x17, 0x1d)), ([Drawing.Color]::FromArgb(255, 0x23, 0x26, 0x4a)), 20.0
    $g.FillRectangle($bg, $rect)
    # A soft accent glow behind the icon.
    $glow = New-Object Drawing.Drawing2D.GraphicsPath
    $gs = $h * 1.1
    $glow.AddEllipse([float](-$gs * 0.25), [float](($h - $gs) / 2), [float]$gs, [float]$gs)
    $pgb = New-Object Drawing.Drawing2D.PathGradientBrush $glow
    $pgb.CenterColor = [Drawing.Color]::FromArgb(90, 0x6e, 0x7b, 0xff)
    $pgb.SurroundColors = [Drawing.Color[]]@([Drawing.Color]::FromArgb(0, 0x6e, 0x7b, 0xff))
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
    $muted = New-Object Drawing.SolidBrush ([Drawing.Color]::FromArgb(255, 0xb4, 0xba, 0xcc))
    $g.DrawString("Faster downloads over many connections.`nPause and resume any time.", $sub, $muted, [float]($tx + $k * 0.01), [float]($h / 2 + $k * 0.02))
    $bmp.Save((Join-Path $out $file), [Drawing.Imaging.ImageFormat]::Png)
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
Draw-Promo 440 280 'promo-small-440x280.png'
Draw-Promo 1400 560 'promo-marquee-1400x560.png'
Get-ChildItem $out | ForEach-Object { "  $($_.Name)" }
