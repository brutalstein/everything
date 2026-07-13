# Everything 0.3.0 Apex — Teslimat ve Doğrulama Raporu

Tarih: 13 Temmuz 2026

## Sonuç

Kaynak paket GitHub'a gönderilmeye uygun hâle getirildi. Windows ana kurucusu `setup.ps1`; eksik araçları kuracak, sabit araç zincirlerini hazırlayacak, tüm kalite kapılarını çalıştıracak, uygulamayı atomik olarak kuracak ve ayrıntılı günlük bırakacak biçimde sağlamlaştırıldı.

En önemli kullanıcı hatası olan `cargo fmt --check` problemi giderildi. Rust kaynaklarının tamamı `rustfmt` ile biçimlendirildi ve biçim kontrolü temiz geçti.

## Düzeltilen kritik sorunlar

- 45 Rust kaynak dosyasındaki biçim farkları giderildi.
- Güncel olmayan `Cargo.lock` düzeltildi.
- Eksik test/çalışma zamanı bağımlılıkları eklendi.
- Clippy'yi durduran uyarılar ve platforma özel ölü kod sorunları giderildi.
- Bellek, skill ve state SQLite sorgularında satır birleştirmesinden doğan `updatedON` benzeri geçersiz SQL üretimi düzeltildi.
- Ollama ikilisi bulunmadığında runtime doctor'ın çökmesi engellendi; artık fallback/degraded raporu üretir.
- `everything-cli --version` ve `everythingd --version` desteği eklendi.
- Windows kurucusuna Visual Studio C++ Build Tools ve Windows SDK otomatik denetimi/kurulumu eklendi.
- Kurulan Rust, Node, npm, Python ve Ollama yollarının aynı PowerShell oturumunda bulunması sağlandı.
- Rust araç zinciri `1.97.0`, Node alt sınırı `22`, npm alt sınırı `10` olarak sabitlendi.
- Python yorumlayıcısı Windows Store takma adlarına takılmadan bulunacak şekilde düzeltildi.
- Ollama Windows kurucusunun çocuk süreçleri nedeniyle sonsuza kadar bekleme ihtimali giderildi.
- Kurulum atomik geri alma, port seçimi, kalıcı servis, günlük ve manifest akışları güçlendirildi.
- Windows ve Unix doğrulama scriptleri aynı temel kalite kapılarına getirildi.
- GitHub CI/release akışlarına Windows PowerShell sözdizimi, kurucu sözleşmesi, release derlemesi ve statik sözleşme kontrolleri eklendi.
- Türkçe, sıfırdan başlayan kullanıcıya yönelik README baştan yazıldı.

## Bu ortamda geçen kalite kapıları

### Rust

- `cargo fmt --all -- --check`: **başarılı**
- `cargo clippy --locked --workspace --all-targets -- -D warnings`: **başarılı, 0 uyarı**
- `cargo test --locked --workspace --all-targets`: **97 başarılı, 0 başarısız**
- `cargo build --locked --workspace --release`: **başarılı**
- Rust statik sözleşme kontrolü: **65 kaynak dosyası başarılı**
- Release CLI sürüm kontrolü: **Everything 0.3.0**
- Release daemon sürüm kontrolü: **everythingd 0.3.0**
- Release daemon `/v1/info` canlı sağlık kontrolü: **başarılı**
- Ollama kurulu değilken runtime doctor: **çökmeden degraded raporu, 10 kontrol, 0 failed**

Bu konteynerde hazır bulunan derleyici Rust `1.88.0` idi. Proje ve GitHub Actions gerçek yayın kapısı için Rust `1.97.0` kullanacak şekilde sabitlenmiştir. Konteynerin ağır bundled-SQLite C optimizasyonu süre sınırına takıldığı için yerel release doğrulamasında C bağımlılıkları `CFLAGS=-O0` ile derlendi; Rust kodu yine Cargo `release` profilinde optimize edildi. GitHub CI varsayılan derleyici bayraklarıyla yeniden derler.

### Electron

- `npm ci`: **başarılı**
- TypeScript kontrolü: **başarılı**
- Electron production build: **başarılı**
- `npm audit --omit=dev --audit-level=high`: **0 vulnerability**
- Node/npm engine bilgisi hem `package.json` hem `package-lock.json` içinde eşleşiyor.

### Python

- Temiz ve geçici sanal ortamda kurulum: **başarılı**
- Pytest: **29 başarılı, 0 başarısız**
- Wheel üretimi: **başarılı**
- Source distribution üretimi: **başarılı**

### Kurucu ve paketleme

- Proje sürüm eşleşmesi: **0.3.0 tüm yüzeylerde aynı**
- Windows/Unix kurucu sözleşme testi: **başarılı**
- Bash sözdizimi: **başarılı**
- Statik ürün smoke: **başarılı**
- PowerShell brace/here-string/güvenli çağrı sözleşmesi: **başarılı**
- Deterministik kaynak ZIP üretimi ve yasaklı dosya denetimi: **başarılı**

## Bu Linux konteynerinde çalıştırılamayan kapılar

- Gerçek bir Windows makinesinde UAC açılması, `winget`, Visual Studio Build Tools, Windows SDK, Zamanlanmış Görev ve Electron pencere açılışı uçtan uca çalıştırılamadı.
- Windows PowerShell'in gerçek parser kontrolü bu konteynerde PowerShell bulunmadığı için yerelde çalıştırılamadı. `.github/workflows/ci.yml` içindeki `windows-latest` işi `scriptblock::Create` ile bütün PowerShell dosyalarını parse eder.
- Ollama ikilisi ve model dosyaları bu konteynerde bulunmadığından gerçek model indirme/inference testi çalıştırılmadı. Kurucu hedef Windows makinesinde Ollama'yı kurar, modeli çeker ve canlı model smoke'u geçmeden başarı bildirmez.
- Gerçek GitHub/OAuth sağlayıcı kimlik bilgileri olmadığı için dış servis mutasyon testleri çalıştırılmadı.

Bu sınırlamalar saklanmamıştır; GitHub Actions matrisi Windows, Linux ve macOS üzerinde kalan platform kapılarını yayın öncesinde zorunlu olarak çalıştırır.

## Yayın kapısı

`v0.3.0` etiketi yalnız GitHub Actions'ın tüm işletim sistemi işlerinde yeşil sonuç vermesinden sonra gönderilmelidir. Release workflow:

1. Rust format, Clippy, test ve release derlemesini çalıştırır.
2. Electron temiz kurulum, tip kontrolü, build ve production audit çalıştırır.
3. Python test ve paket üretimini çalıştırır.
4. Kurucu, statik ürün ve sürüm sözleşmelerini doğrular.
5. PowerShell dosyalarını Windows üzerinde gerçek parser ile denetler.
6. Kaynak ZIP'ini iki kez üretip byte-for-byte deterministik olduğunu doğrular.
7. SHA-256 ve provenance attestation üretir.
8. Yalnız bütün kapılar başarılıysa GitHub Release yayınlar.
