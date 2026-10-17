$WshShell = New-Object -ComObject WScript.Shell
$Shortcut = $WshShell.CreateShortcut("C:\Users\Jorks\Desktop\SpectraScanner.lnk")
$Shortcut.TargetPath = "C:\Users\Jorks\Desktop\ownscanner\spectrascanner.exe"
$Shortcut.WorkingDirectory = "C:\Users\Jorks\Desktop\ownscanner"
$Shortcut.Description = "SpectraScanner - High-Speed Multi-VFO SDR Scanner"
$Shortcut.Save()
Write-Host "Shortcut created on Desktop successfully!"
