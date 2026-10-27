# Deneme yönergesi

İki bilgisayarda (Windows ve Mac) adım adım deneme listesi. Her adımın yanında beklenen sonuç var. Beklenenden farklı bir şey olursa adım numarasını, ne gördüğünüzü (ekran görüntüsü) ve penceredeki **Ayarlar > Günlük** içeriğini gönderin.

Kısaltmalar: **W** = Windows bilgisayar, **M** = Mac.

## 0. İndirme

1. GitHub'da depo > **Actions** > en üstteki yeşil (✓) çalışma > sayfanın altındaki **Artifacts**.
2. W'ye **SwitchControl-Windows**, M'ye **SwitchControl-macOS** indirin ve zip'leri açın.
3. Bu sürümü denemeden önce eski SwitchControl'ü (pencere ya da terminal) iki tarafta da kapatın.

## 1. İlk açılış

| # | Adım | Beklenen |
|---|---|---|
| 1.1 | W: `switchcontrol.exe`'ye çift tıklayın. "Windows bilgisayarınızı korudu" çıkarsa **Ek bilgi > Yine de çalıştır**. | Yalnızca SwitchControl penceresi açılır; arkada siyah konsol penceresi **açılmaz**. |
| 1.2 | W: Gezgin'de `switchcontrol.exe`'ye ve görev çubuğuna bakın. | Mavi zemin üzerinde iki ekranlı simge. |
| 1.3 | W: Görev Yöneticisi > İşlemler. | "SwitchControl" adıyla görünür. |
| 1.4 | W: Saatin yanındaki bildirim alanı (gerekirse ˄ okuna tıklayın). | SwitchControl simgesi var. |
| 1.5 | M: `SwitchControl.app`'i Uygulamalar'a sürükleyin, **sağ tık > Aç > Aç**. | Pencere açılır; Dock'ta ve menü çubuğunda (sağ üst) simge görünür. |
| 1.6 | M: Pencerede "macOS izinleri gerekli" kartı çıkarsa **Ayarları aç** ile Sistem Ayarları > Gizlilik ve Güvenlik altında **Girdi İzleme** ve **Erişilebilirlik** listelerinde SwitchControl'ü açın. Listede eski bir SwitchControl varsa önce − ile silin. Sonra uygulamadan menü çubuğu simgesi > **Çıkış** ile çıkıp yeniden açın. | Kart kaybolur (iki izin "verildi"). |
| 1.7 | İki tarafta pencerenin görünüşü. | Sistemin açık/koyu temasına uygun; yazılar keskin; üstte iki monitör çizimi. |

## 2. Bağlantı ve eşleştirme

| # | Adım | Beklenen |
|---|---|---|
| 2.1 | W: **Rol: Bağlantı bekle**, **Diğer bilgisayar**: Mac hangi taraftaysa (ör. Sağda). **Başlat**. Windows Güvenlik Duvarı sorarsa izin verin. | Durum "Bağlantı bekleniyor"; altında W'nin IP adresi. |
| 2.2 | M: **Rol: Bağlan**, **Adres**: W'nin IP'si, **Diğer bilgisayar**: W hangi taraftaysa (ör. Solda). **Başlat**. | Daha önce eşleştiyseniz doğrudan "Bağlı". Eşleşmediyseniz W'de 6 haneli PIN, M'de PIN sorusu çıkar. |
| 2.3 | (PIN sorulduysa) W'deki PIN'i M'ye yazıp **Eşleştir**. | İki tarafta da "Bağlı: …"; çizimde yeşil çizgi, klavyenin olduğu bilgisayar mavi çerçeveli. |

## 2b. Ağda otomatik bulma

| # | Adım | Beklenen |
|---|---|---|
| 2b.1 | W'de Bağlantı bekle ile **Başlat**. M'de Rol: **Bağlan**, henüz Başlat'a basmayın. | M'de "Ağda bulunanlar" altında W'nin adı, işletim sistemi ve IP'si birkaç saniye içinde görünür (eşleştiyseniz yeşil "eşleşmiş"). Windows güvenlik duvarı sorarsa izin verin. |
| 2b.2 | M'de listede **Seç**. | Adres alanı dolar. |
| 2b.3 | (Eşleşmiş olmalı) M'de adres alanını boşaltıp **Başlat**. | Durum "Eşleşmiş bilgisayar aranıyor…", birkaç saniye içinde "Bağlı". |
| 2b.4 | W'de **Ağda görünür ol** anahtarını kapatıp (Durdur > kapat > Başlat) M'deki listeye bakın. | En geç 7 saniyede W listeden kaybolur. |

## 3. Geçiş ve kenar çubuğu

Aşağıda klavye ve farenin W'ye bağlı olduğu varsayıldı. Mac'in klavyesini kullanıyorsanız W ile M'yi yer değiştirerek okuyun.

| # | Adım | Beklenen |
|---|---|---|
| 3.1 | İmleci W'de ortak kenara yaklaştırın. | Kenarda mavi, parlayan bir çubuk belirir; yaklaştıkça parlaklaşır, uzaklaşınca kaybolur. |
| 3.2 | Kenara değin. | İmleç M'ye geçer; W'de imleç görünmez olur; W penceresinde "Kontrol … bilgisayarında". |
| 3.3 | M'de fareyle tıklayın, kaydırın, klavyeyle yazın (Türkçe karakterler: ğüşiöç). | Hepsi M'de çalışır. |
| 3.3b | Aynı sayfayı önce W'de, sonra M'de tekerlekle kaydırıp karşılaştırın; sonra W'de **Ayarlar > Kaydırma hızı**'nı değiştirip (Durdur, Başlat) tekrar deneyin. | Bir tık iki tarafta da yaklaşık aynı yol (3 satır) kaydırır. Ayar büyüdükçe M'de kaydırma hızlanır, küçüldükçe yavaşlar. |
| 3.4 | Fareyi geri, ortak kenara götürün. | İmleç W'ye döner, aynı yükseklikte. |
| 3.5 | W'de **Geçiş: Kısa itince** (önce Durdur, değiştirin, Başlat). Kenara değip bekleyin; sonra kenara doğru itmeye devam edin. | Değince hemen geçmez; çubuğun ortasında beyaz dolgu büyür, dolunca geçer. Kenardan çekilirseniz dolgu sıfırlanır. |
| 3.6 | Kenar çubuğu anahtarını kapatıp yeniden başlatın, kenara yaklaşın. | Çubuk görünmez, geçiş yine çalışır. |
| 3.7 | M'deyken **Ctrl+Alt+Shift+Esc** (klavye W'deyse) basın. | Klavye ve fare anında W'ye döner. |

## 4. Tuşlar

| # | Adım | Beklenen |
|---|---|---|
| 4.1 | Klavye W'de, M'deyken: Ctrl+C / Ctrl+V. | M'de kopyala / yapıştır (Ctrl, Cmd olarak gider). |
| 4.2 | Klavye M'de, W'deyken: Cmd+C / Cmd+V. | W'de kopyala / yapıştır. |
| 4.3 | Klavye M'de, W'deyken: **Cmd'ye tek başına basıp bırakın**. | W'de Başlat menüsü açılır. |
| 4.4 | Takılı tuş: klavye M'de, W'deyken W'de Görev Yöneticisi'ni öne getirin; Cmd veya Ctrl'ye basıp bırakın; sonra başka bir pencereye tıklayın. | Hiçbir tuş basılı kalmaz. Kaldıysa en geç birkaç saniyede bırakılır; Günlük'te `key … was still down; released it again` satırı olabilir. |
| 4.5 | Kontrol kendi bilgisayarınızdayken normal yazın. | Hiçbir şey karşı tarafa gitmez (program kendi bilgisayarınızda klavyeyi dinlemiyor). |

## 4b. Pano paylaşımı

İki tarafın da bu sürümle çalışması gerekir (eski sürümle bağlanınca Günlük'te "peer speaks protocol 2, we speak 3" yazar).

| # | Adım | Beklenen |
|---|---|---|
| 4b.1 | Klavyenin olduğu bilgisayarda bir metin kopyalayın, imleci diğer bilgisayara geçirin, orada yapıştırın. | Kopyaladığınız metin yapıştırılır. Günlük: "clipboard sent (N characters)" / "clipboard received (N characters)". |
| 4b.2 | Diğer bilgisayardayken orada başka bir metin kopyalayın, imleci geri getirin, kendi bilgisayarınızda yapıştırın. | O metin yapıştırılır. |
| 4b.3 | Türkçe karakterli ve çok satırlı bir metinle 4b.1'i tekrarlayın. | Metin aynen gelir. |
| 4b.4 | Bir tarafta **Pano paylaşımı** anahtarını kapatıp (Durdur > kapat > Başlat) 4b.1'i tekrarlayın. | Metin aktarılmaz. |

## 5. Tepsi / menü çubuğu simgesi

| # | Adım | Beklenen |
|---|---|---|
| 5.1 | W: simgeye sağ tıklayın. M: menü çubuğundaki simgeye tıklayın. | Menü: durum satırı (ör. "Bağlı: …"), Başlat/Durdur, Pencereyi göster, Çıkış. |
| 5.2 | Fareyi simgenin üstünde bekletin (W). | İpucu: "SwitchControl: <durum>". |
| 5.3 | Pencerenin kapat (X / kırmızı) düğmesine basın. | Pencere kapanır ama program çalışmaya devam eder: geçiş hâlâ çalışır, simge durur. |
| 5.4 | W: simgeye sol tıklayın. M: menüden **Pencereyi göster**. | Pencere geri gelir. |
| 5.5 | Pencere gizliyken menüden **Durdur**, sonra **Başlat**. | Menüdeki durum değişir; Başlat'tan sonra bağlantı geri gelir ve geçiş çalışır. |
| 5.6 | Pencere gizliyken kenara yaklaşın. | Kenar çubuğu yine görünür. |
| 5.7 | Menüden **Çıkış**. | Program tamamen kapanır, simge kaybolur. Karşı tarafta "Bağlantı bekleniyor / Bağlanıyor…". |

## 6. Oturum açılınca başlat

| # | Adım | Beklenen |
|---|---|---|
| 6.1 | Ayarların doğru olduğundan emin olun (W: Bağlantı bekle; M: Bağlan + adres) ve bir kez **Başlat**'a basın (ayarlar böyle kaydedilir). |  |
| 6.2 | **Ayarlar > Uygulama > Oturum açılınca başlat** anahtarını açın (iki tarafta). | Anahtar açık kalır. W: Görev Yöneticisi > Başlangıç uygulamaları'nda SwitchControl görünür. M: Sistem Ayarları > Genel > Giriş Öğeleri'nde görünebilir. |
| 6.3 | İki bilgisayarda oturumu kapatıp yeniden açın (ya da yeniden başlatın). | SwitchControl kendiliğinden simge olarak başlar (pencere bir an görünüp kaybolabilir) ve iki taraf birbirine bağlanır; geçiş çalışır. |
| 6.4 | Anahtarı kapatın, yeniden oturum açın. | SwitchControl kendiliğinden başlamaz. |

## 7. Uyutma, kapatma, uyandırma

Hazırlık (bir kez):
- **M:** Sistem Ayarları > Enerji > **Ağ erişimi için uyandır** açık.
- **W:** Aygıt Yöneticisi > Ağ bağdaştırıcıları > kablolu kart > **Güç Yönetimi**: "Bu aygıtın bilgisayarı uyandırmasına izin ver" ve "Bilgisayarı yalnızca sihirli paket uyandırsın" açık; **Gelişmiş**: "Wake on Magic Packet" etkin. Kapalıyken uyandırma için ayrıca Güç Seçenekleri'nde **Hızlı başlatma** kapalı ve BIOS'ta **Wake on LAN** açık.
- İki taraf da yeni sürüm olmalı; **Oturum açılınca başlat** iki tarafta açık olsun (bölüm 6).

| # | Adım | Beklenen |
|---|---|---|
| 7.1 | Bağlıyken iki tarafta **Ayarlar > Uyandırma adresi (MAC)** alanına bakın. | Alan diğer bilgisayarın kablolu kartının adresiyle dolu (ör. `3C:22:FB:…`). Günlük'te `… can be woken at …` satırı. |
| 7.2 | W'de Bağlantı sayfasında **Uyut**'a basın. | M uyur; W "Bağlanıyor…"/"Bağlantı bekleniyor"a döner ve **Diğer bilgisayarı uyandır** düğmesi çıkar. |
| 7.3 | W'de **Diğer bilgisayarı uyandır**'a basın. | "Uyandırma paketi gönderildi" yazısı; M birkaç saniyede uyanır (ekran açılmayabilir, ağ uyanır) ve iki taraf yeniden bağlanır. |
| 7.4 | M'de **Kapat**'a basın. | "… kapatılsın mı?" sorusu çıkar. **Vazgeç** hiçbir şey yapmaz. |
| 7.5 | M'de **Kapat** > **Evet, kapat**. | W kapanır (kaydedilmemiş iş varsa Windows sorar). |
| 7.6 | M'de **Diğer bilgisayarı uyandır**. | W açılır (BIOS ayarı açıksa), oturum açınca SwitchControl başlar ve bağlanır. |
| 7.7 | W'de **Kapat**'a basın (hedef M). | Soruda turuncu uyarı: kapalı Mac ağdan uyandırılamaz. **Vazgeç** deyin. |
| 7.8 | Simgenin menüsüne bakın, bağlıyken ve değilken. | Bağlıyken "Diğer bilgisayarı uyut/kapat…", değilken "…uyandır" seçilebilir. |

## 8. Gönderilecekler

- Hangi adımlar beklendiği gibi, hangileri değil (numaraları yeterli).
- Sorunlu adımların ekran görüntüsü ve iki taraftaki **Günlük** içeriği.
- Windows sürümü (10 / 11), ekran ölçeği (Ayarlar > Ekran > Ölçek), Mac modeli ve macOS sürümü.
