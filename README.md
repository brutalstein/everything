# Everything 0.3.0 Apex

Everything; yerel bir yapay zekâ modeli, Rust çalışma zamanı, Electron masaüstü arayüzü, kod grafiği, bellek, otomasyon, araç çalıştırma ve araştırma özelliklerini tek projede birleştiren bir masaüstü geliştirme yardımcısıdır.

Bu sürümün Windows kurulumu **tek ana komut** üzerinden tasarlanmıştır. Normal kullanıcı olarak `setup.ps1` çalıştırılır; kurucu eksik araçları bulur, gerekiyorsa yükler, kaynak biçimini doğrular, projeyi denetler, test eder, derler, Ollama modelini indirir, servisi kurar ve uygulamayı açar.

> **Windows kullanıyorsanız çalıştırmanız gereken ana dosya yalnızca `setup.ps1` dosyasıdır.** Diğer kurulum dosyalarını sırayla elle çalıştırmanız gerekmez.

Yapılan düzeltmelerin ve çalıştırılan testlerin ayrıntılı kaydı `EVERYTHING_0_3_0_VALIDATION_REPORT_TR.md` dosyasındadır. Gerçek Windows kurulumu ve canlı Ollama testi gibi yalnız hedef makinede yapılabilecek kontroller de bu raporda açıkça belirtilmiştir.

---

## 1. En kolay Windows kurulumu

### Gerekenler

- 64 bit Windows 10 veya Windows 11
- Çalışan internet bağlantısı
- En az 15 GiB boş disk alanı
- Kurulum sırasında çıkabilecek Windows yönetici izni penceresinde **Evet** seçebilme

Kurucu yönetici PowerShell'iyle başlatılmak zorunda değildir. Visual C++ Build Tools kurulacağı zaman Windows kendisi bir UAC/yönetici izni penceresi gösterebilir.

### Adım 1 — Projeyi indir

GitHub sayfasında:

1. **Code** düğmesine basın.
2. **Download ZIP** seçeneğine basın.
3. İnen ZIP dosyasını çıkarın.
4. Klasörü mümkünse kısa bir yola taşıyın. Örnek:

```text
C:\Everything
```

Git kullanıyorsanız ZIP yerine şunu da kullanabilirsiniz:

```powershell
git clone https://github.com/KULLANICI_ADI/REPO_ADI.git C:\Everything
```

`KULLANICI_ADI/REPO_ADI` bölümünü kendi GitHub adresinizle değiştirin.

### Adım 2 — PowerShell'i aç

Başlat menüsünde **PowerShell** aratıp açın. Yönetici olarak açmanız şart değildir.

### Adım 3 — Proje klasörüne gir

```powershell
Set-Location C:\Everything
```

Klasörü başka yere çıkardıysanız kendi yolunuzu yazın.

### Adım 4 — Tek komutla kurulumu başlat

```powershell
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\setup.ps1 -Workspace "$HOME\EverythingWorkspace"
```

Bu komutta iki farklı klasör vardır:

- `C:\Everything`: indirdiğiniz kaynak kodun bulunduğu klasördür.
- `$HOME\EverythingWorkspace`: Everything'in üzerinde çalışacağı proje/çalışma klasörüdür.

Everything'i doğrudan bu kaynak kod üzerinde çalıştırmak isterseniz:

```powershell
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\setup.ps1 -Workspace (Get-Location).Path
```

### Adım 5 — Kurulum bitince

Kurucu varsayılan olarak uygulamayı açar. `everything` komutunun PATH'e tamamen yerleşmesi için açık terminalleri kapatıp yeni bir PowerShell açın.

Uygulamayı daha sonra belirli bir çalışma klasörüyle açmak için:

```powershell
everything --workspace "$HOME\EverythingWorkspace"
```

Çalışma zamanı doktorunu çalıştırmak için:

```powershell
everything-cli --workspace "$HOME\EverythingWorkspace" doctor
```

---

## 2. `setup.ps1` tam olarak ne yapıyor?

Kurulum aşağıdaki sırayla ilerler:

1. Windows ve 64 bit işletim sistemi kontrol edilir.
2. RAM, NVIDIA ekran kartı belleği ve boş disk alanı ölçülür.
3. Uygun Qwen2.5-Coder modeli seçilir.
4. Kurulum günlüğü oluşturulur.
5. Visual Studio 2022 C++ Build Tools ve Windows SDK denetlenir; eksikse kurulur.
6. Rustup ve sabitlenmiş Rust `1.97.0` araç zinciri kurulur.
7. `rustfmt` ve `clippy` bileşenleri kurulur.
8. Node.js 22 veya daha yenisi ve npm 10 veya daha yenisi denetlenir; eksikse kurulur.
9. Python 3.11 veya daha yenisi denetlenir; eksikse Python 3.12 kurulur.
10. Ollama denetlenir; eksikse kurulur.
11. `cargo fmt --all -- --check` çalıştırılır.
12. `cargo clippy --locked --workspace --all-targets -- -D warnings` çalıştırılır.
13. `cargo test --locked --workspace --all-targets` çalıştırılır.
14. Rust release derlemesi oluşturulur.
15. Electron bağımlılıkları kilit dosyasından kurulur.
16. Electron çalışma zamanı indirilir, TypeScript denetlenir ve masaüstü uygulaması derlenir.
17. Yalnızca üretim bağımlılıklarında yüksek seviye güvenlik açığı denetimi yapılır.
18. Python SDK geçici ve yalıtılmış bir sanal ortamda test edilir ve paketlenir.
19. Statik ürün duman testi çalıştırılır.
20. Yeni kurulum geçici bir klasörde hazırlanır.
21. Servis gerçek olarak başlatılır ve `/v1/info` üzerinden canlı sağlık kontrolü yapılır.
22. Ollama modeli indirilir ve gerçek model isteğiyle canlı hazırlık testi yapılır.
23. Arka plan servisi kullanıcı Zamanlanmış Görevi olarak kurulur; bu mümkün değilse Başlangıç klasörü kullanılır.
24. İsteğe bağlı yerel SearXNG araştırma yardımcısı denenir.
25. Tam çalışma zamanı doktoru çalıştırılır.
26. Kurulum manifestosu yazılır.
27. Komut dosyaları kullanıcı PATH'ine eklenir.
28. Her şey başarılıysa önceki sürüm silinir ve uygulama açılır.

Bir aşama başarısız olursa kurucu sessizce devam etmez. Hangi aşamada durduğunu yazar, yarım kurulumu bırakmaz ve mümkünse önceki çalışan kurulumu geri yükler.

---

## 3. Otomatik model seçimi

`-Model auto` varsayılandır. Yaklaşık seçim mantığı şöyledir:

| Algılanan sistem | Seçilen model |
|---|---|
| Daha düşük RAM/VRAM veya 22 GiB'dan az boş alan | `qwen2.5-coder:3b` |
| En az 16 GiB RAM veya 8 GiB NVIDIA VRAM ve yeterli disk | `qwen2.5-coder:7b` |
| En az 24 GiB RAM, 16 GiB NVIDIA VRAM ve en az 35 GiB boş alan | `qwen2.5-coder:14b` |

Modeli elle seçmek için:

```powershell
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\setup.ps1 `
  -Workspace "$HOME\EverythingWorkspace" `
  -Model "qwen2.5-coder:7b"
```

Ortam değişkeniyle de seçebilirsiniz:

```powershell
$env:EVERYTHING_MODEL = "qwen2.5-coder:3b"
.\setup.ps1 -Workspace "$HOME\EverythingWorkspace"
```

---

## 4. Kurulum seçenekleri

| Seçenek | Ne işe yarar? |
|---|---|
| `-Workspace "YOL"` | Everything'in çalışacağı proje klasörünü belirler. |
| `-Model "MODEL"` | Otomatik seçim yerine belirli bir Ollama modeli kullanır. |
| `-NoLaunch` | Kurulum bitince masaüstü uygulamasını açmaz. |
| `-NoService` | Arka plan servisini kurmaz. Otomasyonlar pencere kapalıyken çalışmaz. |
| `-NoVerify` | Biçim, Clippy, test ve bazı canlı kontrolleri atlar. Yalnızca sorun ayıklarken kullanılmalıdır. |

Normal kurulumda `-NoVerify` kullanmayın. Bu seçenek kurulum süresini kısaltır fakat GitHub'a gönderilecek bir sürümün gerçekten sağlam olduğunu doğrulamaz.

Örnek:

```powershell
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\setup.ps1 `
  -Workspace "D:\Projeler\BenimProjem" `
  -Model "qwen2.5-coder:3b" `
  -NoLaunch
```

---

## 5. Kurulum sırasında hangi araçlar yükleniyor?

Kurucu önce `winget` kullanmayı dener. `winget` yoksa desteklenen araçları resmî HTTPS adreslerinden indirerek kurmayı dener.

| Araç | Neden gerekiyor? |
|---|---|
| Visual Studio C++ Build Tools + Windows SDK | Rust'ın Windows MSVC hedefini ve SQLite/C bağımlılıklarını derlemek için |
| Rust 1.97.0 | Yerel servisleri ve CLI uygulamasını derlemek için |
| rustfmt | Rust kaynak biçimini doğrulamak için |
| Clippy | Rust kalite ve hata denetimleri için |
| Node.js 22+ ve npm 10+ | Electron masaüstü uygulamasını derlemek için |
| Python 3.11+ | Python SDK testleri, paketleme ve duman testleri için |
| Ollama | Yerel Qwen modelini indirmek ve çalıştırmak için |

Kurucu bir aracı yeni yükledikten sonra sizden yeni terminal açmanızı beklemez; bilinen kurulum yollarını mevcut PowerShell oturumuna ekleyip aynı çalışmada devam eder.

---

## 6. Script dosyaları — hangisini ne zaman çalıştıracağım?

### Normal Windows kullanıcısı için

| Dosya | Kullanım durumu |
|---|---|
| `setup.ps1` | **Ana kurucu. Normalde yalnızca bunu çalıştırın.** |
| `install.ps1` | `setup.ps1` tarafından çağrılan ayrıntılı iç kurucu. Elle çalıştırmanız gerekmez. |
| `bootstrap.ps1` | Yayınlanmış GitHub release ZIP'ini ve SHA-256 dosyasını indirip doğrular, sonra `setup.ps1` çalıştırır. |
| `scripts/verify.ps1` | Geliştirici doğrulaması: Rust, Electron, Python, paketleme ve arşiv testlerini çalıştırır. |
| `package-source.ps1` | Temiz ve deterministik `everything-source.zip` üretir. |
| `scripts/research_sidecar.ps1` | İsteğe bağlı yerel SearXNG yardımcısını başlatır/durdurur. |

### Linux/macOS karşılıkları

| Windows | Linux/macOS |
|---|---|
| `setup.ps1` | `setup.sh` |
| `install.ps1` | `install.sh` |
| `bootstrap.ps1` | `bootstrap.sh` |
| `package-source.ps1` | `package-source.sh` |
| `scripts/verify.ps1` | `scripts/verify.sh` |
| `scripts/research_sidecar.ps1` | `scripts/research_sidecar.sh` |

### Kısaca doğru sıra

Son kullanıcı kurulumu:

```text
1. ZIP'i çıkar
2. PowerShell ile klasöre gir
3. setup.ps1 çalıştır
4. Başka kurulum scripti çalıştırma
```

GitHub'a release hazırlayan geliştirici:

```text
1. setup.ps1 ile tüm bağımlılıkları ve çalışan kurulumu hazırla
2. scripts/verify.ps1 çalıştır
3. package-source.ps1 çalıştır
4. Git commit/push yap
5. v0.3.0 etiketi oluşturup gönder
```

---

## 7. `cargo fmt --check` hatası hakkında

Bu kaynak paketteki Rust dosyaları `rustfmt` ile yeniden biçimlendirilmiştir. Aşağıdaki kontrol temiz geçmelidir:

```powershell
cargo fmt --all -- --check
```

İleride Rust dosyalarını değiştirdikten sonra aynı hata yeniden oluşursa önce otomatik biçimlendirme yapın:

```powershell
cargo fmt --all
```

Ardından kontrol edin:

```powershell
cargo fmt --all -- --check
```

`cargo fmt --check` kaynak kodu değiştirmez; yalnızca fark varsa hata koduyla durur. Kurucunun bu aşamada durması doğrudur, çünkü biçimlendirilmemiş kodun GitHub'a gönderilmesini engeller.

---

## 8. Hata olursa ne yapacağım?

Kurulum ekranındaki son satırlarda üç önemli bilgi görünür:

```text
Aşama : ...
Hata  : ...
Günlük: C:\Users\KULLANICI\.everything\setup\setup-TARIH-SAAT.log
```

Kurulum günlükleri burada tutulur:

```text
%USERPROFILE%\.everything\setup
```

PowerShell ile klasörü açmak için:

```powershell
explorer.exe "$HOME\.everything\setup"
```

Son aşamayı görmek için:

```powershell
Get-Content "$HOME\.everything\setup\current-stage"
```

En yeni günlük dosyasını açmak için:

```powershell
$log = Get-ChildItem "$HOME\.everything\setup\setup-*.log" |
  Sort-Object LastWriteTime -Descending |
  Select-Object -First 1
notepad.exe $log.FullName
```

### Aynı kurulumu tekrar çalıştırabilir miyim?

Evet. Kurucu tekrar çalıştırılabilir şekilde tasarlanmıştır. Zaten kurulu ve uygun sürümde olan araçları yeniden indirmez; eksik veya eski olanları tamamlar. Yarım kalan geçici kurulumları temizler ve çalışan eski sürümü mümkün olduğunca korur.

Aynı komutu tekrar kullanın:

```powershell
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\setup.ps1 -Workspace "$HOME\EverythingWorkspace"
```

### Sık görülen hata türleri

**`cl.exe` veya `link.exe` bulunamadı**
Kurucu Visual C++ Build Tools ve Windows SDK'yı otomatik kurar ve ortamı mevcut oturuma aktarır. Kurulum paketi Windows yeniden başlatması isterse bilgisayarı yeniden başlatıp aynı `setup.ps1` komutunu tekrar çalıştırın.

**Yetersiz disk alanı**
Model dosyaları, `target` klasörü ve Electron bağımlılıkları birlikte ciddi alan kullanabilir. En az 15 GiB, 7B model için tercihen 22 GiB veya daha fazla boş alan bırakın.

**Port kullanımda**
Varsayılan servis portu doluysa kurucu otomatik boş port seçip kaydeder. Portu siz elle sabitlediyseniz farklı değer verin:

```powershell
$env:EVERYTHING_SERVICE_PORT = "3473"
$env:EVERYTHING_OAUTH_PORT = "43822"
.\setup.ps1 -Workspace "$HOME\EverythingWorkspace"
```

**Ollama modeli indirilemiyor**
İnternet bağlantısını ve boş alanı kontrol edin. Sonra aynı kurulum komutunu tekrar çalıştırın. Ollama indirmesi tekrar denenir.

**PowerShell script çalıştırmayı engelliyor**
Dosyaya çift tıklamak yerine şu komutu kullanın:

```powershell
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\setup.ps1 -Workspace "$HOME\EverythingWorkspace"
```

---

## 9. Kurulumdan sonra dosyalar nereye gider?

| İçerik | Varsayılan konum |
|---|---|
| Kurulu Everything | `%LOCALAPPDATA%\Everything` |
| Komut dosyaları | `%LOCALAPPDATA%\Everything\bin` |
| Kullanıcı durumu, bellek ve loglar | `%USERPROFILE%\.everything` |
| Ollama modelleri | `%USERPROFILE%\.ollama` veya `OLLAMA_MODELS` |
| Çalışma alanı | `-Workspace` ile verdiğiniz klasör |
| Kurulum manifestosu | `%LOCALAPPDATA%\Everything\install-manifest.json` |
| Çalışma zamanı doktor raporu | `%LOCALAPPDATA%\Everything\runtime-doctor.json` |

---

## 10. Geliştirici doğrulaması

Önce normal `setup.ps1` kurulumunu tamamlayın. Ardından proje kökünde:

```powershell
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\scripts\verify.ps1
```

Bu doğrulama şunları yapar:

- Rust biçim kontrolü
- Kilitli Clippy denetimi
- Tüm Rust hedeflerinin testleri
- Electron temiz bağımlılık kurulumu
- TypeScript kontrolü
- Electron derlemesi
- Üretim bağımlılığı güvenlik denetimi
- Geçici Python sanal ortamında test ve paketleme
- Statik ürün duman testi
- Deterministik kaynak ZIP'i üretme
- Kaynak ZIP'i doğrulama

Tek tek temel Rust kontrolleri:

```powershell
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace --all-targets
cargo build --release --locked --workspace
```

Electron kontrolleri:

```powershell
Set-Location .\apps\everything-app
npm ci
npm run typecheck
npm run build
npm audit --omit=dev --audit-level=high
Set-Location ..\..
```

Statik kurucu kontrolleri:

```powershell
python .\scripts\check_versions.py
python .\scripts\smoke_installers.py
python .\scripts\smoke_mvp.py --require-built-ui
```

---

## 11. GitHub'a gönderme sırası

Yeni boş bir GitHub deposu oluşturduktan sonra proje kökünde:

```powershell
git init
git add .
git commit -m "Everything 0.3.0 Apex"
git branch -M main
git remote add origin https://github.com/KULLANICI_ADI/REPO_ADI.git
git push -u origin main
```

Depoda zaten `.git` varsa yalnızca:

```powershell
git add .
git commit -m "Kurulum ve doğrulama zincirini sağlamlaştır"
git push
```

Göndermeden önce mutlaka:

```powershell
.\scripts\verify.ps1
```

çalıştırın. GitHub Actions da Windows, Linux ve macOS üzerinde Rust, Electron, Python, kurucu sözleşmesi, sürüm eşleşmesi ve kaynak paket kontrollerini tekrarlar.

---

## 12. GitHub release oluşturma

Kaynak arşivini elle üretmek için:

```powershell
.\package-source.ps1
```

Bu komut proje kökünde şunu oluşturur:

```text
everything-source.zip
```

Arşiv; `.git`, `target`, `node_modules`, sanal ortamlar, yerel veritabanları, loglar ve makineye özel oluşturulmuş dosyaları içermez.

Otomatik GitHub release akışı için doğrulanmış commit'e etiket gönderin:

```powershell
git tag v0.3.0
git push origin v0.3.0
```

`.github/workflows/release.yml` tüm platform kontrollerini yeniden çalıştırır. Başarılı olursa:

- deterministik `everything-source.zip` üretir,
- arşivi tekrar doğrular,
- SHA-256 dosyası üretir,
- GitHub provenance attestation oluşturur,
- GitHub Release yayınlar.

Etiketi yalnızca tüm CI kontrolleri yeşil olduğunda oluşturun.

---

## 13. Release üzerinden tek dosyalık bootstrap kullanımı

`bootstrap.ps1`, yayınlanmış son GitHub release içindeki kaynak ZIP'ini ve SHA-256 dosyasını indirir. Özeti doğruladıktan sonra arşivi geçici klasöre çıkarır ve `setup.ps1` çalıştırır.

Kendi fork/deponuzu kullanacaksanız önce depo adını verin:

```powershell
$env:EVERYTHING_GITHUB_REPOSITORY = "KULLANICI_ADI/REPO_ADI"
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\bootstrap.ps1 `
  -Workspace "$HOME\EverythingWorkspace"
```

Varsayılan depo değeri kaynakta `brutalstein/everything` olarak tanımlıdır. Farklı bir GitHub deposuna yükleyecekseniz ya yukarıdaki ortam değişkenini kullanın ya da `bootstrap.ps1` içindeki varsayılan değeri kendi deponuzla değiştirin.

---

## 14. Projenin ana bileşenleri

| Bileşen | Görevi |
|---|---|
| `apps/everythingd` | Yerel HTTP servisi, zamanlayıcı ve çalışma zamanı |
| `apps/everything-cli` | Komut satırı kontrolü ve doktor komutları |
| `apps/everything-app` | Electron masaüstü arayüzü |
| `crates/everything-runtime` | Planlama, yürütme, bellek, beceri ve otomasyon orkestrasyonu |
| `crates/everything-graph` | Kod grafiği ve artımlı değişiklik takibi |
| `crates/everything-tools` | Komut, dosya yaması ve çalışma alanı güvenlik politikaları |
| `crates/everything-memory` | Kalıcı bellek ve arama |
| `crates/everything-state` | Otomasyon ve çalışma durumu veritabanı |
| `crates/everything-connectors` | Haricî servis bağlayıcıları ve gizli bilgi kasası |
| `crates/everything-research` | Web araştırma sağlayıcıları ve kaynak toplama |
| `crates/everything-skills` | Beceri/eklenti yükleme ve yürütme |
| `crates/everything-verifier` | Sonuç ve değişiklik doğrulama |
| `python/everything_control` | Python kontrol SDK'sı |

---

## 15. Güvenlik ve geri alma davranışı

Kurucu aşağıdaki korumaları uygular:

- Rust bağımlılıklarını `Cargo.lock` ile kilitli derler.
- Electron bağımlılıklarını `npm ci` ile kilit dosyasından kurar.
- Kaynak release indirmelerinde SHA-256 doğrular.
- Ollama doğrudan kurulum paketinde Authenticode yayıncı imzasını denetler.
- Yeni kurulumu önce geçici klasörde oluşturur.
- Canlı daemon ve model testleri geçmeden kurulumu başarılı saymaz.
- Geçiş sonrasında hata olursa önceki kurulumu geri yüklemeye çalışır.
- Servisleri yalnızca loopback (`127.0.0.1`) üzerinde dinletir.
- Yüksek seviye üretim bağımlılığı güvenlik açığında kurulumu durdurur.
- Kurulumun hangi aşamada olduğunu ve tam günlüğünü kullanıcı klasöründe tutar.

---

## 16. Kısa özet

Windows'ta ilk kurulum için gereken komut:

```powershell
Set-Location C:\Everything
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\setup.ps1 -Workspace "$HOME\EverythingWorkspace"
```

GitHub'a göndermeden önce gereken komut:

```powershell
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\scripts\verify.ps1
```

Release için gereken son komutlar:

```powershell
git tag v0.3.0
git push origin v0.3.0
```

## Lisans

MIT — ayrıntılar için `LICENSE` dosyasına bakın.
