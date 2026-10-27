# Draws the app icon into crates\app\assets:
#   bdownloader.ico  - for the exe, shortcuts and the installer (16 to 256 px)
#   icon-64.rgba     - raw 64x64 RGBA pixels for the window's title bar and taskbar
#
#   powershell -ExecutionPolicy Bypass -File scripts\app-icon.ps1

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
$out = Join-Path (Split-Path $PSScriptRoot -Parent) 'crates\app\assets'
New-Item -ItemType Directory -Force $out | Out-Null

function Draw-Icon([int]$size) {
    $bmp = New-Object Drawing.Bitmap $size, $size, ([Drawing.Imaging.PixelFormat]::Format32bppArgb)
    $g = [Drawing.Graphics]::FromImage($bmp)
    $g.SmoothingMode = 'AntiAlias'
    $g.Clear([Drawing.Color]::Transparent)
    # Small sizes fill the square; larger ones keep a little margin like other Windows icons.
    $pad = if ($size -le 32) { 0 } else { $size / 32 }
    $box = $size - 2 * $pad
    $s = $box / 24.0
    $r = 6 * $s
    $path = New-Object Drawing.Drawing2D.GraphicsPath
    $x = $pad; $y = $pad; $w = $box - 1
    $path.AddArc($x, $y, 2 * $r, 2 * $r, 180, 90)
    $path.AddArc($x + $w - 2 * $r, $y, 2 * $r, 2 * $r, 270, 90)
    $path.AddArc($x + $w - 2 * $r, $y + $w - 2 * $r, 2 * $r, 2 * $r, 0, 90)
    $path.AddArc($x, $y + $w - 2 * $r, 2 * $r, 2 * $r, 90, 90)
    $path.CloseFigure()
    $g.FillPath((New-Object Drawing.SolidBrush ([Drawing.Color]::FromArgb(255, 0x25, 0x63, 0xeb))), $path)
    $pen = New-Object Drawing.Pen ([Drawing.Color]::White), ([Math]::Max(1.6, 2.3 * $s))
    $pen.StartCap = 'Round'; $pen.EndCap = 'Round'; $pen.LineJoin = 'Round'
    $p = { param($a, $b) New-Object Drawing.PointF ($x + $a * $s), ($y + $b * $s) }
    $g.DrawLine($pen, (& $p 12 5.5), (& $p 12 14))
    $g.DrawLines($pen, [Drawing.PointF[]]@((& $p 8 10.5), (& $p 12 14.5), (& $p 16 10.5)))
    $g.DrawLines($pen, [Drawing.PointF[]]@((& $p 6 15), (& $p 6 18), (& $p 18 18), (& $p 18 15)))
    $g.Dispose()
    return $bmp
}

# An .ico holding one PNG per size (supported since Windows Vista).
$sizes = 16, 20, 24, 32, 40, 48, 64, 128, 256
$pngs = foreach ($size in $sizes) {
    $bmp = Draw-Icon $size
    $ms = New-Object IO.MemoryStream
    $bmp.Save($ms, [Drawing.Imaging.ImageFormat]::Png)
    $bmp.Dispose()
    , $ms.ToArray()
}
$ico = New-Object IO.MemoryStream
$bw = New-Object IO.BinaryWriter $ico
$bw.Write([uint16]0); $bw.Write([uint16]1); $bw.Write([uint16]$sizes.Count)
$offset = 6 + 16 * $sizes.Count
for ($i = 0; $i -lt $sizes.Count; $i++) {
    $dim = if ($sizes[$i] -ge 256) { 0 } else { $sizes[$i] }
    $bw.Write([byte]$dim); $bw.Write([byte]$dim); $bw.Write([byte]0); $bw.Write([byte]0)
    $bw.Write([uint16]1); $bw.Write([uint16]32)
    $bw.Write([uint32]$pngs[$i].Length); $bw.Write([uint32]$offset)
    $offset += $pngs[$i].Length
}
foreach ($png in $pngs) { $bw.Write([byte[]]$png) }
$bw.Flush()
[IO.File]::WriteAllBytes((Join-Path $out 'bdownloader.ico'), $ico.ToArray())

# Raw RGBA (GDI+ stores BGRA, so swap red and blue).
$bmp = Draw-Icon 64
$rgba = New-Object byte[] (64 * 64 * 4)
$i = 0
for ($y = 0; $y -lt 64; $y++) {
    for ($x = 0; $x -lt 64; $x++) {
        $c = $bmp.GetPixel($x, $y)
        $rgba[$i] = $c.R; $rgba[$i + 1] = $c.G; $rgba[$i + 2] = $c.B; $rgba[$i + 3] = $c.A
        $i += 4
    }
}
$bmp.Dispose()
[IO.File]::WriteAllBytes((Join-Path $out 'icon-64.rgba'), $rgba)
Get-ChildItem $out | ForEach-Object { "  $($_.Name)  $($_.Length) bytes" }
