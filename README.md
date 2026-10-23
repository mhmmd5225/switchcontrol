# SwitchControl

Windows ↔ macOS arasında tek klavye ve mouse paylaşımı (ShareMouse benzeri yazılım KVM).

## Hazır dosyalar

Her gönderimde GitHub Actions programı gerçek Mac ve Windows makinelerinde derleyip test eder. Hazır dosyalar: GitHub'da depo > **Actions** > son yeşil çalışma > **Artifacts** (`SwitchControl-macOS`, `SwitchControl-Windows`). `v` ile başlayan bir sürüm etiketi (ör. `v0.2.0`) gönderilince aynı dosyalar **Releases** sayfasına da eklenir.

- **Mac** (yalnızca Apple Silicon / M serisi): zip'i açın. Uygulama yerel imzalı ve internetten indirildiği için macOS ilk açılışı engeller: Finder'da sağ tık > **Aç** deyin ya da Terminal'de `xattr -dr com.apple.quarantine SwitchControl.app` çalıştırın. Her yeni sürümde Girdi İzleme ve Erişilebilirlik izinlerini yeniden verin (eski kaydı silip yenisini ekleyin).
- **Windows**: zip'ten `switchcontrol.exe`'yi çıkarın. "Windows bilgisayarınızı korudu" uyarısı çıkarsa **Ek bilgi > Yine de çalıştır**.

## Pencere

Program argümansız açılınca (ör. Finder'dan çift tıklayınca) bir pencere açar. Pencerede:

- **Diğer bilgisayar**: öteki bilgisayar bu ekranın solunda mı sağında mı.
- **Bağlantı**: bir taraf *Bağlantı bekle*, diğeri *Bağlan* seçer. Bekleyen taraf kendi adresini gösterir, bağlanan taraf bu adresi *Adres* alanına yazar.
- **Başlat / Durdur**: başlık satırı durumu gösterir (bekleniyor, bağlanılıyor, bağlı, klavye ve fare karşı tarafta).
- **Geçiş**: *Kenara değince* hemen geçer; *Kısa itince* / *Uzun itince* imlecin kenara 0,3 / 0,6 saniye itilmesini bekler (kenardaki kaydırma çubuğu ve düğmelerde yanlışlıkla geçmeyi önler).
- **Kenar çubuğu**: imleç diğer bilgisayara açılan kenara yaklaşınca orada parlayan bir çubuk belirir; itildikçe ortadan dolar. Çubuk tıklamaları engellemez.
- **Eşleştirme**: ilk bağlantıda bekleyen tarafın penceresinde 6 haneli PIN çıkar, bağlanan tarafın penceresinde PIN sorulur.
- **Günlük**: ayrıntılı kayıtlar.

Ayarlar *Başlat*'a basınca veri klasöründeki `settings.toml` dosyasına kaydedilir (`--config` ile aynı biçim).

### Mac uygulaması

Mac'te depo klasöründe:

```bash
./scripts/macos-app.sh
open target/release/SwitchControl.app
```

Uygulamayı isterseniz Uygulamalar klasörüne sürükleyin. İlk açılışta pencere eksik izinleri gösterir: *Ayarları aç* ile Sistem Ayarları > Gizlilik ve Güvenlik altında **Girdi İzleme** ve **Erişilebilirlik** listelerine SwitchControl'ü ekleyin, sonra uygulamayı kapatıp yeniden açın.

Uygulama yerel (ad hoc) imzalı olduğundan macOS izinleri o derlemeye bağlar: yeniden derledikten sonra SwitchControl'ü iki listeden silip tekrar ekleyin. Terminalden `switchcontrol ...` ile çalıştırırken izinler Terminal uygulamasına aittir.

## Komut satırı

Programı iki bilgisayarda da çalıştırın. Bir taraf bağlantı bekler, diğeri ona bağlanır. `--side` her makinede **diğer bilgisayarın** hangi tarafta durduğunu söyler.

Windows (Mac sağda duruyor, bağlantı bekliyor):

```bash
switchcontrol --side right
```

Mac (Windows solda duruyor; IP Windows'unki):

```bash
switchcontrol --side left --connect 192.168.1.20
```

İmleci ortak kenardan dışarı itince klavye ve mouse diğer bilgisayara geçer; geri gelince kontrol döner. Ctrl+Alt+Shift+Esc (Mac'te Ctrl+Option+Shift+Esc) imleci her zaman geri getirir.

| Seçenek | Anlamı |
|---|---|
| `--side left\|right` | Diğer bilgisayarın konumu (zorunlu) |
| `--listen ADDR` | Bağlantı bekle (varsayılan `0.0.0.0:24800`) |
| `--connect ADDR` | Diğerine bağlan; port verilmezse 24800 |
| `--name NAME` | Karşıya gösterilen ad (varsayılan bilgisayar adı) |
| `--speed F` | Karşı ekrandaki imleç hızı çarpanı (varsayılan 1.0) |
| `--no-swap` | Ctrl ↔ Cmd/Win değişimini kapat |
| `--edge-delay MS` | Geçmeden önce imlecin kenara bu kadar milisaniye itilmesini bekle (varsayılan 0) |
| `--config FILE` | Aynı ayarları TOML dosyasından oku (`side = "right"`, `connect = "..."`); komut satırı önceliklidir |
| `--pin PIN` | Eşleştirme PIN'i; verilmezse gerektiğinde sorulur |
| `--data-dir DIR` | Sertifika ve eşleşmiş bilgisayarların tutulduğu klasör |
| `--forget-peers` | Tüm eşleşmeleri sil (sonraki bağlantıda yeniden PIN gerekir) |
| `--dry-run` | Yakalama ve enjeksiyon yapma, sadece logla |
| `-v` | Gönderilen ve alınan her mesajı logla |

Windows'tan gelen Ctrl, Mac'te Cmd olur (ve tersi). Mac klavyesiyle Windows'u kullanırken Cmd tek başına basılıp bırakılırsa Windows tuşu gibi davranır (Başlat menüsü); başka bir tuşla birlikte (Cmd+C) Ctrl olarak kalır. Tuşlar fiziksel konumlarıyla taşınır, bu yüzden iki makinede de aynı klavye düzeni (ör. Türkçe Q) seçili olmalı.

### Gizlilik

Kontrol kendi bilgisayarınızdayken program klavyeyi hiç dinlemez; yalnızca kenarı bulmak için imleç konumunu izler ve bunu ağa göndermez. Klavye ve fare yalnızca imleç diğer bilgisayara geçtiğinde yakalanıp şifreli olarak gönderilir.

### Eşleştirme ve güvenlik

İki bilgisayar ilk kez bağlandığında eşleştirilir:

1. Bağlantı bekleyen taraf `Enter this PIN on the other computer: 144 276` gibi 6 haneli bir PIN yazar.
2. Bağlanan taraf PIN'i sorar (ya da `--pin 144276` ile verilir).
3. PIN doğruysa iki taraf da birbirinin sertifikasını kaydeder. Sonraki bağlantılarda PIN sorulmaz.

- Bağlantının tamamı karşılıklı kimlik doğrulamalı TLS 1.3 ile şifrelenir. Her bilgisayar ilk çalıştırmada kendi sertifikasını üretir.
- PIN, SPAKE2 ile doğrulanır. Ağı dinleyen ya da araya giren biri PIN'i çevrimdışı deneme yanılmayla bulamaz; her bağlantı denemesi tek bir tahmindir. Doğrulama ayrıca o TLS oturumuna bağlıdır.
- Yanlış PIN giren taraf hata verip kapanır. Bekleyen taraf 5 yanlış denemeden sonra yeniden başlatılana kadar yeni eşleştirme kabul etmez.
- Eşleşmemiş bir cihaz, PIN'i bilmeden hiçbir girdi gönderemez veya alamaz.
- Veriler Windows'ta `%APPDATA%\SwitchControl`, Mac'te `~/Library/Application Support/SwitchControl` altında tutulur: `cert.der`, `key.der` (özel anahtar) ve `trusted-peers.txt`.

**macOS:** ilk çalıştırmada Sistem Ayarları > Gizlilik ve Güvenlik altında *Erişilebilirlik* ve *Girdi İzleme* izinleri verilmeli.

## Yapı

```
app/               switchcontrol programı: pencere (gui.rs), ana döngü (engine.rs),
                   ayarlar, TLS bağlantısı, PIN eşleştirme
scripts/           macos-app.sh: SwitchControl.app paketini üretir
crates/core/       ekran kenarı, kontrol devri, tuş iletimi (OS'ten bağımsız, testli)
crates/proto/      ortak tipler ve ağ protokolü
crates/keymap/     USB HID ↔ Windows scan code ↔ macOS key code tabloları
crates/platform/   girdi yakalama ve enjeksiyon (win/, mac/)
spikes/win-spike/  Windows API'lerini ve uçtan uca akışı deneyen yardımcı program
```

## Geliştirme

Gereksinimler (Windows): Rust `stable-x86_64-pc-windows-msvc` ve Visual Studio Build Tools ("Desktop development with C++").

```bash
cargo test
```

Testler pencereyi ekransız çalıştırır (egui_kittest): PIN yazarak eşleştirme, durdurma ve PIN'siz yeniden bağlanma dahil. Linux'ta da derlenir ve çalışır; orada girdi yakalama olmadığından pencere deneme modunda açılır.

macOS platform kodunun tip kontrolü (Windows'ta, önce `rustup target add aarch64-apple-darwin`). Uygulamanın tamamı Windows'tan Mac için kontrol edilemez: `ring` C kodu derliyor ve bunun için Apple araçları gerekiyor.

```bash
cargo check -p sc-platform --target aarch64-apple-darwin
```

### Tek bilgisayarda uçtan uca deneme

Gerçek bir örnek ile `--dry-run` bir örneği birbirine bağlayıp imleci kenardan geçirir ve geri getirir. Örnekler iki ayrı bilgisayar gibi davransın diye farklı `--data-dir` kullanır; ilk seferde ikincisi, birincinin yazdığı PIN'i sorar. Deneme sırasında fareye dokunmayın.

```bash
cargo run -p switchcontrol -- --side right --listen 127.0.0.1:24801 --data-dir test-a -v
```

```bash
cargo run -p switchcontrol -- --side left --connect 127.0.0.1:24801 --data-dir test-b --dry-run -v
```

```bash
cargo run -p win-spike -- edge-test
```

### win-spike komutları

- `screens`: monitörleri ve imleç konumunu listeler.
- `selftest`: yakalama ve enjeksiyonu otomatik doğrular. Kendi işaretli olaylarımızın yok sayıldığını ve grab'de imlecin ortada sabit kalıp hareketin fark olarak okunduğunu kontrol eder.
- `capture [--no-moves] [--seconds N] [--grab-after S --grab-for S]`: gerçek girdiyi yazdırır; grab'i deneme imkânı verir.
- `inject --type`: 3 saniye içinde Not Defteri'ne tıklayın. Türkçe Q'da `hello ğüşi`, ABD düzeninde `hello [];'` yazar.
