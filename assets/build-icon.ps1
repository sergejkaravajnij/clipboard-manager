param([string]$Source = 'icon.png', [string]$Output = 'app.ico')
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
$sourceImage = [System.Drawing.Image]::FromFile((Join-Path $PSScriptRoot $Source))
$sizes = @(16,20,24,32,48,64,128,256)
$frames = @()
try {
    foreach ($size in $sizes) {
        $bitmap = [System.Drawing.Bitmap]::new($size,$size,[System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
        $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
        $graphics.Clear([System.Drawing.Color]::Transparent)
        $graphics.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
        $graphics.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
        $graphics.DrawImage($sourceImage,[System.Drawing.Rectangle]::new(0,0,$size,$size))
        $graphics.Dispose()
        $memory = [IO.MemoryStream]::new()
        $bitmap.Save($memory,[System.Drawing.Imaging.ImageFormat]::Png)
        $frames += ,$memory.ToArray()
        $memory.Dispose()
        $bitmap.Dispose()
    }
    $stream = [IO.File]::Create((Join-Path $PSScriptRoot $Output))
    $writer = [IO.BinaryWriter]::new($stream)
    try {
        $writer.Write([uint16]0); $writer.Write([uint16]1); $writer.Write([uint16]$sizes.Count)
        $offset = 6 + 16 * $sizes.Count
        for ($index=0; $index -lt $sizes.Count; $index++) {
            $dimension = if ($sizes[$index] -eq 256) {0} else {$sizes[$index]}
            $writer.Write([byte]$dimension); $writer.Write([byte]$dimension)
            $writer.Write([byte]0); $writer.Write([byte]0)
            $writer.Write([uint16]1); $writer.Write([uint16]32)
            $writer.Write([uint32]$frames[$index].Length); $writer.Write([uint32]$offset)
            $offset += $frames[$index].Length
        }
        foreach ($frame in $frames) { $writer.Write([byte[]]$frame) }
    } finally { $writer.Dispose(); $stream.Dispose() }
} finally { $sourceImage.Dispose() }
