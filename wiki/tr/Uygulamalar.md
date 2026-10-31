# Yerleşik 15 Uygulama

minOs, tam teşekküllü bir masaüstü deneyimi sunan **15 adet yerleşik uygulama** ve pencere yöneticisi içerir.

| Uygulama | Açıklama |
| :--- | :--- |
| **🔘 Başlat Menüsü** | Kategori menüleri (`Sistem`, `Üretkenlik`, `Medya`, `Güç`) altında simgeli grafik listeleme. Klavye ok tuşları, Enter, fare tekerleği gezintisi ve arama. |
| **📁 Dosya Yöneticisi** | Sanal dosya sistemi (VFS/TarFS) gezgini; dosya türü simgeleri (`[LOG]`, `[TXT]`, `[IMG]`, `[SND]`, `[VID]`), boyutlar ve konum çubuğu. `-UsbDisks` ile bağlanan USB belleğin **FAT32** ve CD'nin **ISO9660** içeriği de listelenir. Masaüstü (`/desktop`) gezgenden bağımsız, sabit bir alandır. |
| **⚙️ Sistem Ayarları** | Beş sekme (Temel / Ekran / Ağ / Aygıtlar / Testler): v2.1.0 sürüm bilgisi ve çekirdek durumu, beş tema, ses düzeyi, gece ışığı, dil (TR/EN), klavye düzeni, yazı yakınlaştırma, hedef çözünürlük ve bölge seçimi; seçilebilir arka plan/duvar kağıdı ve anında önizleme; pencere düzeni kaydetme ve **`settings.cfg` üzerinden kalıcı ayar saklama**. |
| **🎛️ Kontrol Paneli** | Canlı sistem bilgisi: işletim sistemi/çekirdek sürümü, CPU üreticisi ve mantıksal çekirdek sayısı, RAM kullanımı ve doluluk çubuğu, çözünürlük, tema, klavye düzeni, çalışma süresi ve tarih. Ağ sekmesinde e1000 sürücüsü, link durumu, DHCP/IP/maske/ağ geçidi/DNS ve ses sürücüsü; tek düğmeyle tema değiştirme. |
| **📦 Paket Yöneticisi (`minPak`)** | Grafiksel ve komut satırı destekli paket yönetim arayüzü. Depo kataloglarını listeleme, paket arama, yerel/ağ üzerinden paket kurma ve kurulu paketleri yönetme. |
| **📝 Not Defteri** | Üst menü çubuğu (`Dosya`, `Düzen`, `Yardım`), metin temizleme/kaydetme, **Kes / Kopyala / Yapıştır / Tümünü Seç**, **Geri Al / İleri Al** (Ctrl+Z / Ctrl+Y). Türkçe karakter desteği, `Backspace`, `Enter`, **Home / End**, **Shift+ok tuşlarıyla seçim**, satır numaraları ve imleç. |
| **🌐 Web Tarayıcısı** | Sık kullanılanlar (Bookmarks) çubuğu, hafif HTTP test sitesi, `minos://bookmarks`, `minos://help`, `minos://cats`, `minos://net`, `minos://history` ve `minos://demo` / `minos://demo-img` / `minos://demo-video` yerel demo sayfaları, sekmeli gezinme ve e1000 ARP/ping/DNS test paneli. `https://` adresleri (443 portu) **TLS 1.3** el sıkışması ile güvenli bağlantı kurar; sunucu yanıtı AES-128-GCM ile şifresi çözülüp HTTP olarak işlenir. İndirilen sayfalar `/root/Downloads/` altına `.html` uzantısıyla kaydedilir; Dosya Yöneticisi'nden `.html`/`.htm`/`.xhtml` dosyalarına çift tıklandığında tarayıcıda açılır. CSS motoru `@font-face` kurallarını ayrıştırır, `@keyframes` animasyonlarını `opacity`/`color`/`background-color` üzerinde canlandırır; görsel çözücü progresif JPEG destekler. **`Ctrl+Shift+D` ile DevTools açılır; Elements, Console, Network ve Storage sekmeleri tarayıcı durumunu gösterir.** |

DevTools sekmeleri:

- **Elements:** Bağlantı, görsel, canvas, medya, iframe, WASM ve Worker sayaçları.
- **Console:** JavaScript uyarıları ve hata günlüğü.
- **Network:** İstek durumu ve indirme listesi.
- **Storage:** Çerezler ve form otomatik doldurma kayıtları.

### 🎨 CSS & Görsel Desteği

| Özellik | Detay |
| :--- | :--- |
| **Düzen** | `flexbox` / `grid` temel özellikleri, `transform` (translate/scale/rotate), `transition` / `animation` |
| **`@font-face`** | `font-family` ve `src: url(...)` kuralları ayrıştırılır ve `ElementStyle.font_family` üzerinden taşınır. **Sınır:** uzak TTF/WOFF verisi indirilip raster atlassa çevrilmez; metin gömülü atlaslarla çizilir |
| **`@keyframes`** | `blend_styles` ile `opacity`, `color` ve `background-color` lineer interpolasyonu; animasyon yeniden çizimde bastan başlamaz |
| **Görseller** | PNG, GIF, WebP, SVG ve baseline **ve progresif (SOF2) JPEG**; `data:` URI, VFS içi ve uzak görseller |

---

## 📜 minJs JavaScript Motoru

minOs, tarayıcı `<script>` blokları için kullanılan minimal bir istemci tarafı JavaScript motoru içerir (`src/apps/js.rs`, `no_std`). Tam bir ECMAScript uygulaması değildir — sadece basit sayfa betikleri için gereken bir alt kümeyi destekler.

### Desteklenen Özellikler

- Değişkenler: `var`/`let`/`const` atama ve okuma
- Aritmetik: `+`, `-`, `*`, `/`, `%`, parantezler, tekli `+`/`-`/`!`
- Karşılaştırmalar: `!`, `&&`, `||`, `==`/`!=`/`===`/`!==`, `<=`, `>=`, `<`, `>`
- Fonksiyonlar: `function name(a, b) { ... }`, `return`, çağrılar
- Döngüler: `for (init; cond; step)`, `for (item of array)`, `for (key in object)`, `while`, `break`, `continue`
- Diziler: litaller, indeksleme, `.push()`, `.pop()`, `.shift()`, `.unshift()`, `.reverse()`, `.join()`, `.indexOf()`, `.includes()`, `.slice()`, `.concat()`
- Nesneler: litaller, özellik erişimi, `delete`, `Object.keys()`, `Object.values()`
- JSON: `JSON.stringify()`, `JSON.parse()`
- Küresel API'ler: `parseInt()`, `parseFloat()`, `String()`, `Number()`, `Boolean()`, `Math.*`, `String.prototype.*`
- DOM: `getElementById`, `getElementsByClassName`, `innerHTML`, `textContent`, `createElement`, `appendChild`, `setAttribute`, `getAttribute`, `addEventListener`, `dispatchEvent`
- Promises: `new Promise(executor)`, `.then()`, `.catch()`, `await`, `Promise.resolve()`, `Promise.reject()`, `Promise.all()`, `Promise.race()`, `Promise.allSettled()`
- Strict mode: `"use strict"` yönergesi algılanır; bildirilmemiş değişkene atama reddedilir

### Bilinen Sınırlamalar (ECMA-262'ye göre)

| Özellik | Durum | Notlar |
| :--- | :---: | :--- |
| `class` sözdizimi | ✅ Desteklenir | `class Name [extends Parent] { constructor() { } method() { } }`, `new Name(args)`, örnek metod çağrıları |
| Strict mode | ✅ Desteklenir | `"use strict"` algılanır; bildirilmemiş değişkene atama reddedilir |
| `Promise.all/race/allSettled` | ✅ Desteklenir | Promise üzerinde statik metodlar |
| `with` ifadesi | ❌ Desteklenmez | Ayrıştırıcıda yok |
| `eval()` kapsam izolasyonu | ❌ Desteklenmez | `eval` geçerli kapsamı paylaşır |
| `let`/`const` blok kapsamı | ⚠️ Kısım | `var` gibi davranır (fonksiyon/global kapsam) |
| Şablon litalleri | ⚠️ Kısım | Backtick dizgileri desteklenir, `${}` interpolasyonu yok |
| İsteğe bağlı zincirleme (`?.`) | ❌ Desteklenmez | |
| Nullish birleşme (`??`) | ❌ Desteklenmez | |
| Ok fonksiyonları | ❌ Desteklenmez | |
| Üreteçler / async-await | ⚠️ Kısım | `await` Promise değerlerinde desteklenir; `async function` yok |

> **Not:** minJs kasıtlı olarak küçüktür — amacı basit sayfa iyileştirme betiklerini çalıştırmak, tam web uygulaması uyumluluğu değil.

| **🖼️ Resim Gösterici** | Çizim Tahtası'ndan kaydedilen 24-bit `.bmp` çizimlerini ve sanal dosya sistemindeki resimleri açar, yakınlaştırma/uzaklaştırma ve ayrıntılı resim meta verisi paneli sunar; resim yüklenmemişken sevimli kedi maskotunu gösterir. |
| **🎵 Müzik Çalar** | Kullanıcı oynatmayı başlattığında HDA/PC hoparlörü üzerinden ses, animasyonlu spektrum ekolayzerı ve çalma listesi. Sistem açılışında otomatik ses çalınmaz. |
| **🎬 Video Oynatıcı** | Canlı synthwave ufuk sahnesi, neon güneş scanline'ları, aşağı doğru akan 3D perspektif grid yol ve siber araç simülasyonu. |
| **📊 Görev Yöneticisi** | Çekirdek bellek kullanımı, CPU dalgalanma grafiği, çok çekirdekli (SMP) per-core CPU çubukları, görev tablosu, süreç sonlandırma (`Kill`), alfabetik "Program Aç" başlatma menüsü ve RAM doluluk çubuğu. Pencere altına sabitlenmiş durum çubuğu ile ferah performans izleme. |
| **🧮 Hesap Makinesi** | Klavyeyle giriş yapılabilen ifade hesaplayıcı (`+`, `-`, `*`, `/`); 128 bit sabit nokta aritmetiğiyle geniş sayı aralığı ve altı basamağa kadar ondalık hassasiyet. `Backspace` ve `Esc` ile düzenleme/temizleme. |
| **🎨 Çizim Tahtası** | Piksel tabanlı serbest çizim tuvali, renk paleti ve dairesel silgi aracı. Fare basılı tutuldukça **sürekli/akıcı çizim**, fırça kenarının otomatik kırpılması, doğrudan `/root/Documents/cizim.bmp` formatında kaydetme/açma ve 4096 noktalık tuval geçmişi. |
| **⌨️ Komut İstemi (MinShell)** | UNIX/DOS benzeri etkileşimli kabuk: dosya/klasör yönetimi, bellek durumu, ağ teşhisi, `curl` ile HTTP indirme, `format` ile disk biçimlendirme (FAT32/ext4/exFAT/ext2), `pkg`, `exec`, Tab tamamlama ve komut geçmişi. |
| **💽 Tek Tıkla Kur (Kurulum Sihirbazı)** | Adımlı kurulum penceresi: **1)** hedef disk seçimi (ATA sabit disk / QCOW2), **2)** `Biçimlendir (FAT32)` butonu ile diski biçimlendirme ve **3)** `Tek Tıkla Kur` ile kalıcı kurulum. Kurulum; `MINOS.SYS` işaret dosyasını, `BENIOKU.TXT`, mevcut `settings.cfg` ayarlarını ve VFS ağacının tamamını (`VFS.DAT` anlık görüntüsü) diske yazar. |
| **🧊 3D Gösterici** | Küp, silindir, küre ve piramit mesh'lerini dolgulu gölgelendirme veya tel kafes modunda gösterir; sürükleyerek döndürme, yakınlaştırma, otomatik döndürme ve `.3d` dosyalarından şekil/metin yükleme. Arka-yüz eleme ve uzaktan-yakına ressam sıralaması ile doğru katı cisim görünümü. |

> **Not:** `src/apps/` altındaki **Kontrol Paneli** (`controlpanel.rs`) kod tabanında mevcuttur; ayarlar ve kişiselleştirme işlevleri **Sistem Ayarları** (`settings.rs`) üzerinden yürütülmektedir.
