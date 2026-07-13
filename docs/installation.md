# Kurulum ve Kurtarma

Windows için tam, adım adım ve başlangıç seviyesindeki kurulum anlatımı proje kökündeki [`README.md`](../README.md) dosyasındadır.

## Windows tek komut kurulumu

Kaynak arşivini çıkardıktan sonra PowerShell ile proje klasörüne girin:

```powershell
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\setup.ps1 -Workspace "$HOME\EverythingWorkspace"
```

Normal kurulumda başka bir scripti elle çalıştırmayın. `setup.ps1`, `install.ps1` dosyasını gereken seçeneklerle kendisi çağırır.

Kurulum günlükleri:

```text
%USERPROFILE%\.everything\setup
```

Son aşamayı görüntülemek için:

```powershell
Get-Content "$HOME\.everything\setup\current-stage"
```

## Linux/macOS

```bash
./setup.sh --workspace "$HOME/EverythingWorkspace"
```

## Geliştirici doğrulaması

Windows:

```powershell
.\scripts\verify.ps1
```

Linux/macOS:

```bash
./scripts/verify.sh
```

Doğrulama; Rust biçim/Clippy/test, Electron temiz kurulum/tip kontrolü/derleme/güvenlik denetimi, Python test/paketleme, kurucu sözleşmesi, ürün duman testi ve kaynak arşivi kontrollerini kapsar.

## Geri alma

Windows kurucusu yeni sürümü geçici klasörde hazırlar. Canlı servis ve doktor kontrolleri geçmeden eski kurulumu kalıcı olarak silmez. Geçiş sonrasında hata oluşursa önceki kurulumu geri yüklemeye çalışır.
