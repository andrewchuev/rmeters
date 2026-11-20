# Generates app_icon.ico (multi-size, PNG frames) and app_icon.png (1024 px).
# Design: dark full-bleed rounded tile with three bars in the tray icon colors
# (CPU blue, RAM green, disk violet). Run from the repository root:
#   powershell -ExecutionPolicy Bypass -File scripts\make-icon.ps1
Add-Type -AssemblyName System.Drawing

$bg = [System.Drawing.Color]::FromArgb(255, 0x1B, 0x1F, 0x24)
$barColors = @(
    [System.Drawing.Color]::FromArgb(255, 0x00, 0x99, 0xFF), # CPU
    [System.Drawing.Color]::FromArgb(255, 0x19, 0xCC, 0x33), # RAM
    [System.Drawing.Color]::FromArgb(255, 0xCC, 0x66, 0xFF)  # disk
)

# Master geometry on a 64-unit grid.
$barX = @(7, 25, 43); $barW = 14; $barBottom = 58
$barH = @(36, 26, 48); $barRadius = 3; $tileRadius = 12

# Pixel-snapped layouts for small sizes, where scaled geometry would blur:
# left margin, bar width, gap, bottom margin, bar heights.
$small = @{
    16 = @{ M = 1; W = 4; Gap = 1; Bottom = 1; H = @(9, 7, 12) }
    20 = @{ M = 2; W = 4; Gap = 2; Bottom = 2; H = @(10, 8, 14) }
    24 = @{ M = 3; W = 5; Gap = 2; Bottom = 2; H = @(13, 9, 18) }
    32 = @{ M = 4; W = 6; Gap = 3; Bottom = 3; H = @(18, 13, 24) }
    48 = @{ M = 5; W = 10; Gap = 4; Bottom = 4; H = @(27, 20, 36) }
}

function New-RoundedRect([single]$x, [single]$y, [single]$w, [single]$h, [single]$r, [bool]$bottomRound = $true) {
    $p = New-Object System.Drawing.Drawing2D.GraphicsPath
    $d = 2 * $r
    $p.AddArc($x, $y, $d, $d, 180, 90)
    $p.AddArc($x + $w - $d, $y, $d, $d, 270, 90)
    if ($bottomRound) {
        $p.AddArc($x + $w - $d, $y + $h - $d, $d, $d, 0, 90)
        $p.AddArc($x, $y + $h - $d, $d, $d, 90, 90)
    } else {
        $p.AddLine($x + $w, $y + $h, $x, $y + $h)
    }
    $p.CloseFigure()
    $p
}

function New-IconBitmap([int]$s) {
    $bmp = New-Object System.Drawing.Bitmap $s, $s, ([System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
    $g.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::Half
    $g.Clear([System.Drawing.Color]::Transparent)

    $k = $s / 64.0
    $tileR = [Math]::Max(2, $tileRadius * $k)
    $tile = New-RoundedRect 0 0 $s $s $tileR
    $brush = New-Object System.Drawing.SolidBrush $bg
    $g.FillPath($brush, $tile)
    $brush.Dispose(); $tile.Dispose()

    # Bars are clipped to the tile so their bottom corners follow its rounding.
    $clip = New-RoundedRect 0 0 $s $s $tileR
    $g.SetClip($clip)

    for ($i = 0; $i -lt 3; $i++) {
        $b = New-Object System.Drawing.SolidBrush $barColors[$i]
        if ($small.ContainsKey($s)) {
            $l = $small[$s]
            $x = $l.M + $i * ($l.W + $l.Gap)
            $h = $l.H[$i]
            $y = $s - $l.Bottom - $h
            if ($s -ge 32) {
                # Integer edges stay crisp; only the rounded tops are antialiased.
                $path = New-RoundedRect $x $y $l.W $h ($barRadius * $k) $false
                $g.FillPath($b, $path)
                $path.Dispose()
            } else {
                $g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::None
                $g.FillRectangle($b, $x, $y, $l.W, $h)
                $g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
            }
        } else {
            $x = $barX[$i] * $k; $w = $barW * $k
            $h = $barH[$i] * $k; $y = $barBottom * $k - $h
            $path = New-RoundedRect $x $y $w $h ($barRadius * $k) $false
            $g.FillPath($b, $path)
            $path.Dispose()
        }
        $b.Dispose()
    }

    $g.ResetClip(); $clip.Dispose(); $g.Dispose()
    $bmp
}

function Get-PngBytes($bmp) {
    $ms = New-Object System.IO.MemoryStream
    $bmp.Save($ms, [System.Drawing.Imaging.ImageFormat]::Png)
    $bytes = $ms.ToArray(); $ms.Dispose()
    , $bytes
}

# Classic DIB icon frame: BITMAPINFOHEADER (double height), bottom-up 32-bit BGRA
# pixels, then an all-zero 1-bit AND mask (transparency comes from alpha).
# Used for frames below 256 px, which some readers do not accept as PNG.
function Get-DibBytes($bmp) {
    $s = $bmp.Width
    $rect = New-Object System.Drawing.Rectangle 0, 0, $s, $s
    $data = $bmp.LockBits($rect, [System.Drawing.Imaging.ImageLockMode]::ReadOnly,
        [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
    $stride = $data.Stride
    $pixels = New-Object byte[] ($stride * $s)
    [System.Runtime.InteropServices.Marshal]::Copy($data.Scan0, $pixels, 0, $pixels.Length)
    $bmp.UnlockBits($data)

    $ms = New-Object System.IO.MemoryStream
    $w = New-Object System.IO.BinaryWriter $ms
    $w.Write([uint32]40); $w.Write([int32]$s); $w.Write([int32](2 * $s))
    $w.Write([uint16]1); $w.Write([uint16]32); $w.Write([uint32]0)
    $w.Write([uint32]($s * $s * 4)); $w.Write([int32]0); $w.Write([int32]0)
    $w.Write([uint32]0); $w.Write([uint32]0)
    for ($y = $s - 1; $y -ge 0; $y--) { $w.Write($pixels, $y * $stride, $s * 4) }
    $maskRow = [int]([Math]::Ceiling($s / 32.0) * 4)
    $w.Write((New-Object byte[] ($maskRow * $s)))
    $w.Flush()
    $bytes = $ms.ToArray(); $w.Dispose()
    , $bytes
}

$sizes = @(16, 20, 24, 32, 40, 48, 64, 128, 256)
$frames = foreach ($s in $sizes) {
    $bmp = New-IconBitmap $s
    $payload = if ($s -ge 256) { Get-PngBytes $bmp } else { Get-DibBytes $bmp }
    $bmp.Dispose()
    , @($s, $payload)
}

# ICO container: ICONDIR, one ICONDIRENTRY per frame, then the frame payloads.
$out = New-Object System.IO.MemoryStream
$wr = New-Object System.IO.BinaryWriter $out
$wr.Write([uint16]0); $wr.Write([uint16]1); $wr.Write([uint16]$frames.Count)
$offset = 6 + 16 * $frames.Count
foreach ($f in $frames) {
    $s = $f[0]; $len = $f[1].Length
    $dim = if ($s -ge 256) { 0 } else { $s }
    $wr.Write([byte]$dim); $wr.Write([byte]$dim); $wr.Write([byte]0); $wr.Write([byte]0)
    $wr.Write([uint16]1); $wr.Write([uint16]32); $wr.Write([uint32]$len); $wr.Write([uint32]$offset)
    $offset += $len
}
foreach ($f in $frames) { $wr.Write([byte[]]$f[1]) }
$wr.Flush()
[System.IO.File]::WriteAllBytes((Join-Path (Get-Location) 'app_icon.ico'), $out.ToArray())
$wr.Dispose()

$big = New-IconBitmap 1024
$big.Save((Join-Path (Get-Location) 'app_icon.png'), [System.Drawing.Imaging.ImageFormat]::Png)
$big.Dispose()

"Wrote app_icon.ico ($($sizes -join ', ') px) and app_icon.png (1024 px)"
